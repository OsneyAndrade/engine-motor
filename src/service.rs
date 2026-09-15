use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::http::StatusCode;
use tokio::sync::Semaphore;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::container::{self, ContainerError, Restored};
use crate::content::{self, ContentProfile};
use crate::db_monitor::MonitorDB;
use crate::dict_store::DictionaryStore;
use crate::metrics::EngineMetrics;
use crate::planner::{self, Effort, ForcedPlan, Measurement};
use crate::proto::ProcessedFile;
use crate::tenancy::{Scope, Tenant, TenancyStore, TenantContext};
use crate::usage::{self, DedupScope, Operation, UsageEvent};
use crate::vault::VaultManager;

pub struct EngineState {
    pub config: Config,
    pub sem_compress: Arc<Semaphore>,
    pub sem_io: Arc<Semaphore>,
    pub dictionaries: Arc<DictionaryStore>,
    pub metrics: EngineMetrics,
    pub pool: Arc<rayon::ThreadPool>,
    pub vault: Arc<VaultManager>,
    pub db: MonitorDB,
    pub max_compress_permits: usize,
    pub default_tenant: Tenant,
    auth_required: AtomicBool,
}

impl EngineState {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        let threads = std::env::var("RAYON_NUM_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|n| *n > 0)
            .unwrap_or_else(num_cpus::get);

        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("syntra-worker-{i}"))
            .build()?;

        std::fs::create_dir_all(&config.vault_path)?;
        std::fs::create_dir_all(&config.watch_dir)?;

        let vault = VaultManager::new(&config.vault_path, &config.sled_index_path)?;

        let redis = match &config.redis_url {
            Some(url) => match redis::Client::open(url.as_str()) {
                Ok(c) => Some(c),
                Err(e) => {
                    warn!("SYNTRA_REDIS_URL inválida ({e}); seguindo sem cluster de dicionários");
                    None
                }
            },
            None => None,
        };

        let dictionaries =
            DictionaryStore::open(&config.dict_path, config.dictionaries_enabled, redis)?;

        let db = MonitorDB::new(&config.db_url).await?;
        let now = container::now_millis();
        let default_tenant = TenancyStore::ensure_default_tenant(&db, now).await?;
        let issued_keys = TenancyStore::count_keys(&db).await.unwrap_or(0);
        let auth_required = config.auth_enabled() || issued_keys > 0;

        let cpus = num_cpus::get();

        Ok(Self {
            sem_compress: Arc::new(Semaphore::new(cpus)),
            sem_io: Arc::new(Semaphore::new(cpus * config.compress_multiplier)),
            dictionaries: Arc::new(dictionaries),
            metrics: EngineMetrics::new(),
            pool: Arc::new(pool),
            vault: Arc::new(vault),
            db,
            max_compress_permits: cpus,
            default_tenant,
            auth_required: AtomicBool::new(auth_required),
            config,
        })
    }

    pub fn auth_required(&self) -> bool {
        self.auth_required.load(Ordering::Relaxed)
    }

    pub fn mark_auth_required(&self) {
        self.auth_required.store(true, Ordering::Relaxed);
    }

    pub fn default_context(&self) -> TenantContext {
        TenantContext {
            tenant: self.default_tenant.clone(),
            key_id: None,
            scopes: Scope::all(),
        }
    }
}

#[derive(Debug)]
pub enum EngineError {
    NotFound { message: String },
    Container(ContainerError),
    Io(String),
    Internal(String),
    Overloaded,
}

