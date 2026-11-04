use std::collections::HashSet;
use std::fs::{File, Metadata, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::Semaphore;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use crate::service::{self, CompressRequest, EngineState};
use crate::watch_registry::{self, WatchRecord};

const SCAN_INTERVAL: Duration = Duration::from_secs(2);
const DRAIN_INTERVAL: Duration = Duration::from_millis(500);
const STABILITY_STEP: Duration = Duration::from_millis(200);
const STABILITY_SAMPLES: u8 = 3;
const STABILITY_TIMEOUT: Duration = Duration::from_secs(60);
const INGEST_CONCURRENCY: usize = 4;

type Pending = Arc<Mutex<HashSet<PathBuf>>>;

#[cfg(all(
    target_os = "linux",
    any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "riscv64",
        target_arch = "loongarch64",
        target_arch = "s390x",
        target_arch = "powerpc64"
    )
))]
const OPEN_NONBLOCK: i32 = 0o4000;
#[cfg(not(all(
    target_os = "linux",
    any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "riscv64",
        target_arch = "loongarch64",
        target_arch = "s390x",
        target_arch = "powerpc64"
    )
)))]
const OPEN_NONBLOCK: i32 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Skip {
    Symlink,
    Diretorio,
    NaoRegular,
    Trocado,
}

impl Skip {
    fn label(self) -> &'static str {
        match self {
            Skip::Symlink => "symlink",
            Skip::Diretorio => "diretorio",
            Skip::NaoRegular => "nao_regular",
            Skip::Trocado => "trocado",
        }
    }

    fn is_state(self) -> bool {
        self != Skip::Trocado
    }
}

enum Entry {
    Regular(Metadata),
    Skipped(Skip),
    Gone,
}

#[derive(Debug)]
enum OpenError {
    Skip(Skip),
    Io(std::io::Error),
}

fn skip_of(meta: &Metadata) -> Option<Skip> {
    let kind = meta.file_type();
    if kind.is_symlink() {
        Some(Skip::Symlink)
    } else if kind.is_file() {
        None
    } else if kind.is_dir() {
        Some(Skip::Diretorio)
    } else {
        Some(Skip::NaoRegular)
    }
}

fn entry_of(path: &Path) -> Entry {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => match skip_of(&meta) {
            Some(motivo) => Entry::Skipped(motivo),
            None => Entry::Regular(meta),
        },
        Err(_) => Entry::Gone,
    }
}

fn open_verified(path: &Path, before: &Metadata) -> Result<(File, Metadata), OpenError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(OPEN_NONBLOCK)
        .open(path)
        .map_err(OpenError::Io)?;
    let after = file.metadata().map_err(OpenError::Io)?;
    if !after.file_type().is_file() || after.dev() != before.dev() || after.ino() != before.ino() {
        return Err(OpenError::Skip(Skip::Trocado));
    }
    Ok((file, after))
}

async fn read_open(file: File) -> std::io::Result<Vec<u8>> {
    tokio::task::spawn_blocking(move || {
        let mut file = file;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        Ok(data)
    })
    .await
    .map_err(std::io::Error::other)?
}

