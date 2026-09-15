use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use notify::{Watcher, RecursiveMode, Event, EventKind};
use tracing::{info, warn, error};
use tokio::time::{Duration, interval};
use prost::Message;
use std::time::Instant;
use std::collections::HashSet;
use parking_lot::Mutex;

use crate::EngineState;
use crate::adaptive;
use crate::compress;
use crate::proto::{FileMetrics, ProcessedFile};

pub fn start_directory_watcher(state: Arc<EngineState>, watch_dir: impl AsRef<Path>) {
    let watch_dir = watch_dir.as_ref().to_path_buf();
    let _ = std::fs::create_dir_all(&watch_dir);

    // Buffer de coleta para lote (extremamente escalável)
    let batch_buffer = Arc::new(Mutex::new(HashSet::<PathBuf>::new()));

    // Rastreador de processamento ativo (debounce temporal)
    let active_processing = Arc::new(dashmap::DashMap::<PathBuf, Instant>::new());

    let buffer_for_notify = batch_buffer.clone();
    let active_for_notify = active_processing.clone();
    let watch_dir_for_notify = watch_dir.clone();

    std::thread::spawn(move || {
        info!("Iniciando Batch Watcher (Notify) em: {}", watch_dir_for_notify.display());

        let config = notify::Config::default().with_poll_interval(Duration::from_millis(100));

        let mut watcher = match notify::RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Access(notify::event::AccessKind::Close(_)) => {
                            let mut buffer = buffer_for_notify.lock();
                            for path in event.paths {
                                if path.is_file() {
                                    // Debounce básico
                                    if let Some(last_time) = active_for_notify.get(&path) {
                                        if last_time.elapsed() < Duration::from_millis(500) {
                                            continue;
                                        }
                                    }
                                    buffer.insert(path);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            },
            config
        ) {
            Ok(w) => w,
            Err(e) => {
                warn!("Notify watcher falhou: {} - polling continuará", e);
                return;
            }
        };

        match watcher.watch(&watch_dir_for_notify, RecursiveMode::NonRecursive) {
            Ok(_) => {
                // Mantém a thread viva com um loop apropriado
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
            Err(e) => {
                warn!("Falha ao iniciar watch: {}", e);
            }
        }
    });

    let watch_dir_clone = watch_dir.clone();
    let buffer_for_polling = batch_buffer.clone();
    let active_for_poll = active_processing.clone();
    let processed_files = Arc::new(dashmap::DashMap::<PathBuf, std::time::SystemTime>::new());

    tokio::spawn(async move {
        info!("Iniciando Polling Watcher (Batch Mode) em: {} (intervalo: 2s)", watch_dir_clone.display());
        let mut poll_timer = interval(Duration::from_secs(2));

        loop {
            poll_timer.tick().await;

            let mut entries = match fs::read_dir(&watch_dir_clone).await {
                Ok(e) => e,
                Err(_) => continue,
            };

            let mut found_paths = Vec::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_file() {
                    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                    // Ignora arquivos ocultos, temporários
                    if file_name.starts_with('.') || file_name.ends_with(".tmp") {
                        continue;
                    }

                    let file_meta = match tokio::fs::metadata(&path).await {
                        Ok(meta) => meta,
                        Err(_) => continue,
                    };

                    let modified_time = file_meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);

                    // Registra para processamento (sempre processa, não verifica modified_time anterior)
                    if !active_for_poll.contains_key(&path) {
                        found_paths.push((path, modified_time));
                    }
                }
            }

            if !found_paths.is_empty() {
                let mut buffer = buffer_for_polling.lock();
                for (path, modified_time) in found_paths {
                    buffer.insert(path.clone());
                    processed_files.insert(path, modified_time);
                }
            }
        }
    });

    let batch_drain = batch_buffer.clone();
    let state_drain = state.clone();
    let active_drain = active_processing.clone();

    tokio::spawn(async move {
        let mut drain_interval = interval(Duration::from_millis(500));
        loop {
            drain_interval.tick().await;

            // Take everything from the buffer
            let tasks: Vec<PathBuf> = {
                let mut buffer = batch_drain.lock();
                if buffer.is_empty() {
                    continue;
                }
                let items = buffer.drain().collect();
                items
            };

            info!("Batch Ingestion: Draining {} files for processing...", tasks.len());

            for path in tasks {
                let state_inner = state_drain.clone();
                let active_inner = active_drain.clone();
                let path_inner = path.clone();

                active_inner.insert(path.clone(), Instant::now());

                tokio::spawn(async move {
                    if let Err(e) = process_watched_file(path, state_inner).await {
                        error!("Batch Worker Error [{}]: {:?}", path_inner.display(), e);
                    }
                    active_inner.remove(&path_inner);
                });
            }
        }
    });
}

