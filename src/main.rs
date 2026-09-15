#![recursion_limit = "1024"]

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::sync::Arc;
use std::time::Duration;

use sysinfo::{CpuRefreshKind, RefreshKind, System};
use tracing::{info, warn};

pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/engine.rs"));
}

#[macro_use]
mod sql;

mod api;
mod codec;
mod config;
mod container;
mod content;
mod db_monitor;
mod dict_store;
mod metrics;
mod planner;
mod service;
mod tenancy;
mod transform;
mod usage;
mod vault;
mod watcher;

use config::Config;
use service::EngineState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "engine=info".into()),
        )
        .init();

    let state = Arc::new(EngineState::new(config).await?);

    reconcile_vault_index(&state).await;

    if state.config.watch_enabled {
        watcher::start(state.clone());
    } else {
        info!("ingestão por diretório desabilitada (SYNTRA_WATCH_ENABLED=false)");
    }

    spawn_load_balancer(state.clone());

    let addr = state.config.bind_addr.clone();
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| anyhow::anyhow!("falha ao ligar em {addr}: {e}"))?;

    print_banner(&state);

    axum::serve(listener, api::router(state.clone()))
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    if let Err(e) = state.vault.flush() {
        warn!("falha ao sincronizar o índice do vault no encerramento: {e}");
    }
    info!("motor encerrado");
    Ok(())
}

async fn reconcile_vault_index(state: &Arc<EngineState>) {
    use prost::Message;
    use walkdir::WalkDir;

    let vault = state.vault.clone();
    let base = state.config.vault_path.clone();

    let (encontrados, indexados, problemas) = tokio::task::spawn_blocking(move || {
        let mut encontrados = 0usize;
        let mut indexados = 0usize;
        let mut problemas = 0usize;

        for entry in WalkDir::new(&base).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if !path.is_file()
                || path.extension().and_then(|e| e.to_str()) != Some(config::ESSENCE_EXTENSION)
            {
                continue;
            }
            encontrados += 1;

            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                problemas += 1;
                continue;
            };
            let Ok(hash) = vault::VaultManager::parse_id(stem) else {
                problemas += 1;
                continue;
            };

            if vault.get_ref(&hash).ok().flatten().is_some() {
                continue;
            }

            let Ok(bytes) = std::fs::read(path) else {
                problemas += 1;
                continue;
            };
            let Ok(envelope) = crate::proto::ProcessedFile::decode(&*bytes) else {
                problemas += 1;
                continue;
            };

            let now = container::now_millis();
            if vault
                .record(
                    &hash,
                    &envelope.original_name,
                    envelope.original_size,
                    envelope.processed_size,
                    now,
                )
                .is_ok()
            {
                indexados += 1;
            } else {
                problemas += 1;
            }
        }

        let _ = vault.flush();
        (encontrados, indexados, problemas)
    })
    .await
    .unwrap_or((0, 0, 0));

    if encontrados > 0 || problemas > 0 {
        info!(
            essencias_no_vault = encontrados,
            reindexadas = indexados,
            ignoradas = problemas,
            "índice do vault reconciliado"
        );
    }
}

fn spawn_load_balancer(state: Arc<EngineState>) {
    tokio::spawn(async move {
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()),
        );
        let mut retained = Vec::new();
        let mut ticker = tokio::time::interval(Duration::from_secs(5));
        let mut ticks_until_snapshot = 12u8;

        sys.refresh_cpu_usage();
        ticker.tick().await;

        info!("load balancer adaptativo ativo (modulação por CPU)");

        loop {
            ticker.tick().await;
            sys.refresh_cpu_usage();
            let cpu = sys.global_cpu_usage();
            state.metrics.record_cpu_usage(cpu);

            let available = state.sem_compress.available_permits();
            if cpu > 85.0 && available > 1 {
                if let Ok(permit) = state.sem_compress.clone().try_acquire_owned() {
                    retained.push(permit);
                    info!(
                        cpu_pct = format!("{cpu:.1}"),
                        permits_retidos = retained.len(),
                        "CPU alta: reduzindo concorrência"
                    );
                }
            } else if cpu < 50.0 && !retained.is_empty() {
                retained.pop();
                info!(
                    cpu_pct = format!("{cpu:.1}"),
                    permits_retidos = retained.len(),
                    "CPU normalizada: devolvendo concorrência"
                );
            }

            ticks_until_snapshot = ticks_until_snapshot.saturating_sub(1);
            if ticks_until_snapshot == 0 {
                ticks_until_snapshot = 12;
                record_snapshot(&state, cpu).await;
            }
        }
    });
}

async fn record_snapshot(state: &Arc<EngineState>, cpu: f32) {
    use std::sync::atomic::Ordering;

    let ram = state
        .metrics
        .system_monitor
        .lock()
        .map(|mut s| {
            s.refresh_memory();
            s.used_memory()
        })
        .unwrap_or(0);

    let m = &state.metrics;
    if let Err(e) = state
        .db
        .record_snapshot(
            m.bytes_in.load(Ordering::Relaxed),
            m.bytes_out.load(Ordering::Relaxed),
            m.files_processed.load(Ordering::Relaxed),
            cpu as f64,
            ram,
        )
        .await
    {
        tracing::debug!("falha ao gravar snapshot de sistema: {e}");
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("falha ao instalar handler de Ctrl+C");
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => warn!("não foi possível observar SIGTERM: {e}"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("SIGINT recebido; encerrando"),
        _ = terminate => info!("SIGTERM recebido; encerrando"),
    }
}

fn print_banner(state: &Arc<EngineState>) {
    let c = &state.config;
    let codecs: Vec<&str> = codec::CATALOG.iter().map(|i| i.name).collect();

    info!("═══════════════════════════════════════════════════════════");
    info!(" Syntra Engine v{}  ·  container v{}", env!("CARGO_PKG_VERSION"), container::CONTAINER_VERSION);
    info!("═══════════════════════════════════════════════════════════");
    info!(" endereço .......... {}", c.bind_addr);
    info!(" vault ............. {}", c.vault_path);
    info!(" dicionários ....... {} ({})", c.dict_path, if c.dictionaries_enabled { "ativos" } else { "desativados" });
    info!(" ingestão .......... {} ({})", c.watch_dir, if c.watch_enabled { "ativa" } else { "inativa" });
    info!(" codecs ............ {}", codecs.join(", "));
    info!(" esforço padrão .... {}", c.default_effort.as_str());
    info!(" verifica gravação . {}", c.verify_on_write);
    info!(" corpo máximo ...... {} MB", c.max_body_size_mb);
    info!(" permits de CPU .... {}", state.max_compress_permits);
    info!(" autenticação ...... {}", if c.auth_enabled() { "API key exigida" } else { "ABERTA (defina SYNTRA_API_KEYS)" });
    info!("───────────────────────────────────────────────────────────");
    info!(" POST /api/v1/compress    gera essência");
    info!(" POST /api/v1/decompress  reconstrói original");
    info!(" POST /api/v1/analyze     compara planos sem gravar");
    info!(" GET  /api/v1/objects     essências arquivadas");
    info!(" GET  /api/v1/codecs      capacidades do motor");
    info!(" GET  /api/v1/openapi.json  contrato");
    info!(" GET  /metrics            Prometheus");
    info!("═══════════════════════════════════════════════════════════");
}
