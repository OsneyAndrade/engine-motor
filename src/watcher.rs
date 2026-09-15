use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::Semaphore;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use crate::service::{self, CompressRequest, EngineState};

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
    mtime: Option<std::time::SystemTime>,
}

type Seen = Arc<dashmap::DashMap<PathBuf, FileStamp>>;

pub fn start(state: Arc<EngineState>) {
    let dir = PathBuf::from(&state.config.watch_dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        error!("não foi possível criar {}: {e}; ingestão desativada", dir.display());
        return;
    }

    let pending: Pending = Arc::new(Mutex::new(HashSet::new()));
    let inflight = Arc::new(dashmap::DashMap::<PathBuf, ()>::new());
    let seen: Seen = Arc::new(dashmap::DashMap::new());

    spawn_notify(dir.clone(), pending.clone());
    spawn_scanner(dir.clone(), pending.clone(), inflight.clone(), seen.clone());
    spawn_drainer(state, pending, inflight, seen);

    info!(diretorio = %dir.display(), "ingestão por diretório ativa");
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
                    (seen.get(&path).map(|v| *v), entry.metadata().await)
                {
                    if anterior == stamp_of(&meta) {
                        continue;
                    }
                }
                found.push(path);
            }

            seen.retain(|path, _| presentes.contains(path));

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
        mtime: meta.modified().ok(),
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
    if seen.get(path).map(|v| *v == stamp).unwrap_or(false) {
        return Ok(());
    }
    if stamp.size == 0 {
        debug!(arquivo = %path.display(), "arquivo vazio ignorado");
        seen.insert(path.to_path_buf(), stamp);
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
                seen.remove(path);
            }
            Err(e) => {
                warn!(arquivo = %name, erro = %e, "essência gravada, mas o original não pôde ser removido");
                seen.insert(path.to_path_buf(), stamp);
            }
        }
    } else {
        seen.insert(path.to_path_buf(), stamp);
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
}