async fn process_watched_file(path: PathBuf, state: Arc<EngineState>) -> anyhow::Result<()> {
    // 1. O SO as vezes dispara o evento Create antes do arquivo terminar de gravar
    // Vamos esperar o arquivo estabilizar (tempo NÃO contado nas métricas)
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Se o arquivo tiver sumido no meio do sleep (deletado rápido ou movido), aborta.
    if !path.exists() {
        return Ok(());
    }

    let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    info!("Watcher detectou arquivo: {}", file_name);

    // 2. Tenta ler os bytes do arquivo
    let data = match fs::read(&path).await {
        Ok(d) => d,
        Err(e) => {
            warn!("Arquivo travado ou ilegível ({}): {}", file_name, e);
            return Ok(());
        }
    };

    if data.is_empty() {
        return Ok(());
    }

    let _total_raw_size = data.len() as u64;

    let hash = blake3::hash(&data);
    let hash_bytes: [u8; 32] = *hash.as_bytes();
    let hash_hex = hex::encode(hash_bytes);

    // ID único para evitar conflitos de nomes (arquivos com mesmo conteúdo)
    let unique_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    let syn_filename = format!("{}_{}.syntra", hash_hex, unique_id);

    // Cria diretório do vault (ensure_dir retorna o caminho genérico, vamos usar o parent para pegar só o diretório)
    let vault_dir = state.vault.ensure_dir(&hash_bytes)?;

    // Constrói o caminho completo com o nome único do arquivo
    let vault_path = vault_dir.parent().unwrap().join(&syn_filename);

    let start_latency = Instant::now();
    let mime = "application/octet-stream";
    let analysis = adaptive::analyze(&data, mime);
    let algo = analysis.algorithm;
    let lvl = analysis.zstd_level;
    let dict = state.dictionaries.get_dictionary(mime);

    let total_raw_size = data.len() as u64;

    // Comprime
    let compressed = state.pool.install(|| {
        compress::compress(&data, algo, lvl, dict.as_deref())
    }).map_err(|e| anyhow::anyhow!("Erro de compressão: {}", e))?;

    let processed_size = compressed.len() as u64;
    let savings_pct = if total_raw_size > 0 {
        (1.0 - (processed_size as f64 / total_raw_size as f64)) * 100.0
    } else { 0.0 };

    let duration_ms = start_latency.elapsed().as_secs_f64() * 1000.0;

    // Registra latência nas métricas
    state.metrics.record_latency(duration_ms);

    let envelope = ProcessedFile {
        version: 2,
        original_name: file_name.clone(),
        original_names: vec![file_name.clone()],
        original_mime: mime.to_string(),
        original_size: total_raw_size,
        processed_size,
        savings_pct: savings_pct as f32,
        algorithm: algo as i32,
        compression_level: lvl,
        dictionary_id: String::new(),
        data: compressed,
        blocks: vec![],
        file_metrics: Some(FileMetrics {
            throughput_mbps: 0.0,
            compression_ratio: if total_raw_size > 0 { processed_size as f64 / total_raw_size as f64 } else { 1.0 },
            dedup_savings_bytes: 0,
            processing_time_ms: duration_ms,
            algorithm_used: format!("{:?}", algo),
            strategy_reason: "Watcher".to_string(),
        }),
        pointer_to: String::new(),
        file_hash: hash_bytes.to_vec(),
        ref_count: 1,
        envelope_checksum: 0,
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
        updated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
    };

    let mut buf = Vec::with_capacity(envelope.encoded_len());
    if let Err(e) = envelope.encode(&mut buf) {
        warn!("Falha ao codificar envelope para {}: {}", file_name, e);
        return Ok(());
    }

    let checksum = blake3::hash(&buf);
    let mut envelope_with_checksum = envelope.clone();
    envelope_with_checksum.envelope_checksum = u64::from_be_bytes(checksum.as_bytes()[0..8].try_into().unwrap_or([0u8; 8]));

    let mut buf_final = Vec::with_capacity(envelope_with_checksum.encoded_len());
    if let Err(e) = envelope_with_checksum.encode(&mut buf_final) {
        warn!("Falha ao codificar envelope com checksum: {}", e);
        return Ok(());
    }

    // Cria ou sobrescreve o arquivo no vault
    // Isso permite enviar o mesmo arquivo múltiplas vezes
    match fs::File::create(&vault_path).await {
        Ok(mut file) => {
            use tokio::io::AsyncWriteExt;
            if let Err(e) = file.write_all(&buf_final).await {
                warn!("Falha ao escrever no vault: {}", e);
                return Ok(());
            }
        }
        Err(e) => {
            warn!("Falha ao criar arquivo no vault: {}", e);
            let _ = fs::remove_file(&path).await;
            return Ok(());
        }
    }

    state.metrics.bytes_in.fetch_add(total_raw_size, std::sync::atomic::Ordering::Relaxed);
    state.metrics.bytes_out.fetch_add(processed_size, std::sync::atomic::Ordering::Relaxed);
    state.metrics.files_processed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let _ = state.vault.store_hash(&hash_bytes, &syn_filename);

    // ID no banco sem extensão .syntra
    let db_id = syn_filename.trim_end_matches(".syntra");
    let _ = state.db.log_file(
        &db_id,
        &file_name,
        mime,
        total_raw_size,
        processed_size,
        savings_pct,
        &format!("{:?}", algo),
        duration_ms,
        &vault_path.to_string_lossy()
    ).await;

    info!("Arquivado: {} -> {} ({:.1}% economia)", file_name, syn_filename, savings_pct);

    if let Err(e) = fs::remove_file(&path).await {
        warn!("Falha ao deletar original: {}", e);
    }

    Ok(())
}