impl EngineError {
    pub fn not_found(message: impl Into<String>) -> Self {
        EngineError::NotFound {
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        EngineError::Internal(message.into())
    }

    pub fn code(&self) -> &'static str {
        match self {
            EngineError::NotFound { .. } => "not_found",
            EngineError::Container(c) => c.code(),
            EngineError::Io(_) => "io_error",
            EngineError::Internal(_) => "internal_error",
            EngineError::Overloaded => "overloaded",
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            EngineError::NotFound { .. } => StatusCode::NOT_FOUND,
            EngineError::Container(c) => match c {
                ContainerError::Decode(_) | ContainerError::Malformed(_) => StatusCode::BAD_REQUEST,
                ContainerError::MissingDictionary { .. } => StatusCode::CONFLICT,
                _ => StatusCode::UNPROCESSABLE_ENTITY,
            },
            EngineError::Io(_) | EngineError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EngineError::Overloaded => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::NotFound { message } => write!(f, "{message}"),
            EngineError::Container(c) => write!(f, "{c}"),
            EngineError::Io(m) => write!(f, "falha de I/O: {m}"),
            EngineError::Internal(m) => write!(f, "erro interno: {m}"),
            EngineError::Overloaded => write!(f, "motor saturado; tente novamente"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<ContainerError> for EngineError {
    fn from(e: ContainerError) -> Self {
        EngineError::Container(e)
    }
}

impl From<std::io::Error> for EngineError {
    fn from(e: std::io::Error) -> Self {
        EngineError::Io(e.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct CompressRequest {
    pub filename: String,
    pub mime_hint: Option<String>,
    pub effort: Effort,
    pub forced: ForcedPlan,
    pub verify: bool,
    pub persist: bool,
    pub allow_dedup: bool,
    pub idempotency_key: Option<String>,
    pub request_id: Option<String>,
}

impl CompressRequest {
    pub fn from_config(config: &Config, filename: impl Into<String>) -> Self {
        CompressRequest {
            filename: filename.into(),
            mime_hint: None,
            effort: config.default_effort,
            forced: ForcedPlan::default(),
            verify: config.verify_on_write,
            persist: true,
            allow_dedup: true,
            idempotency_key: None,
            request_id: None,
        }
    }
}

pub struct CompressResult {
    pub id: String,
    pub envelope: ProcessedFile,
    pub bytes: Vec<u8>,
    pub deduplicated: bool,
    pub sampled_decision: bool,
    pub references: Option<i64>,
    pub total_ms: f64,
    pub compress_ms: f64,
    pub verify_ms: f64,
    pub replayed: bool,
}

impl CompressResult {
    pub fn savings_pct(&self) -> f64 {
        if self.envelope.original_size == 0 {
            return 0.0;
        }
        (1.0 - self.envelope.processed_size as f64 / self.envelope.original_size as f64) * 100.0
    }

}

struct BlockingOutcome {
    hash: [u8; 32],
    envelope: ProcessedFile,
    bytes: Vec<u8>,
    blob_existed: bool,
    sampled_decision: bool,
    compress_ms: f64,
    verify_ms: f64,
}

pub async fn compress(
    state: &Arc<EngineState>,
    ctx: &TenantContext,
    data: Vec<u8>,
    req: CompressRequest,
) -> Result<CompressResult, EngineError> {
    let started = std::time::Instant::now();
    let original_size = data.len() as u64;
    let tenant_id = ctx.tenant_id().to_string();

    if let Some(key) = &req.idempotency_key {
        if let Some(prev) = usage::find_by_idempotency(&state.db, &tenant_id, key)
            .await
            .map_err(|e| EngineError::internal(e.to_string()))?
        {
            return replay(state, &tenant_id, prev, started).await;
        }
    }

    let _permit = state
        .sem_compress
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| EngineError::Overloaded)?;

    let st = state.clone();
    let req_clone = req.clone();
    let pool = state.pool.clone();

    let outcome = tokio::task::spawn_blocking(move || {
        pool.install(|| compress_blocking(&st, data, &req_clone))
    })
    .await
    .map_err(|e| EngineError::internal(format!("tarefa de compressão abortada: {e}")))??;

    drop(_permit);

    let hash_hex = hex::encode(outcome.hash);
    let now = container::now_millis();

    let mut references = None;
    let mut had_grant = false;

    if req.persist {
        had_grant = TenancyStore::find_grant(&state.db, &tenant_id, &hash_hex)
            .await
            .map_err(|e| EngineError::internal(e.to_string()))?
            .is_some();

        let (refs, _) = TenancyStore::add_grant(
            &state.db,
            &tenant_id,
            &hash_hex,
            &req.filename,
            outcome.envelope.original_size as i64,
            outcome.envelope.processed_size as i64,
            now,
        )
        .await
        .map_err(|e| EngineError::internal(e.to_string()))?;
        references = Some(refs);
    }

    let dedup_scope = if had_grant {
        DedupScope::Tenant
    } else if outcome.blob_existed {
        DedupScope::Global
    } else {
        DedupScope::None
    };

    let m = &state.metrics;
    m.files_processed.fetch_add(1, Ordering::Relaxed);
    m.bytes_in.fetch_add(original_size, Ordering::Relaxed);
    m.bytes_out
        .fetch_add(outcome.envelope.processed_size, Ordering::Relaxed);
    m.record_latency(outcome.compress_ms);
    m.record_codec(outcome.envelope.codec);
    if dedup_scope != DedupScope::None {
        m.dedup_hits.fetch_add(1, Ordering::Relaxed);
        m.dedup_bytes_saved
            .fetch_add(outcome.envelope.processed_size, Ordering::Relaxed);
    }

    let saved = (outcome.envelope.original_size as i64 - outcome.envelope.processed_size as i64).max(0);

    if req.persist {
        let event = UsageEvent {
            tenant_id: tenant_id.clone(),
            key_id: ctx.key_id.clone(),
            idempotency_key: req.idempotency_key.clone(),
            operation: Operation::Compress,
            object_hash: Some(hash_hex.clone()),
            bytes_in: original_size as i64,
            bytes_out: outcome.envelope.processed_size as i64,
            bytes_saved: saved,
            effort: req.effort.as_str().to_string(),
            codec: codec_label(&outcome.envelope),
            cpu_ms: outcome.compress_ms + outcome.verify_ms,
            dedup_scope,
            request_id: req.request_id.clone(),
            occurred_at_ms: now,
        };
        if let Err(e) = usage::record(&state.db, &event).await {
            error!("falha ao gravar evento de uso: {e}");
        }
    }

    let total_ms = started.elapsed().as_secs_f64() * 1000.0;
    info!(
        tenant = tenant_id,
        id = hash_hex,
        arquivo = req.filename,
        classe = outcome.envelope.content_class,
        plano = codec_label(&outcome.envelope),
        entrada = original_size,
        saida = outcome.envelope.processed_size,
        dedup = dedup_scope.as_str(),
        verificado = outcome.envelope.verified,
        ms = format!("{total_ms:.1}"),
        "essência gerada"
    );

    Ok(CompressResult {
        id: hash_hex,
        envelope: outcome.envelope,
        bytes: outcome.bytes,
        deduplicated: had_grant,
        sampled_decision: outcome.sampled_decision,
        references,
        total_ms,
        compress_ms: outcome.compress_ms,
        verify_ms: outcome.verify_ms,
        replayed: false,
    })
}

async fn replay(
    state: &Arc<EngineState>,
    tenant_id: &str,
    prev: usage::StoredEvent,
    started: std::time::Instant,
) -> Result<CompressResult, EngineError> {
    let hash_hex = prev
        .object_hash
        .clone()
        .ok_or_else(|| EngineError::internal("evento idempotente sem objeto associado"))?;
    let hash = VaultManager::parse_id(&hash_hex)
        .map_err(|e| EngineError::internal(e.to_string()))?;

    let envelope = load_object(state, tenant_id, &hash).await?;
    let bytes = load_container(state, tenant_id, &hash).await?;
    let references = TenancyStore::find_grant(&state.db, tenant_id, &hash_hex)
        .await
        .ok()
        .flatten()
        .map(|g| g.refs);

    info!(
        tenant = tenant_id,
        id = hash_hex,
        "requisição idempotente reaproveitada sem novo processamento"
    );

    Ok(CompressResult {
        id: hash_hex,
        envelope,
        bytes,
        deduplicated: true,
        sampled_decision: false,
        references,
        total_ms: started.elapsed().as_secs_f64() * 1000.0,
        compress_ms: prev.cpu_ms,
        verify_ms: 0.0,
        replayed: true,
    })
}

fn compress_blocking(
    state: &Arc<EngineState>,
    data: Vec<u8>,
    req: &CompressRequest,
) -> Result<BlockingOutcome, EngineError> {
    let hash: [u8; 32] = blake3::hash(&data).into();
    let id = hex::encode(hash);

    if req.persist && req.allow_dedup && state.vault.contains(&hash) {
        let path = state.vault.path_for(&hash);
        match std::fs::read(&path) {
            Ok(bytes) => match container::parse(&bytes) {
                Ok(envelope) => {
                    return Ok(BlockingOutcome {
                        hash,
                        envelope,
                        bytes,
                        blob_existed: true,
                        sampled_decision: false,
                        compress_ms: 0.0,
                        verify_ms: 0.0,
                    });
                }
                Err(e) => warn!(
                    id = id,
                    erro = %e,
                    "essência existente ilegível; regravando a partir do original"
                ),
            },
            Err(e) => warn!(id = id, erro = %e, "essência existente não pôde ser lida"),
        }
    }

    let profile: ContentProfile =
        content::classify(&data, &req.filename, req.mime_hint.as_deref());

    let dict = state.dictionaries.active_for(&profile.dictionary_key);
    let dict_bytes = dict.as_ref().map(|d| d.bytes.as_slice());

    let t_compress = std::time::Instant::now();
    let planned = planner::compress_best(
        &data,
        &profile,
        req.effort,
        dict_bytes,
        &req.forced,
        &state.config.limits,
    )
    .map_err(|e| EngineError::internal(format!("compressão falhou: {e}")))?;
    let compress_ms = t_compress.elapsed().as_secs_f64() * 1000.0;
    let sampled_decision = planned.sampled_decision;

    let mut verify_ms = 0.0;
    let mut verified = false;
    if req.verify {
        match container::verify_roundtrip(&planned, &data, dict_bytes) {
            Ok(ms) => {
                verify_ms = ms;
                verified = true;
            }
            Err(e) => {
                state.metrics.verify_failures.fetch_add(1, Ordering::Relaxed);
                error!(
                    id = id,
                    plano = planned.plan.label(),
                    erro = %e,
                    "verificação de round-trip falhou; essência descartada"
                );
                return Err(EngineError::Container(e));
            }
        }
    }

    state.dictionaries.observe(&profile.dictionary_key, &data);

    let envelope = container::build(
        planned,
        &profile,
        &req.filename,
        &hash,
        dict.as_deref(),
        compress_ms,
        verified,
        verify_ms,
    );
    let sealed = container::seal(envelope)?;

    if req.persist {
        let path = state
            .vault
            .ensure_path(&hash)
            .map_err(|e| EngineError::internal(e.to_string()))?;
        write_atomic(&path, &sealed.bytes)?;
        state
            .vault
            .record(&hash, &req.filename, sealed.envelope.original_size,
                    sealed.envelope.processed_size, container::now_millis())
            .map_err(|e| EngineError::internal(e.to_string()))?;
    }

    Ok(BlockingOutcome {
        hash,
        envelope: sealed.envelope,
        bytes: sealed.bytes,
        blob_existed: false,
        sampled_decision,
        compress_ms,
        verify_ms,
    })
}

fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("syntra.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

pub async fn reconstruct_bytes(
    state: &Arc<EngineState>,
    bytes: Vec<u8>,
) -> Result<(ProcessedFile, Restored), EngineError> {
    let envelope = container::parse(&bytes)?;
    let restored = reconstruct(state, envelope.clone()).await?;
    Ok((envelope, restored))
}

pub async fn reconstruct(
    state: &Arc<EngineState>,
    envelope: ProcessedFile,
) -> Result<Restored, EngineError> {
    let _permit = state
        .sem_compress
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| EngineError::Overloaded)?;

    let dicts = state.dictionaries.clone();
    let pool = state.pool.clone();

    let restored = tokio::task::spawn_blocking(move || {
        pool.install(|| container::restore(&envelope, dicts.as_ref()))
    })
    .await
    .map_err(|e| EngineError::internal(format!("tarefa de reconstrução abortada: {e}")))??;

    state.metrics.record_latency(restored.duration_ms);
    state.metrics.reconstructions.fetch_add(1, Ordering::Relaxed);

    Ok(restored)
}

async fn require_grant(
    state: &Arc<EngineState>,
    tenant_id: &str,
    hash: &[u8; 32],
) -> Result<(), EngineError> {
    let hash_hex = hex::encode(hash);
    let grant = TenancyStore::find_grant(&state.db, tenant_id, &hash_hex)
        .await
        .map_err(|e| EngineError::internal(e.to_string()))?;
    if grant.is_none() {
        return Err(EngineError::not_found(format!(
            "objeto {hash_hex} não encontrado"
        )));
    }
    Ok(())
}

pub async fn load_object(
    state: &Arc<EngineState>,
    tenant_id: &str,
    hash: &[u8; 32],
) -> Result<ProcessedFile, EngineError> {
    require_grant(state, tenant_id, hash).await?;

    let _permit = state
        .sem_io
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| EngineError::Overloaded)?;

    let path = state.vault.path_for(hash);
    let id = hex::encode(hash);

    let bytes = tokio::task::spawn_blocking(move || std::fs::read(&path))
        .await
        .map_err(|e| EngineError::internal(format!("leitura abortada: {e}")))?
        .map_err(|_| EngineError::not_found(format!("objeto {id} não encontrado no vault")))?;

    Ok(container::parse(&bytes)?)
}

pub async fn load_container(
    state: &Arc<EngineState>,
    tenant_id: &str,
    hash: &[u8; 32],
) -> Result<Vec<u8>, EngineError> {
    require_grant(state, tenant_id, hash).await?;

    let path = state.vault.path_for(hash);
    let id = hex::encode(hash);
    tokio::task::spawn_blocking(move || std::fs::read(&path))
        .await
        .map_err(|e| EngineError::internal(format!("leitura abortada: {e}")))?
        .map_err(|_| EngineError::not_found(format!("objeto {id} não encontrado no vault")))
}

pub async fn record_read(
    state: &Arc<EngineState>,
    ctx: &TenantContext,
    hash_hex: &str,
    bytes_out: u64,
    operation: Operation,
    request_id: Option<String>,
) {
    let event = UsageEvent {
        tenant_id: ctx.tenant_id().to_string(),
        key_id: ctx.key_id.clone(),
        idempotency_key: None,
        operation,
        object_hash: Some(hash_hex.to_string()),
        bytes_in: 0,
        bytes_out: bytes_out as i64,
        bytes_saved: 0,
        effort: String::new(),
        codec: String::new(),
        cpu_ms: 0.0,
        dedup_scope: DedupScope::None,
        request_id,
        occurred_at_ms: container::now_millis(),
    };
    if let Err(e) = usage::record(&state.db, &event).await {
        error!("falha ao gravar evento de leitura: {e}");
    }
}

pub struct AnalysisReport {
    pub profile: ContentProfile,
    pub effort: Effort,
    pub probe_size: usize,
    pub measurements: Vec<Measurement>,
    pub dictionary_id: Option<String>,
}

pub async fn analyze(
    state: &Arc<EngineState>,
    data: Vec<u8>,
    filename: String,
    mime_hint: Option<String>,
    effort: Effort,
) -> Result<AnalysisReport, EngineError> {
    let _permit = state
        .sem_compress
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| EngineError::Overloaded)?;

    let st = state.clone();
    let pool = state.pool.clone();

    tokio::task::spawn_blocking(move || {
        pool.install(|| {
            let profile = content::classify(&data, &filename, mime_hint.as_deref());
            let dict = st.dictionaries.active_for(&profile.dictionary_key);
            let measurements = planner::measure_all(
                &data,
                &profile,
                effort,
                dict.as_ref().map(|d| d.bytes.as_slice()),
                &st.config.limits,
            );
            let probe_size = data.len().min(st.config.limits.probe_sample);
            AnalysisReport {
                profile,
                effort,
                probe_size,
                measurements,
                dictionary_id: dict.map(|d| d.id.clone()),
            }
        })
    })
    .await
    .map_err(|e| EngineError::internal(format!("tarefa de análise abortada: {e}")))
}

pub struct DeleteOutcome {
    pub id: String,
    pub remaining_refs: Option<i64>,
    pub blob_removed: bool,
}

pub async fn delete_object(
    state: &Arc<EngineState>,
    ctx: &TenantContext,
    hash: [u8; 32],
    purge: bool,
) -> Result<DeleteOutcome, EngineError> {
    let hash_hex = hex::encode(hash);
    let tenant_id = ctx.tenant_id().to_string();
    let now = container::now_millis();

    let remaining = TenancyStore::release_grant(&state.db, &tenant_id, &hash_hex, purge, now)
        .await
        .map_err(|e| EngineError::internal(e.to_string()))?
        .ok_or_else(|| EngineError::not_found(format!("objeto {hash_hex} não encontrado")))?;

    let mut blob_removed = false;
    if remaining == 0 {
        let others = TenancyStore::count_grants_for_object(&state.db, &hash_hex)
            .await
            .map_err(|e| EngineError::internal(e.to_string()))?;

        if others == 0 {
            let vault = state.vault.clone();
            blob_removed = tokio::task::spawn_blocking(move || vault.purge(&hash))
                .await
                .map_err(|e| EngineError::internal(format!("tarefa de remoção abortada: {e}")))?
                .map_err(|e| EngineError::internal(e.to_string()))?;
            let _ = state.vault.flush();
        }
    }

    record_read(state, ctx, &hash_hex, 0, Operation::Delete, None).await;

    Ok(DeleteOutcome {
        id: hash_hex,
        remaining_refs: if remaining > 0 { Some(remaining) } else { None },
        blob_removed,
    })
}

pub async fn quote_usage(
    state: &Arc<EngineState>,
    ctx: &TenantContext,
    from_ms: i64,
    to_ms: i64,
) -> Result<(usage::UsageTotals, usage::Quote), EngineError> {
    let totals = usage::totals(&state.db, ctx.tenant_id(), from_ms, to_ms)
        .await
        .map_err(|e| EngineError::internal(e.to_string()))?;
    let quote = usage::quote(ctx.tenant.plan, &ctx.tenant.price, &totals);
    Ok((totals, quote))
}

pub fn codec_label(envelope: &ProcessedFile) -> String {
    envelope
        .file_metrics
        .as_ref()
        .map(|m| m.algorithm_used.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            crate::codec::Codec::try_from(envelope.codec)
                .map(|c| crate::codec::name(c).to_string())
                .unwrap_or_else(|_| "desconhecido".to_string())
        })
}
