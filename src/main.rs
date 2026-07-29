//! # Syntra Engine V2 — Motor Inteligente de Destilação de Dados
//! (versão otimizada para build rápido)
//!
//! Este é o arquivo principal do motor Syntra, responsável por inicializar
//! e configurar todos os componentes do sistema de destilação de dados.
//!
//! ## O que é Destilação de Dados?
//!
//! Destilação de dados é o processo de reduzir o tamanho dos arquivos através de
//! técnicas avançadas de compressão, deduplicação e análise adaptativa. Diferente
//! da compressão tradicional, a destilação analisa o tipo de conteúdo e escolhe
//! a melhor estratégia automaticamente.
//!
//! ## Diferenciais do Syntra Engine:
//!
//! - **Geração de Essência Adaptativa**: Analisa os dados e escolhe o melhor algoritmo
//! - **Reconstrução de Alta Fidelidade**: Garante perfeita reconstrução dos dados originais
//! - **Deduplicação de blocos em tempo real**: Identifica e elimina dados duplicados
//! - **Treinamento automático de modelos**: Cria dicionários otimizados por tipo de arquivo
//! - **Health checks**: Monitoramento constante para ambientes de datacenter
//!
//! ## Otimizações v2.1:
//!
//! - **mimalloc**: Alocador de memória de alta performance para datacenter
//! - **Two-level semaphore**: Controle separado para processamento CPU e I/O
//! - **Modelo de contexto cache**: Cache otimizado em tempo de compilação (PHF)
//! - **ShardedDedupStore**: Armazenamento distribuído com evicção LRU
//! - **Zero-copy pipeline**: Elimina redundâncias no processamento de hash
//!
//! # Estrutura do Código
//!
//! ## Linhas 1-2: Configuração do Alocador de Memória
//! ```rust
//! #[global_allocator]
//! static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
//! ```
//! - Define o alocador de memória global como mimalloc
//! - mimalloc é mais rápido que o alocador padrão do Rust para grandes workloads
//! - `#[global_allocator]` é um atributo especial que configura o alocador global
//!
//! ## Importações (linhas 20-31)
//! - `axum`: Framework web para criar o servidor HTTP
//! - `tokio`: Runtime assíncrono do Rust (similar ao async/await do JavaScript)
//! - `tower_http`: Middleware para CORS e serving de arquivos estáticos
//! - `tracing`: Biblioteca de logging estruturado
//! - `sysinfo`: Biblioteca para monitorar CPU e memória do sistema

/// Configura o alocador de memória global para usar mimalloc.
///
/// # O que é um alocador de memória?
/// Um alocador de memória é responsável por reservar e liberar memória RAM.
/// O mimalloc é mais eficiente que o alocador padrão porque:
/// - Usa algoritmos otimizados para multi-threading
/// - Reduz a fragmentação de memória
/// - É mais rápido em cenários de alta concorrência
///
/// # O que significa `static`?
/// `static` significa que esta variável existe durante toda a vida do programa
/// e é compartilhada entre todas as threads.
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

/// Módulo proto: Contém as definições geradas pelo Protocol Buffers.
///
/// # O que é Protocol Buffers?
/// Protocol Buffers (protobuf) é um formato de serialização de dados binário
/// desenvolvido pelo Google. É mais compacto e rápido que JSON.
///
/// # O que faz essa linha?
/// - `env!("OUT_DIR")` pega o diretório de saída da compilação (definido pelo Cargo)
/// - `concat!` junta strings em tempo de compilação
/// - `include!` inclui o código Rust gerado a partir do arquivo .proto
///
/// O arquivo engine.proto define a estrutura dos dados que serão serializados.
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

/// Estado compartilhado do motor com dois níveis de semáforo.
///
/// # O que é um estado compartilhado?
/// É uma estrutura que contém todos os dados que precisam ser acessados
/// por diferentes partes do programa simultaneamente.
///
/// # O que é `Arc`?
/// `Arc` significa "Atomic Reference Counting". É um ponteiro inteligente
/// que permite compartilhar dados entre threads de forma segura. O Arc
/// conta quantas vezes o dado está sendo usado e só libera a memória
/// quando ninguém mais o está usando.
///
/// # O que é um semáforo?
/// Um semáforo é um mecanismo de controle de concorrência que limita
/// quantas threads podem executar uma operação simultaneamente.
///
/// # Two-level semaphore
/// Esta estrutura usa dois semáforos diferentes:
/// - `sem_compress`: Limita operações que consomem muita CPU (compressão)
/// - `sem_io`: Limita operações de entrada/saída (leitura/escrita de disco)
///
/// Isso permite mais operações I/O do que CPU, melhorando o throughput.
pub struct EngineState {
    /// Semáforo para controlar operações de compressão (CPU-bound)
    /// CPU-bound significa que a operação consome muitos recursos da CPU.
    /// O limite é baseado no número de núcleos do processador.
    pub sem_compress: Arc<Semaphore>,