fn screen(path: &Path, seen: &SeenStore) -> Option<Metadata> {
    if !name_allowed(path) {
        return None;
    }
    match entry_of(path) {
        Entry::Regular(meta) => Some(meta),
        Entry::Skipped(motivo) => {
            seen.note_skip(path, motivo);
            None
        }
        Entry::Gone => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    size: u64,
    mtime_ns: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SeenRecord {
    stamp: FileStamp,
    hash: Option<String>,
}

struct SeenStore {
    cache: dashmap::DashMap<PathBuf, SeenRecord>,
    skipped: dashmap::DashMap<PathBuf, Skip>,
    state: Arc<EngineState>,
}

type Seen = Arc<SeenStore>;

impl SeenStore {
    async fn load(state: Arc<EngineState>, dir: &Path) -> Seen {
        let cache = dashmap::DashMap::new();
        match watch_registry::load_all(&state.db).await {
            Ok(rows) => {
                for row in rows {
                    let path = PathBuf::from(&row.path);
                    if path.parent() != Some(dir) {
                        continue;
                    }
                    cache.insert(
                        path,
                        SeenRecord {
                            stamp: FileStamp {
                                size: row.size.max(0) as u64,
                                mtime_ns: row.mtime_ns,
                            },
                            hash: row.object_hash,
                        },
                    );
                }
                info!(arquivos = cache.len(), "registro de ingestão restaurado do banco");
            }
            Err(e) => warn!("registro de ingestão ilegível ({e}); tudo será tratado como novo"),
        }
        Arc::new(SeenStore {
            cache,
            skipped: dashmap::DashMap::new(),
            state,
        })
    }

    fn get(&self, path: &Path) -> Option<SeenRecord> {
        self.cache.get(path).map(|r| r.value().clone())
    }

    fn note_skip(&self, path: &Path, motivo: Skip) {
        if motivo.is_state() && self.skipped.insert(path.to_path_buf(), motivo) == Some(motivo) {
            return;
        }
        self.state.metrics.record_watch_skipped(motivo.label());
        let nome = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        warn!(
            arquivo = ?nome,
            motivo = motivo.label(),
            "item do diretório observado ignorado: só arquivos regulares são ingeridos"
        );
    }

    fn retain_skipped(&self) {
        self.skipped
            .retain(|path, motivo| matches!(entry_of(path), Entry::Skipped(agora) if agora == *motivo));
    }

    async fn put(&self, path: &Path, record: SeenRecord) {
        self.cache.insert(path.to_path_buf(), record.clone());
        let Some(text) = path.to_str() else {
            return;
        };
        let row = WatchRecord {
            path: text.to_string(),
            size: i64::try_from(record.stamp.size).unwrap_or(i64::MAX),
            mtime_ns: record.stamp.mtime_ns,
            object_hash: record.hash,
            seen_at_ms: crate::container::now_millis(),
        };
        if let Err(e) = watch_registry::upsert(&self.state.db, &row).await {
            warn!(arquivo = %path.display(), erro = %e, "registro de ingestão não persistido");
        }
    }

    async fn forget(&self, path: &Path) {
        self.cache.remove(path);
        if let Some(text) = path.to_str() {
            if let Err(e) = watch_registry::remove(&self.state.db, text).await {
                warn!(arquivo = %path.display(), erro = %e, "registro de ingestão não removido");
            }
        }
    }

    async fn retain_present(&self, presentes: &HashSet<PathBuf>) {
        let ausentes: Vec<PathBuf> = self
            .cache
            .iter()
            .filter(|e| !presentes.contains(e.key()))
            .map(|e| e.key().clone())
            .collect();
        for path in ausentes {
            self.forget(&path).await;
        }
    }
}

pub fn start(state: Arc<EngineState>) {
    let dir = PathBuf::from(&state.config.watch_dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        error!("não foi possível criar {}: {e}; ingestão desativada", dir.display());
        return;
    }

    let pending: Pending = Arc::new(Mutex::new(HashSet::new()));
    let inflight = Arc::new(dashmap::DashMap::<PathBuf, ()>::new());

    info!(diretorio = %dir.display(), "ingestão por diretório ativa");

    tokio::spawn(async move {
        let seen = SeenStore::load(state.clone(), &dir).await;
        spawn_notify(dir.clone(), pending.clone(), seen.clone());
        spawn_scanner(dir, pending.clone(), inflight.clone(), seen.clone());
        spawn_drainer(state, pending, inflight, seen);
    });
}

fn spawn_notify(dir: PathBuf, pending: Pending, seen: Seen) {
    std::thread::Builder::new()
        .name("syntra-notify".into())
        .spawn(move || {
            use notify::{EventKind, RecursiveMode, Watcher};

            let (tx, rx) = crossbeam_channel::unbounded();
            let mut watcher = match notify::RecommendedWatcher::new(
                move |res| {
                    let _ = tx.send(res);
                },
                notify::Config::default(),
            ) {
                Ok(w) => w,
                Err(e) => {
                    warn!("notify indisponível ({e}); a varredura periódica cobre a ingestão");
                    return;
                }
            };

            if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
                warn!("falha ao observar {}: {e}; seguindo só com varredura", dir.display());
                return;
            }

            while let Ok(result) = rx.recv() {
                let Ok(event) = result else { continue };
                if !matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Access(_)
                ) {
                    continue;
                }
                let admitted: Vec<PathBuf> = event
                    .paths
                    .into_iter()
                    .filter(|path| screen(path, &seen).is_some())
                    .collect();
                if !admitted.is_empty() {
                    pending.lock().extend(admitted);
                }
            }
        })
        .map(|_| ())
        .unwrap_or_else(|e| error!("falha ao criar thread do notify: {e}"));
}

fn spawn_scanner(
    dir: PathBuf,
    pending: Pending,
    inflight: Arc<dashmap::DashMap<PathBuf, ()>>,
    seen: Seen,
) {
    tokio::spawn(async move {
        let mut ticker = interval(SCAN_INTERVAL);
        loop {
            ticker.tick().await;

            let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
                continue;
            };
            let mut found = Vec::new();
            let mut presentes = HashSet::new();

            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if !name_allowed(&path) {
                    continue;
                }
                let Ok(meta) = tokio::fs::symlink_metadata(&path).await else {
                    continue;
                };
                if let Some(motivo) = skip_of(&meta) {
                    seen.note_skip(&path, motivo);
                    continue;
                }
                presentes.insert(path.clone());

                if inflight.contains_key(&path) {
                    continue;
                }
                if seen.get(&path).map(|r| r.stamp) == Some(stamp_of(&meta)) {
                    continue;
                }
                found.push(path);
            }

            seen.retain_present(&presentes).await;
            seen.retain_skipped();

            if !found.is_empty() {
                let mut queue = pending.lock();
                queue.extend(found);
            }
        }
    });
}

fn stamp_of(meta: &std::fs::Metadata) -> FileStamp {
    FileStamp {
        size: meta.len(),
        mtime_ns: meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_nanos()).ok()),
    }
}

