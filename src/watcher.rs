use std::collections::HashSet;
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
        Arc::new(SeenStore { cache, state })
    }

    fn get(&self, path: &Path) -> Option<SeenRecord> {
        self.cache.get(path).map(|r| r.value().clone())
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

    spawn_notify(dir.clone(), pending.clone());

    info!(diretorio = %dir.display(), "ingestão por diretório ativa");

    tokio::spawn(async move {
        let seen = SeenStore::load(state.clone(), &dir).await;
        spawn_scanner(dir, pending.clone(), inflight.clone(), seen.clone());
        spawn_drainer(state, pending, inflight, seen);
    });
}

fn spawn_notify(dir: PathBuf, pending: Pending) {
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
                let mut queue = pending.lock();
                for path in event.paths {
                    if is_candidate(&path) {
                        queue.insert(path);
                    }
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
                if !is_candidate(&path) {
                    continue;
                }
                presentes.insert(path.clone());

                if inflight.contains_key(&path) {
                    continue;
                }
                if let (Some(anterior), Ok(meta)) =
                    (seen.get(&path).map(|r| r.stamp), entry.metadata().await)
                {
                    if anterior == stamp_of(&meta) {
                        continue;
                    }
                }
                found.push(path);
            }

            seen.retain_present(&presentes).await;

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
    if wait_until_stable(path).await.is_none() {
        return Ok(());
    }

    let Ok(meta) = tokio::fs::metadata(path).await else {
        return Ok(());
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

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "sem-nome".to_string());

    let data = match tokio::fs::read(path).await {
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
        let size = tokio::fs::metadata(path).await.ok()?.len();

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

fn is_candidate(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
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
}