    /// Semáforo para controlar operações de I/O (disco, rede)
    /// I/O-bound significa que a operação espera por entrada/saída de dados.
    /// O limite é maior que o de compressão (geralmente 4x) pois I/O não
    /// consome tanta CPU quanto compressão.
    pub sem_io: Arc<Semaphore>,

    /// Gerenciador de dicionários de compressão
    /// Dicionários são amostras de dados usadas para treinar o compressor
    /// Zstd, tornando-o mais eficiente para certos tipos de arquivos.
    pub dictionaries: dictionary::DictionaryManager,

    /// Coletor de métricas do sistema
    /// Registra estatísticas como bytes processados, taxa de compressão,
    /// latência, etc. para monitoramento e análise.
    pub metrics: metrics::EngineMetrics,

    /// Pool de threads do Rayon para processamento paralelo
    /// Rayon é uma biblioteca de paralelismo para Rust. O ThreadPool
    /// gerencia um conjunto de threads que podem executar tarefas em paralelo.
    pub pool: rayon::ThreadPool,

    /// Gerenciador do cofre de arquivos processados (Vault)
    /// O Vault é onde os arquivos comprimidos (essências) são armazenados.
    pub vault: VaultManager,

    /// Monitor de banco de dados para auditoria
    /// Registra informações sobre arquivos processados em um banco de dados
    /// para consulta posterior e auditoria.
    pub db: MonitorDB,

    /// Capacidade máxima configurada para o semáforo de compressão

    /// Capacidade máxima configurada para o semáforo de compressão
    /// Guarda o número máximo de permissões (permits) disponíveis para
    /// operações de compressão simultâneas.
    pub max_compress_permits: usize,
}