fn spawn_drainer(
    state: Arc<EngineState>,
    pending: Pending,
    inflight: Arc<dashmap::DashMap<PathBuf, ()>>,
    seen: Seen,
) {
    let gate = Arc::new(Semaphore::new(INGEST_CONCURRENCY));

    tokio::spawn(async move {
        let mut ticker = interval(DRAIN_INTERVAL);
        loop {
            ticker.tick().await;

            let batch: Vec<PathBuf> = {
                let mut queue = pending.lock();
                if queue.is_empty() {
                    continue;
                }
                queue.drain().collect()
            };

            debug!(arquivos = batch.len(), "lote de ingestão despachado");

            for path in batch {
                if inflight.insert(path.clone(), ()).is_some() {
                    continue;
                }

                let state = state.clone();
                let gate = gate.clone();
                let inflight = inflight.clone();
                let seen = seen.clone();

                tokio::spawn(async move {
                    let _permit = gate.acquire().await;
                    if let Err(e) = ingest(&state, &path, &seen).await {
                        error!(arquivo = %path.display(), erro = %e, "falha na ingestão");
                    }
                    inflight.remove(&path);
                });
            }
        }
    });
}

async fn ingest(state: &Arc<EngineState>, path: &Path, seen: &Seen) -> anyhow::Result<()> {
    if screen(path, seen).is_none() {
        return Ok(());
    }
    if wait_until_stable(path).await.is_none() {
        return Ok(());
    }
    let Some(before) = screen(path, seen) else {
        return Ok(());
    };

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "sem-nome".to_string());

    let (file, meta) = match open_verified(path, &before) {
        Ok(opened) => opened,
        Err(OpenError::Skip(motivo)) => {
            seen.note_skip(path, motivo);
            return Ok(());
        }
        Err(OpenError::Io(e)) => {
            warn!(arquivo = %name, erro = %e, "arquivo ilegível; será tentado na próxima varredura");
            return Ok(());
        }
    };
    let stamp = stamp_of(&meta);
    let previous = seen.get(path);
    if previous.as_ref().map(|r| r.stamp == stamp).unwrap_or(false) {
        return Ok(());
    }
    if stamp.size == 0 {
        debug!(arquivo = %path.display(), "arquivo vazio ignorado");
        seen.put(path, SeenRecord { stamp, hash: None }).await;
        return Ok(());
    }

    let data = match read_open(file).await {
        Ok(d) => d,
        Err(e) => {
            warn!(arquivo = %name, erro = %e, "arquivo ilegível; será tentado na próxima varredura");
            return Ok(());
        }
    };

    if let Some(known) = previous.and_then(|r| r.hash) {
        let digest = hex::encode(blake3::hash(&data).as_bytes());
        if digest == known {
            debug!(arquivo = %name, "conteúdo idêntico ao já arquivado; só a data de modificação mudou");
            seen.put(path, SeenRecord { stamp, hash: Some(digest) }).await;
            return Ok(());
        }
    }

    let mut req = CompressRequest::from_config(&state.config, name.clone());
    req.persist = true;

    let ctx = state.default_context();
    let result = service::compress(state, &ctx, data, req).await?;

    info!(
        arquivo = %name,
        id = result.id,
        plano = service::codec_label(&result.envelope),
        economia_pct = format!("{:.1}", result.savings_pct()),
        dedup = result.deduplicated,
        "arquivado pela ingestão"
    );

    if state.config.watch_delete_source {
        match tokio::fs::remove_file(path).await {
            Ok(()) => {
                seen.forget(path).await;
            }
            Err(e) => {
                warn!(arquivo = %name, erro = %e, "essência gravada, mas o original não pôde ser removido");
                seen.put(path, SeenRecord { stamp, hash: Some(result.id) }).await;
            }
        }
    } else {
        seen.put(path, SeenRecord { stamp, hash: Some(result.id) }).await;
    }

    Ok(())
}

async fn wait_until_stable(path: &Path) -> Option<u64> {
    let deadline = tokio::time::Instant::now() + STABILITY_TIMEOUT;
    let mut last: Option<u64> = None;
    let mut stable = 0u8;

    loop {
        let meta = tokio::fs::symlink_metadata(path).await.ok()?;
        if !meta.file_type().is_file() {
            return None;
        }
        let size = meta.len();

        if Some(size) == last {
            stable += 1;
            if stable >= STABILITY_SAMPLES {
                return Some(size);
            }
        } else {
            stable = 1;
            last = Some(size);
        }

        if tokio::time::Instant::now() >= deadline {
            warn!(
                arquivo = %path.display(),
                "tamanho não estabilizou em {}s; adiado",
                STABILITY_TIMEOUT.as_secs()
            );
            return None;
        }
        tokio::time::sleep(STABILITY_STEP).await;
    }
}

#[cfg(test)]
fn is_candidate(path: &Path) -> bool {
    name_allowed(path) && matches!(entry_of(path), Entry::Regular(_))
}

