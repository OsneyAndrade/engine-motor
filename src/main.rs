#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// Importações das bibliotecas externas
use axum::{
    extract::DefaultBodyLimit,  // Middleware para limitar tamanho de uploads
    routing::{get, post, get_service, delete},  // Métodos de roteamento HTTP
    Router,  // Estrutura principal do servidor Axum
};
use std::sync::Arc;  // Arc permite compartilhar dados entre threads de forma segura
use tokio::sync::Semaphore;  // Semáforo para controle de concorrência
use tower_http::cors::CorsLayer;  // Middleware para permitir requisições de outras origens
use tower_http::services::{ServeFile, ServeDir};  // Serving de arquivos estáticos
use tracing::info;  // Macro para logging de informações
use std::time::Duration;  // Tipo para representar durações de tempo
use sysinfo::{System, CpuRefreshKind, RefreshKind};  // Monitoramento de recursos do sistema


pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/engine.rs"));
}

// Declaração dos módulos internos do projeto
// Cada arquivo .rs é um módulo separado
mod adaptive;    // Análise adaptativa de dados
mod compress;    // Algoritmos de compressão
mod config;      // Configuração do sistema
mod dictionary;  // Gerenciamento de dicionários Zstd
mod handlers;    // Manipuladores de requisições HTTP
mod metrics;     // Métricas e monitoramento
mod watcher;     // Monitoramento de diretórios
mod vault;       // Gerenciamento de armazenamento
mod db_monitor;  // Monitoramento de banco de dados

// Importações para uso conveniente no código
use vault::VaultManager;      // Gerencia o armazenamento de arquivos processados
use db_monitor::MonitorDB;    // Gerencia a conexão com o banco de dados
use config::Config;           // Estrutura de configuração


pub struct EngineState {

    pub sem_compress: Arc<Semaphore>,
    pub sem_io: Arc<Semaphore>,
    pub dictionaries: dictionary::DictionaryManager,
    pub metrics: metrics::EngineMetrics,
    pub pool: rayon::ThreadPool,
    pub vault: VaultManager,
    pub db: MonitorDB,
    pub max_compress_permits: usize,
}

impl EngineState {