impl EngineState {
    /// Cria uma nova instância do estado compartilhado do motor.
    ///
    /// # O que é um `impl`?
    /// `impl` é a palavra-chave usada em Rust para implementar métodos
    /// em uma struct (similar a métodos de classe em outras linguagens).
    ///
    /// # O que é `async`?
    /// `async` indica que esta função pode ter operações que levam tempo
    /// para completar (como ler arquivos ou conectar ao banco) e não deve
    /// bloquear o programa enquanto espera.
    ///
    /// # Parâmetros
    /// * `config` - Referência para a configuração do sistema
    ///
    /// # Retorna
    /// Uma nova instância de EngineState pronta para uso
    pub async fn new(config: &Config) -> Self {
        // Cria o pool de threads do Rayon para processamento paralelo
        //
        // O que é Rayon? É uma biblioteca de paralelismo de dados para Rust.
        // O ThreadPool gerencia várias threads que podem executar tarefas
        // simultaneamente em diferentes núcleos da CPU.
        //
        // num_cpus::get() retorna o número de núcleos lógicos do processador
        // RAYON_NUM_THREADS permite limitar o número de threads via variável de ambiente
        let rayon_threads = std::env::var("RAYON_NUM_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(num_cpus::get);

        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(rayon_threads)  // Usa RAYON_NUM_THREADS ou todas as CPUs
            .thread_name(|i| format!("syntra-worker-{}", i))  // Nomeia cada thread
            .build()
            .expect("Falha ao criar ThreadPool do Rayon");

        // Garante que diretórios de trabalho existam (para Docker volumes)
        //
        // std::fs::create_dir_all cria um diretório e todos os diretórios pai
        // necessários. Se já existirem, não faz nada.
        if let Err(e) = std::fs::create_dir_all(&config.vault_path) {
            panic!("Falha ao criar diretório vault {}: {}", config.vault_path, e);
        }
        if let Err(e) = std::fs::create_dir_all(&config.watch_dir) {
            panic!("Falha ao criar diretório watch {}: {}", config.watch_dir, e);
        }

        // Inicializa o gerenciador do cofre de arquivos (Vault)
        //
        // O Vault é onde os arquivos processados (essências) são armazenados.
        // Usa Sled (banco de dados embutido) para indexação rápida.
        let vault = VaultManager::new(&config.vault_path, &config.sled_index_path)
            .expect("Falha ao abrir vault/index");

        // Inicializa o monitor de banco de dados
        // Conecta ao banco de dados configurado (SQLite ou PostgreSQL)
        // para registrar informações sobre arquivos processados.
        let db = MonitorDB::new(&config.db_url)
            .await
            .expect("Falha ao criar banco de monitoramento");

        // Obtém o número de CPUs para configurar os semáforos
        let cpus = num_cpus::get();

        // Retorna uma nova instância de EngineState com todos os componentes inicializados
        Self {
            // Cria o semáforo de compressão com limite igual ao número de CPUs
            // Isso garante que nunca teremos mais operações de compressão
            // do que núcleos disponíveis, evitando sobrecarga.
            sem_compress: Arc::new(Semaphore::new(cpus)),

            // Cria o semáforo de I/O com limite maior (CPUs * multiplicador)
            // Operações de I/O não consomem muita CPU, então podemos ter
            // mais delas simultaneamente. O multiplicador padrão é 4.
            sem_io: Arc::new(Semaphore::new(cpus * config.compress_multiplier)),

            // Inicializa o gerenciador de dicionários sem conexão Redis (None)
            // Dicionários são amostras de dados usadas para treinar o
            // compressor Zstd, tornando-o mais eficiente.
            dictionaries: dictionary::DictionaryManager::new(None),

            // Inicializa o coletor de métricas vazio
            metrics: metrics::EngineMetrics::new(),

            // Usa o pool de threads criado anteriormente
            pool,

            // Usa o vault e DB inicializados
            vault,
            db,

            // Armazena a capacidade máxima do semáforo de compressão
            max_compress_permits: cpus,
        }
    }
}

/// Reconstrói o índice do vault a partir dos arquivos existentes.
///
/// # O que é o índice do vault?
/// É um mapa que permite encontrar rapidamente arquivos pelo seu hash.
/// Quando o servidor é reiniciado, o índice precisa ser reconstruído.
///
/// # Parâmetros
/// * `state` - Referência para o estado compartilhado do motor
/// * `vault_path` - Caminho para o diretório onde os arquivos são armazenados
///
/// # O que essa função faz?
/// 1. Percorre recursivamente o diretório do vault
/// 2. Lê todos os arquivos .syntra
/// 3. Decodifica cada arquivo para extrair metadados
/// 4. Atualiza as métricas
async fn rebuild_vault_index(state: &Arc<EngineState>, vault_path: &str) {
    use std::fs;
    use crate::proto::ProcessedFile;
    use prost::Message;
    use walkdir::WalkDir;

    // Cria um "walker" que percorre recursivamente o diretório
    let walker = WalkDir::new(vault_path).into_iter().filter_map(|e| e.ok());

    let mut file_count = 0;

    // Itera sobre cada entrada no diretório
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

/// Função principal do programa.
///
/// # O que é `#[tokio::main]`?
/// É um atributo que transforma a função `main` em uma função `async`
/// e cria um runtime Tokio para executá-la. Tokio é o runtime assíncrono
/// mais usado no Rust, similar ao Node.js para JavaScript.
///
/// # Fluxo de execução:
/// 1. Carrega configuração das variáveis de ambiente
/// 2. Inicializa o sistema de logging
/// 3. Cria o estado compartilhado do motor
/// 4. Reconstrói o índice do vault
/// 5. Inicia o monitoramento de diretórios
/// 6. Inicia o balanceador de carga adaptativo
/// 7. Configura e inicia o servidor HTTP
#[tokio::main]
async fn main() {
    // Carrega configuração das variáveis de ambiente
    // Variáveis de ambiente são configurações definidas no sistema operacional.
    // Isso permite configurar o aplicativo sem modificar o código.
    let config = Config::from_env();

    // Inicializa o sistema de logging (tracing)
    // tracing_subscriber é uma biblioteca de logging estruturado.
    // with_env_filter configura qual nível de log mostrar (info, debug, error, etc.).
    // try_from_default_env() tenta ler a variável RUST_LOG.
    // unwrap_or_else() usa "engine=info" se a variável não estiver definida.
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

    // Cria o estado compartilhado do motor
    // Arc::new() cria uma referência atômica que pode ser compartilhada
    // entre múltiplas threads de forma segura. O estado será usado por
    // todos os handlers HTTP e tarefas em background.
    let state = Arc::new(EngineState::new(&config).await);
    let cores = state.max_compress_permits;  // Número de núcleos disponíveis
    let io_limit = cores * config.compress_multiplier;  // Limite de I/O

    // NOTA: Métricas são obtidas SOMENTE do banco de dados em tempo real.
    // Não carregamos métricas na memória ao iniciar.

    // Reconstrói o índice global de deduplicação a partir dos arquivos do vault
    // Isso é necessário para que o sistema saiba quais arquivos já existem
    // e possa fazer deduplicação com arquivos processados anteriormente.
    rebuild_vault_index(&state, &config.vault_path).await;

    // Inicia o monitoramento de diretórios para ingestão automática
    // O watcher monitora o diretório configurado e processa automaticamente
    // qualquer arquivo colocado lá. Isso é útil para automação.
    watcher::start_directory_watcher(state.clone(), &config.watch_dir);

    // =========================================================================
    // ADAPTIVE LOAD BALANCER (Controle de Carga Adaptativo)
    // =========================================================================
    // O load balancer monitora o uso da CPU e ajusta dinamicamente quantas
    // operações de compressão podem ocorrer simultaneamente.
    // Isso previne que o sistema fique sobrecarregado quando muitas requisições
    // chegam simultaneamente.

    // Clona o estado para usar na thread do load balancer
    let lb_state = state.clone();

    // Inicia uma nova tarefa em background (tokio::spawn)
    // tokio::spawn cria uma tarefa assíncrona que roda em paralelo.
    // async move move o ownership de lb_state para dentro da tarefa.
    tokio::spawn(async move {
        // Cria um monitor de sistema focado apenas em CPU
        // System::new_with_specifics cria um monitor com configurações específicas.
        // RefreshKind::nothing() não atualiza nada por padrão.
        // .with_cpu(CpuRefreshKind::everything()) ativa monitoramento completo de CPU.
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
        );

        // Vetor para armazenar permissões retidas durante throttling
        let mut throttle_permits = Vec::new();

        // Timer que dispara a cada 5 segundos
        let mut lb_interval = tokio::time::interval(Duration::from_secs(5));

        info!("Iniciando Adaptive Load Balancer (CPU-bound modulation)");

        // Loop infinito do balanceador de carga
        loop {
            // Aguarda o próximo tick do timer
            lb_interval.tick().await;

            // Atualiza os dados de uso da CPU
            sys.refresh_cpu_usage();

            // Obtém o uso global da CPU (0.0 a 100.0)
            let global_cpu = sys.global_cpu_usage();

            // Obtém quantas permissões estão disponíveis no semáforo
            let current_available = lb_state.sem_compress.available_permits();

            // LÓGICA DE MODULAÇÃO:
            // Se CPU > 85%: Throttling (reduz número de operações simultâneas)
            // Se CPU < 50%: Scaling (aumenta número de operações simultâneas)

            if global_cpu > 85.0 && current_available > 1 {
                // CPU alta: retém uma permissão para reduzir carga
                // try_acquire_owned() tenta adquirir uma permissão.
                // Se conseguir, a permissão é armazenada e não devolvida,
                // reduzindo o número de operações possíveis.
                if let Ok(permit) = lb_state.sem_compress.clone().try_acquire_owned() {
                    throttle_permits.push(permit);
                    info!(
                        "Load Balancer: CPU Alta ({:.1}%). Throttling... (Permits Retained: {})",
                        global_cpu,
                        throttle_permits.len()
                    );
                }
            } else if global_cpu < 50.0 && !throttle_permits.is_empty() {
                // CPU estável: libera uma permissão retida
                // pop() remove a última permissão do vetor e a devolve
                // automaticamente (porque OwnedSemaphoreGuard faz drop)
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

    // Configura o servidor HTTP com todas as rotas
    // Router é a estrutura que define todas as rotas HTTP do servidor.
    // Cada route() adiciona um novo endpoint.
    let app = Router::new()
        // Serve arquivos estáticos do diretório "static" (dashboard HTML, CSS, JS)
        .nest_service("/static", get_service(ServeDir::new("static")))

        // Página do dashboard (Tier 3 - funcionalidade premium)
        .route("/", get_service(ServeFile::new("static/dashboard.html")))
        .route("/dashboard", get_service(ServeFile::new("static/dashboard.html")))

        // === ENDPOINTS DE PROCESSAMENTO ===
        // POST /process - Processa um arquivo e gera a essência comprimida
        // Este endpoint recebe um arquivo, analisa seu conteúdo, escolhe o
        // melhor algoritmo de compressão e retorna a versão comprimida.
        .route("/process", post(handlers::process_handler))

        // POST /reconstruct - Reconstrói o arquivo original a partir da essência
        // Este endpoint recebe uma essência comprimida e retorna o arquivo
        // original exatamente como era (lossless reconstruction).
        .route("/reconstruct", post(handlers::reconstruct_handler))

        // === ENDPOINTS DE AUDITORIA E ROI ===
        // GET /stats - Retorna estatísticas em formato JSON
        // Fornece métricas sobre taxas de compressão, economia de espaço,
        // número de arquivos processados, etc.
        .route("/stats", get(handlers::stats_handler))

        // GET /api/v1/stats - Versão alternativa do endpoint de estatísticas
        .route("/api/v1/stats", get(handlers::stats_handler))

        // GET /metrics - Retorna métricas em formato Prometheus
        // Prometheus é um sistema de monitoramento. Este endpoint retorna
        // métricas em formato de texto que o Prometheus pode coletar.
        .route("/metrics", get(handlers::metrics_handler))

        // GET /health - Health check para orquestradores (Kubernetes, etc)
        // Retorna se o servidor está funcionando. Usado por sistemas de
        // orquestração para saber se o container está saudável.
        .route("/health", get(handlers::health_handler))

        // === GERENCIAMENTO DE ARQUIVOS ===
        // GET /api/files - Lista todos os arquivos processados
        .route("/api/files", get(handlers::list_files_handler))

        // GET /api/files/:id - Baixa um arquivo processado por ID (hash BLAKE3)
        .route("/api/files/:id", get(handlers::download_file_by_id_handler))

        // DELETE /api/files/:id - Deleta um arquivo processado por ID (hash BLAKE3)
        .route("/api/files/:id", delete(handlers::delete_file_by_id_handler))

        // === API DE SIMULAÇÃO ===
        // POST /api/sim/process - Processa e retorna JSON simplificado
        // Similar ao /process mas retorna um JSON amigável para demonstrações.
        .route("/api/sim/process", post(handlers::sim_process_handler))

        // GET /api/sim/download/:id - Download de arquivo processado (simulação)
        .route("/api/sim/download/:id", get(handlers::sim_download_handler))

        // === COMPATIBILIDADE COM V1 ===
        // POST /compress - Nome alternativo para /process (compatibilidade)
        .route("/compress", post(handlers::process_handler))

        // POST /decompress - Nome alternativo para /reconstruct (compatibilidade)
        .route("/decompress", post(handlers::reconstruct_handler))

        // === CONFIGURAÇÕES DO SERVIDOR ===

        // Limita o tamanho do corpo das requisições (previne ataques de esgotamento de memória)
        .layer(DefaultBodyLimit::max(config.max_body_size_mb * 1024 * 1024))

        // Habilita CORS (Cross-Origin Resource Sharing)
        // CORS permite que navegadores acessem o servidor de outros domínios.
        // permissive() permite requisições de qualquer origem.
        .layer(CorsLayer::permissive())

        // Injeta o estado compartilhado em todos os handlers
        .with_state(state.clone());

    // Obtém o endereço de bind da configuração
    let addr = &config.bind_addr;

    // Cria o listener TCP (socket que aceita conexões)
    // Tokio's TcpListener é assíncrono e não-bloqueante.
    // bind() reserva a porta especificada.
    // expect() encerra o programa se a porta estiver ocupada.
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

    // Inicia o servidor HTTP
    // axum::serve() inicia o servidor que processa requisições HTTP.
    // O servidor roda indefinidamente até ser interrompido (Ctrl+C).
    // expect() encerra o programa se houver erro ao iniciar o servidor.
    axum::serve(listener, app).await.expect("falha no servidor");
}