fn name_allowed(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with('.') || name.starts_with('~') {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    !(lower.ends_with(".tmp")
        || lower.ends_with(".part")
        || lower.ends_with(".crdownload")
        || lower.ends_with(".swp")
        || lower.ends_with(".syntra"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtra_arquivos_que_nao_devem_ser_ingeridos() {
        let dir = std::env::temp_dir().join(format!("syntra-watch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let criar = |nome: &str| {
            let p = dir.join(nome);
            std::fs::write(&p, b"x").unwrap();
            p
        };

        assert!(is_candidate(&criar("relatorio.csv")));
        assert!(is_candidate(&criar("dados.json")));

        assert!(!is_candidate(&criar(".oculto")));
        assert!(!is_candidate(&criar("~temporario")));
        assert!(!is_candidate(&criar("parcial.tmp")));
        assert!(!is_candidate(&criar("download.part")));
        assert!(!is_candidate(&criar("chrome.crdownload")));
        assert!(!is_candidate(&criar("vim.swp")));
        assert!(!is_candidate(&criar("abc.syntra")));
        let sub = dir.join("subdir");
        std::fs::create_dir_all(&sub).unwrap();
        assert!(!is_candidate(&sub));
        assert!(!is_candidate(&dir.join("nao-existe.txt")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn espera_o_arquivo_estabilizar() {
        let dir = std::env::temp_dir().join(format!("syntra-stable-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("crescendo.bin");
        std::fs::write(&path, vec![0u8; 100]).unwrap();

        let p2 = path.clone();
        tokio::spawn(async move {
            for i in 1..=3 {
                tokio::time::sleep(Duration::from_millis(120)).await;
                let _ = std::fs::write(&p2, vec![0u8; 100 + i * 100]);
            }
        });

        let size = wait_until_stable(&path).await.expect("deveria estabilizar");
        assert_eq!(size, 400, "deveria ler o tamanho final, não o parcial");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn identidade_do_arquivo_detecta_alteracao() {
        let dir = std::env::temp_dir().join(format!("syntra-stamp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.txt");

        std::fs::write(&path, b"conteudo original").unwrap();
        let antes = stamp_of(&std::fs::metadata(&path).unwrap());

        assert_eq!(antes, stamp_of(&std::fs::metadata(&path).unwrap()));

        std::fs::write(&path, b"conteudo original mais longo").unwrap();
        assert_ne!(antes, stamp_of(&std::fs::metadata(&path).unwrap()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn arquivo_inexistente_nao_estabiliza() {
        assert!(wait_until_stable(Path::new("/tmp/syntra-nao-existe-jamais")).await.is_none());
    }

    fn csv_de(linhas: usize) -> Vec<u8> {
        let mut s = String::from("id,data,regiao,valor\n");
        for i in 0..linhas {
            s.push_str(&format!("{},2026-03-{:02},sudeste,{}.{:02}\n", i, (i % 28) + 1, i * 7, i % 100));
        }
        s.into_bytes()
    }

    fn raiz_de_teste(nome: &str) -> PathBuf {
        let raiz = std::env::temp_dir().join(format!(
            "syntra-watch-{}-{}-{}",
            nome,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&raiz).unwrap();
        raiz
    }

    async fn motor_em(raiz: &Path) -> Arc<EngineState> {
        let config = crate::config::Config {
            vault_path: raiz.join("vault").to_string_lossy().to_string(),
            sled_index_path: raiz.join("index.sled").to_string_lossy().to_string(),
            dict_path: raiz.join("dicts").to_string_lossy().to_string(),
            db_url: format!("sqlite:{}", raiz.join("syntra.db").display()),
            watch_dir: raiz.join("watch").to_string_lossy().to_string(),
            bind_addr: "127.0.0.1:0".to_string(),
            max_body_size_mb: 64,
            max_decompressed_mb: 64,
            cors_origins: vec![],
            api_keys: vec![],
            compress_multiplier: 2,
            default_effort: crate::planner::Effort::Fast,
            verify_on_write: true,
            dictionaries_enabled: false,
            redis_url: None,
            limits: Default::default(),
            watch_enabled: false,
            watch_delete_source: false,
        };
        Arc::new(EngineState::new(config).await.expect("motor não subiu"))
    }

    async fn referencias(state: &Arc<EngineState>, conteudo: &[u8]) -> i64 {
        let id = hex::encode(blake3::hash(conteudo).as_bytes());
        crate::tenancy::TenancyStore::find_grant(&state.db, &state.default_tenant.id, &id)
            .await
            .unwrap()
            .map(|g| g.refs)
            .unwrap_or(0)
    }

    async fn eventos(state: &Arc<EngineState>) -> i64 {
        crate::usage::totals(&state.db, &state.default_tenant.id, 0, i64::MAX)
            .await
            .unwrap()
            .events
    }

    #[tokio::test]
    async fn reinicio_nao_reingere_arquivo_inalterado_mas_reingere_o_modificado() {
        let raiz = raiz_de_teste("reinicio");
        let dir = raiz.join("watch");
        std::fs::create_dir_all(&dir).unwrap();
        let arquivo = dir.join("vendas.csv");
        let v1 = csv_de(2000);
        std::fs::write(&arquivo, &v1).unwrap();

        let state = motor_em(&raiz).await;
        let seen = SeenStore::load(state.clone(), &dir).await;
        ingest(&state, &arquivo, &seen).await.unwrap();
        assert_eq!(referencias(&state, &v1).await, 1);
        assert_eq!(eventos(&state).await, 1);

        ingest(&state, &arquivo, &seen).await.unwrap();
        assert_eq!(eventos(&state).await, 1, "mesmo processo: arquivo inalterado");

        drop(seen);
        let seen = SeenStore::load(state.clone(), &dir).await;
        ingest(&state, &arquivo, &seen).await.unwrap();
        assert_eq!(referencias(&state, &v1).await, 1, "reinício do watcher: sem nova referência");
        assert_eq!(eventos(&state).await, 1, "reinício do watcher: sem novo evento");

        drop(seen);
        drop(state);
        let state = motor_em(&raiz).await;
        let seen = SeenStore::load(state.clone(), &dir).await;
        ingest(&state, &arquivo, &seen).await.unwrap();
        assert_eq!(referencias(&state, &v1).await, 1, "reinício do motor: sem nova referência");
        assert_eq!(eventos(&state).await, 1, "reinício do motor: sem novo evento");

        let v2 = csv_de(2600);
        std::fs::write(&arquivo, &v2).unwrap();
        ingest(&state, &arquivo, &seen).await.unwrap();
        assert_eq!(referencias(&state, &v2).await, 1, "arquivo modificado vira novo objeto");
        assert_eq!(referencias(&state, &v1).await, 1);
        assert_eq!(eventos(&state).await, 2);

        drop(seen);
        let seen = SeenStore::load(state.clone(), &dir).await;
        ingest(&state, &arquivo, &seen).await.unwrap();
        assert_eq!(eventos(&state).await, 2, "versão modificada também sobrevive ao reinício");

        let novo = dir.join("outro.csv");
        let v3 = csv_de(900);
        std::fs::write(&novo, &v3).unwrap();
        ingest(&state, &novo, &seen).await.unwrap();
        assert_eq!(referencias(&state, &v3).await, 1, "arquivo novo é ingerido");
        assert_eq!(eventos(&state).await, 3);

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[tokio::test]
    async fn so_a_data_de_modificacao_mudar_nao_gera_nova_referencia() {
        let raiz = raiz_de_teste("mtime");
        let dir = raiz.join("watch");
        std::fs::create_dir_all(&dir).unwrap();
        let arquivo = dir.join("dados.csv");
        let conteudo = csv_de(1500);
        std::fs::write(&arquivo, &conteudo).unwrap();

        let state = motor_em(&raiz).await;
        let seen = SeenStore::load(state.clone(), &dir).await;
        ingest(&state, &arquivo, &seen).await.unwrap();
        assert_eq!(eventos(&state).await, 1);

        let futuro = std::time::SystemTime::now() + Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&arquivo)
            .unwrap()
            .set_modified(futuro)
            .unwrap();

        ingest(&state, &arquivo, &seen).await.unwrap();
        assert_eq!(referencias(&state, &conteudo).await, 1);
        assert_eq!(eventos(&state).await, 1, "conteúdo idêntico não pode gerar evento");

        drop(seen);
        let seen = SeenStore::load(state.clone(), &dir).await;
        assert_eq!(
            seen.get(&arquivo).map(|r| r.stamp),
            Some(stamp_of(&std::fs::metadata(&arquivo).unwrap())),
            "o novo mtime deve ter sido persistido"
        );
        ingest(&state, &arquivo, &seen).await.unwrap();
        assert_eq!(eventos(&state).await, 1);

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[tokio::test]
    async fn arquivo_removido_sai_do_registro_persistido() {
        let raiz = raiz_de_teste("removido");
        let dir = raiz.join("watch");
        std::fs::create_dir_all(&dir).unwrap();
        let arquivo = dir.join("efemero.csv");
        std::fs::write(&arquivo, csv_de(300)).unwrap();

        let state = motor_em(&raiz).await;
        let seen = SeenStore::load(state.clone(), &dir).await;
        ingest(&state, &arquivo, &seen).await.unwrap();
        assert!(seen.get(&arquivo).is_some());

        std::fs::remove_file(&arquivo).unwrap();
        seen.retain_present(&HashSet::new()).await;
        assert!(seen.get(&arquivo).is_none());

        let recarregado = SeenStore::load(state.clone(), &dir).await;
        assert!(recarregado.get(&arquivo).is_none(), "o registro removido não pode voltar");

        let _ = std::fs::remove_dir_all(&raiz);
    }

    fn pular(state: &Arc<EngineState>, motivo: &str) -> u64 {
        state
            .metrics
            .watch_skipped()
            .into_iter()
            .find(|(m, _)| *m == motivo)
            .map(|(_, n)| n)
            .unwrap_or(0)
    }

    fn mkfifo(path: &Path) {
        let status = std::process::Command::new("mkfifo").arg(path).status().unwrap();
        assert!(status.success(), "mkfifo falhou");
    }

    async fn esperar_eventos(state: &Arc<EngineState>, alvo: i64, limite: Duration) -> bool {
        let fim = tokio::time::Instant::now() + limite;
        while tokio::time::Instant::now() < fim {
            if eventos(state).await >= alvo {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        false
    }

    #[tokio::test]
    async fn link_simbolico_para_arquivo_nunca_e_ingerido_e_o_regular_ao_lado_continua() {
        let raiz = raiz_de_teste("link-arquivo");
        let dir = raiz.join("watch");
        std::fs::create_dir_all(&dir).unwrap();
        let segredo = csv_de(800);
        let alvo = raiz.join("segredo.csv");
        std::fs::write(&alvo, &segredo).unwrap();
        let link = dir.join("link.csv");
        std::os::unix::fs::symlink(&alvo, &link).unwrap();
        let regular = dir.join("regular.csv");
        let conteudo = csv_de(500);
        std::fs::write(&regular, &conteudo).unwrap();

        let state = motor_em(&raiz).await;
        let seen = SeenStore::load(state.clone(), &dir).await;

        ingest(&state, &link, &seen).await.unwrap();
        assert_eq!(eventos(&state).await, 0, "o link não pode virar evento de compressão");
        assert_eq!(referencias(&state, &segredo).await, 0, "o alvo do link não pode virar objeto");
        assert!(seen.get(&link).is_none(), "o link não entra no registro de ingestão");
        assert_eq!(pular(&state, "symlink"), 1);

        ingest(&state, &regular, &seen).await.unwrap();
        assert_eq!(eventos(&state).await, 1, "o arquivo regular ao lado segue sendo ingerido");
        assert_eq!(referencias(&state, &conteudo).await, 1);
        assert_eq!(referencias(&state, &segredo).await, 0);

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[tokio::test]
    async fn link_para_diretorio_link_quebrado_e_laco_de_links_sao_ignorados() {
        let raiz = raiz_de_teste("link-outros");
        let dir = raiz.join("watch");
        std::fs::create_dir_all(&dir).unwrap();
        let pasta_fora = raiz.join("pasta-fora");
        std::fs::create_dir_all(&pasta_fora).unwrap();
        let dentro = csv_de(400);
        std::fs::write(pasta_fora.join("dentro.csv"), &dentro).unwrap();

        let link_dir = dir.join("link-dir");
        std::os::unix::fs::symlink(&pasta_fora, &link_dir).unwrap();
        let quebrado = dir.join("quebrado.csv");
        std::os::unix::fs::symlink(raiz.join("nao-existe.csv"), &quebrado).unwrap();
        let laco_a = dir.join("laco-a.csv");
        let laco_b = dir.join("laco-b.csv");
        std::os::unix::fs::symlink("laco-b.csv", &laco_a).unwrap();
        std::os::unix::fs::symlink("laco-a.csv", &laco_b).unwrap();
        let subdir = dir.join("subdir");
        std::fs::create_dir_all(&subdir).unwrap();
        let no_subdir = csv_de(300);
        std::fs::write(subdir.join("aninhado.csv"), &no_subdir).unwrap();

        let state = motor_em(&raiz).await;
        let seen = SeenStore::load(state.clone(), &dir).await;

        for caminho in [&link_dir, &quebrado, &laco_a, &laco_b, &subdir] {
            ingest(&state, caminho, &seen).await.unwrap();
        }
        assert_eq!(eventos(&state).await, 0);
        assert_eq!(referencias(&state, &dentro).await, 0, "o conteúdo da pasta apontada não pode ser lido");
        assert_eq!(referencias(&state, &no_subdir).await, 0, "subdiretório não é percorrido");
        assert_eq!(pular(&state, "symlink"), 4);
        assert_eq!(pular(&state, "diretorio"), 1);

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[tokio::test]
    async fn fifo_e_dispositivo_sao_ignorados_sem_bloquear() {
        let raiz = raiz_de_teste("fifo");
        let dir = raiz.join("watch");
        std::fs::create_dir_all(&dir).unwrap();
        let fifo = dir.join("canal.csv");
        mkfifo(&fifo);
        let regular = dir.join("regular.csv");
        let conteudo = csv_de(200);
        std::fs::write(&regular, &conteudo).unwrap();

        assert!(matches!(entry_of(&fifo), Entry::Skipped(Skip::NaoRegular)));
        assert!(matches!(entry_of(Path::new("/dev/null")), Entry::Skipped(Skip::NaoRegular)));
        assert!(matches!(entry_of(&dir), Entry::Skipped(Skip::Diretorio)));
        assert!(matches!(entry_of(&regular), Entry::Regular(_)));
        assert!(!is_candidate(&fifo));

        let state = motor_em(&raiz).await;
        let seen = SeenStore::load(state.clone(), &dir).await;

        tokio::time::timeout(Duration::from_secs(10), ingest(&state, &fifo, &seen))
            .await
            .expect("ingest ficou preso no FIFO")
            .unwrap();
        assert_eq!(eventos(&state).await, 0);
        assert_eq!(pular(&state, "nao_regular"), 1);

        ingest(&state, &regular, &seen).await.unwrap();
        assert_eq!(referencias(&state, &conteudo).await, 1);

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[tokio::test]
    async fn a_varredura_ignora_o_que_nao_e_regular_conta_uma_vez_e_ingere_o_arquivo_regular() {
        let raiz = raiz_de_teste("varredura");
        let dir = raiz.join("watch");
        std::fs::create_dir_all(&dir).unwrap();
        let segredo = csv_de(700);
        let alvo = raiz.join("segredo.csv");
        std::fs::write(&alvo, &segredo).unwrap();
        let pasta_fora = raiz.join("pasta-fora");
        std::fs::create_dir_all(&pasta_fora).unwrap();
        let dentro = csv_de(600);
        std::fs::write(pasta_fora.join("dentro.csv"), &dentro).unwrap();
        let aninhado = csv_de(500);
        std::fs::create_dir_all(dir.join("subdir")).unwrap();
        std::fs::write(dir.join("subdir").join("aninhado.csv"), &aninhado).unwrap();

        std::os::unix::fs::symlink(&alvo, dir.join("link-arquivo.csv")).unwrap();
        std::os::unix::fs::symlink(&pasta_fora, dir.join("link-pasta")).unwrap();
        std::os::unix::fs::symlink(raiz.join("nao-existe.csv"), dir.join("quebrado.csv")).unwrap();
        std::os::unix::fs::symlink("laco-b.csv", dir.join("laco-a.csv")).unwrap();
        std::os::unix::fs::symlink("laco-a.csv", dir.join("laco-b.csv")).unwrap();
        mkfifo(&dir.join("canal.csv"));
        let regular = csv_de(400);
        std::fs::write(dir.join("regular.csv"), &regular).unwrap();

        let state = motor_em(&raiz).await;
        let seen = SeenStore::load(state.clone(), &dir).await;
        let pending: Pending = Arc::new(Mutex::new(HashSet::new()));
        let inflight = Arc::new(dashmap::DashMap::<PathBuf, ()>::new());
        spawn_scanner(dir.clone(), pending.clone(), inflight.clone(), seen.clone());
        spawn_drainer(state.clone(), pending, inflight, seen.clone());

        assert!(
            esperar_eventos(&state, 1, Duration::from_secs(20)).await,
            "o arquivo regular deveria ser ingerido pela varredura"
        );
        tokio::time::sleep(SCAN_INTERVAL * 3).await;

        assert_eq!(eventos(&state).await, 1, "só o arquivo regular pode virar evento");
        assert_eq!(referencias(&state, &regular).await, 1);
        assert_eq!(referencias(&state, &segredo).await, 0, "alvo de link lido");
        assert_eq!(referencias(&state, &dentro).await, 0, "pasta apontada por link lida");
        assert_eq!(referencias(&state, &aninhado).await, 0, "subdiretório percorrido");
        assert_eq!(pular(&state, "symlink"), 4, "contagem por sighting, sem repetir a cada varredura");
        assert_eq!(pular(&state, "nao_regular"), 1);
        assert_eq!(pular(&state, "diretorio"), 1);

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[tokio::test]
    async fn a_notificacao_do_inotify_nao_enfileira_links_nem_especiais() {
        let raiz = raiz_de_teste("inotify");
        let dir = raiz.join("watch");
        std::fs::create_dir_all(&dir).unwrap();
        let alvo = raiz.join("segredo.csv");
        std::fs::write(&alvo, csv_de(300)).unwrap();

        let state = motor_em(&raiz).await;
        let seen = SeenStore::load(state.clone(), &dir).await;
        let pending: Pending = Arc::new(Mutex::new(HashSet::new()));
        spawn_notify(dir.clone(), pending.clone(), seen.clone());
        tokio::time::sleep(Duration::from_millis(500)).await;

        std::os::unix::fs::symlink(&alvo, dir.join("link.csv")).unwrap();
        mkfifo(&dir.join("canal.csv"));
        std::fs::create_dir_all(dir.join("subdir")).unwrap();
        let regular = dir.join("regular.csv");
        std::fs::write(&regular, csv_de(200)).unwrap();

        let fim = tokio::time::Instant::now() + Duration::from_secs(10);
        while !pending.lock().contains(&regular) && tokio::time::Instant::now() < fim {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;

        let fila = pending.lock().clone();
        assert!(fila.contains(&regular), "o arquivo regular deveria ser enfileirado: {fila:?}");
        assert_eq!(fila.len(), 1, "só o arquivo regular pode entrar na fila: {fila:?}");
        assert_eq!(pular(&state, "symlink"), 1);
        assert_eq!(pular(&state, "nao_regular"), 1);
        assert_eq!(pular(&state, "diretorio"), 1);

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[tokio::test]
    async fn arquivo_enfileirado_que_virou_link_antes_da_ingestao_nao_e_lido() {
        let raiz = raiz_de_teste("troca-fila");
        let dir = raiz.join("watch");
        std::fs::create_dir_all(&dir).unwrap();
        let segredo = csv_de(900);
        let alvo = raiz.join("segredo.csv");
        std::fs::write(&alvo, &segredo).unwrap();
        let caminho = dir.join("dados.csv");
        std::fs::write(&caminho, csv_de(100)).unwrap();

        let state = motor_em(&raiz).await;
        let seen = SeenStore::load(state.clone(), &dir).await;
        assert!(screen(&caminho, &seen).is_some(), "enfileirado como arquivo regular");

        std::fs::remove_file(&caminho).unwrap();
        std::os::unix::fs::symlink(&alvo, &caminho).unwrap();

        ingest(&state, &caminho, &seen).await.unwrap();
        assert_eq!(eventos(&state).await, 0);
        assert_eq!(referencias(&state, &segredo).await, 0);
        assert_eq!(pular(&state, "symlink"), 1);

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn troca_do_arquivo_regular_entre_verificar_e_abrir_e_detectada_pelo_descritor() {
        let raiz = raiz_de_teste("troca-abertura");
        let alvo = raiz.join("alvo.txt");
        std::fs::write(&alvo, b"conteudo de outro arquivo").unwrap();
        let caminho = raiz.join("dados.csv");

        let verificar = |caminho: &Path| match entry_of(caminho) {
            Entry::Regular(meta) => meta,
            _ => panic!("deveria ser regular"),
        };

        std::fs::write(&caminho, b"original").unwrap();
        let antes = verificar(&caminho);
        let (mut file, meta) = open_verified(&caminho, &antes).expect("arquivo intacto abre");
        let mut lido = String::new();
        file.read_to_string(&mut lido).unwrap();
        assert_eq!(lido, "original");
        assert_eq!(meta.len(), 8);

        let antes = verificar(&caminho);
        std::fs::remove_file(&caminho).unwrap();
        std::os::unix::fs::symlink(&alvo, &caminho).unwrap();
        match open_verified(&caminho, &antes) {
            Err(OpenError::Skip(Skip::Trocado)) => {}
            outro => panic!("troca por link deveria ser recusada: {outro:?}"),
        }

        std::fs::remove_file(&caminho).unwrap();
        std::fs::write(&caminho, b"original").unwrap();
        let antes = verificar(&caminho);
        std::fs::remove_file(&caminho).unwrap();
        std::fs::write(&caminho, b"outro arquivo regular").unwrap();
        match open_verified(&caminho, &antes) {
            Err(OpenError::Skip(Skip::Trocado)) => {}
            outro => panic!("troca por outro inode deveria ser recusada: {outro:?}"),
        }

        std::fs::remove_file(&caminho).unwrap();
        std::fs::write(&caminho, b"original").unwrap();
        let antes = verificar(&caminho);
        std::fs::remove_file(&caminho).unwrap();
        mkfifo(&caminho);
        let (tx, rx) = std::sync::mpsc::channel();
        let p = caminho.clone();
        std::thread::spawn(move || {
            let r = open_verified(&p, &antes).map(|_| ());
            let _ = tx.send(r);
        });
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Err(OpenError::Skip(Skip::Trocado))) => {}
            Ok(outro) => panic!("troca por FIFO deveria ser recusada: {outro:?}"),
            Err(_) => panic!("open_verified bloqueou ao abrir um FIFO"),
        }

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[tokio::test]
    async fn o_mesmo_item_so_conta_uma_vez_e_volta_a_contar_se_sumir_e_reaparecer() {
        let raiz = raiz_de_teste("dedupe");
        let dir = raiz.join("watch");
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join("link.csv");
        std::os::unix::fs::symlink(raiz.join("qualquer"), &link).unwrap();

        let state = motor_em(&raiz).await;
        let seen = SeenStore::load(state.clone(), &dir).await;
        seen.note_skip(&link, Skip::Symlink);
        seen.note_skip(&link, Skip::Symlink);
        seen.retain_skipped();
        seen.note_skip(&link, Skip::Symlink);
        assert_eq!(pular(&state, "symlink"), 1);

        std::fs::remove_file(&link).unwrap();
        seen.retain_skipped();
        std::os::unix::fs::symlink(raiz.join("qualquer"), &link).unwrap();
        seen.note_skip(&link, Skip::Symlink);
        assert_eq!(pular(&state, "symlink"), 2);

        seen.note_skip(&link, Skip::Trocado);
        seen.note_skip(&link, Skip::Trocado);
        assert_eq!(pular(&state, "trocado"), 2, "troca é evento, não estado: conta sempre");

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn os_rotulos_do_motivo_existem_na_metrica() {
        for motivo in [Skip::Symlink, Skip::Diretorio, Skip::NaoRegular, Skip::Trocado] {
            assert!(
                crate::metrics::WATCH_SKIP_REASONS.contains(&motivo.label()),
                "{} não está em WATCH_SKIP_REASONS",
                motivo.label()
            );
        }
    }
}