    pub async fn new(config: &Config) -> Self {

        let rayon_threads = std::env::var("RAYON_NUM_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(num_cpus::get);

        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(rayon_threads)  // Usa RAYON_NUM_THREADS ou todas as CPUs
            .thread_name(|i| format!("syntra-worker-{}", i))  // Nomeia cada thread
            .build()
            .expect("Falha ao criar ThreadPool do Rayon");

        if let Err(e) = std::fs::create_dir_all(&config.vault_path) {
            panic!("Falha ao criar diretório vault {}: {}", config.vault_path, e);
        }
        if let Err(e) = std::fs::create_dir_all(&config.watch_dir) {
            panic!("Falha ao criar diretório watch {}: {}", config.watch_dir, e);
        }

        let vault = VaultManager::new(&config.vault_path, &config.sled_index_path)
            .expect("Falha ao abrir vault/index");

        let db = MonitorDB::new(&config.db_url)
            .await
            .expect("Falha ao criar banco de monitoramento");

        let cpus = num_cpus::get();

        Self {
            sem_compress: Arc::new(Semaphore::new(cpus)),
            sem_io: Arc::new(Semaphore::new(cpus * config.compress_multiplier)),
            dictionaries: dictionary::DictionaryManager::new(None),
            metrics: metrics::EngineMetrics::new(),
            pool,
            vault,
            db,
            max_compress_permits: cpus,
        }
    }
}

async fn rebuild_vault_index(state: &Arc<EngineState>, vault_path: &str) {
    use std::fs;
    use crate::proto::ProcessedFile;
    use prost::Message;
    use walkdir::WalkDir;
    let walker = WalkDir::new(vault_path).into_iter().filter_map(|e| e.ok());
    let mut file_count = 0;

    for entry in walker {
        let path = entry.path();

        if path.extension().map(|e| e != "syntra").unwrap_or(false) {
            continue;
        }

        let Ok(data) = fs::read(path) else {
            continue;
        };

        let Ok(envelope) = ProcessedFile::decode(&*data) else {
            continue;
        };

        use std::sync::atomic::Ordering;

        state.metrics.files_processed.fetch_add(1, Ordering::Relaxed);
        state.metrics.bytes_in.fetch_add(envelope.original_size, Ordering::Relaxed);

        file_count += 1;

        let mut hash_arr = [0u8; 32];
        hash_arr.copy_from_slice(&envelope.file_hash);

        let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let _ = state.vault.store_hash(&hash_arr, &filename);

        let essence_size: u64 = envelope.data.len() as u64;
        state.metrics.bytes_out.fetch_add(essence_size, Ordering::Relaxed);
    }

    info!("Vault Index Rebuilt: {} files found.", file_count);
}


#[tokio::main]
async fn main() {
    let config = Config::from_env();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "engine=info".into()),
        )
        .init();

    // Exibe banner de inicialização com informações de configuração
    info!("╔══════════════════════════════════════════════════╗");
    info!("║        SYNTRA ENGINE v2.1 — Datacenter Edition   ║");
    info!("╠══════════════════════════════════════════════════╣");
    info!("║  Configuração                                   ║");
    info!("║  Vault: {}                           ║", config.vault_path);
    info!("║  Watch Dir: {}                         ║", config.watch_dir);
    info!("║  Bind: {}                                ║", config.bind_addr);
    info!("║  Max Body: {} MB                           ║", config.max_body_size_mb);
    info!("╚══════════════════════════════════════════════════╝");

    let state = Arc::new(EngineState::new(&config).await);
    let cores = state.max_compress_permits;  // Número de núcleos disponíveis
    let io_limit = cores * config.compress_multiplier;  // Limite de I/O

    rebuild_vault_index(&state, &config.vault_path).await;

    watcher::start_directory_watcher(state.clone(), &config.watch_dir);

    let lb_state = state.clone();

    tokio::spawn(async move {
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
        );

        let mut throttle_permits = Vec::new();

        // Timer que dispara a cada 5 segundos
        let mut lb_interval = tokio::time::interval(Duration::from_secs(5));

        info!("Iniciando Adaptive Load Balancer (CPU-bound modulation)");

        loop {
            // Aguarda o próximo tick do timer
            lb_interval.tick().await;

            sys.refresh_cpu_usage();

            let global_cpu = sys.global_cpu_usage();

            let current_available = lb_state.sem_compress.available_permits();

            if global_cpu > 85.0 && current_available > 1 {
                if let Ok(permit) = lb_state.sem_compress.clone().try_acquire_owned() {
                    throttle_permits.push(permit);
                    info!(
                        "Load Balancer: CPU Alta ({:.1}%). Throttling... (Permits Retained: {})",
                        global_cpu,
                        throttle_permits.len()
                    );
                }
            } else if global_cpu < 50.0 && !throttle_permits.is_empty() {
                throttle_permits.pop();
                info!(
                    "Load Balancer: CPU Estável ({:.1}%). Scaling Up... (Permits Retained: {})",
                    global_cpu,
                    throttle_permits.len()
                );
            }

            // Atualiza métricas de CPU para o dashboard
            lb_state.metrics.record_cpu_usage(global_cpu);
        }
    });

    let app = Router::new()
        // Serve arquivos estáticos do diretório "static" (dashboard HTML, CSS, JS)
        .nest_service("/static", get_service(ServeDir::new("static")))
        .route("/", get_service(ServeFile::new("static/dashboard.html")))
        .route("/dashboard", get_service(ServeFile::new("static/dashboard.html")))
        .route("/process", post(handlers::process_handler))
        .route("/reconstruct", post(handlers::reconstruct_handler))
        .route("/stats", get(handlers::stats_handler))
        .route("/api/v1/stats", get(handlers::stats_handler))
        .route("/metrics", get(handlers::metrics_handler))
        .route("/health", get(handlers::health_handler))
        .route("/api/files", get(handlers::list_files_handler))
        .route("/api/files/:id", get(handlers::download_file_by_id_handler))
        .route("/api/files/:id", delete(handlers::delete_file_by_id_handler))
        .route("/api/sim/process", post(handlers::sim_process_handler))
        .route("/api/sim/download/:id", get(handlers::sim_download_handler))
        .route("/compress", post(handlers::process_handler))
        .route("/decompress", post(handlers::reconstruct_handler))
        .layer(DefaultBodyLimit::max(config.max_body_size_mb * 1024 * 1024))
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    // Obtém o endereço de bind da configuração
    let addr = &config.bind_addr;

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("porta ocupada");

    // Exibe banner final com informações de conexão
    info!("╔══════════════════════════════════════════════════╗");
    info!("║        SYNTRA ENGINE v2.1 — Datacenter Edition   ║");
    info!("╠══════════════════════════════════════════════════╣");
    info!("║  Adaptive Compression • Inline Dedup • Auto-Dict ║");
    info!("║  Cores: {:<3}  |  Porta: {:<23} ║", cores, addr);
    info!("║  Compress Limit: {:<3} | I/O Limit: {:<14} ║", cores, io_limit);
    info!("╠══════════════════════════════════════════════════╣");
    info!("║  POST /process      → Processamento inteligente  ║");
    info!("║  POST /reconstruct  → Reconstrução de dados       ║");
    info!("║  GET  /stats        → Dashboard JSON (ROI)        ║");
    info!("║  GET  /metrics      → Prometheus                  ║");
    info!("║  GET  /health       → Liveness probe              ║");
    info!("╚══════════════════════════════════════════════════╝");

    axum::serve(listener, app).await.expect("falha no servidor");
}
