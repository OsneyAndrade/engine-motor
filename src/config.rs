//! # Módulo de Configuração
//!
//! Este módulo gerencia todas as configurações do motor Syntra. As configurações
//! são lidas de variáveis de ambiente, o que permite configurar o aplicativo
//! sem modificar o código fonte.
//!
//! # O que são Variáveis de Ambiente?
//!
//! Variáveis de ambiente são valores definidos no sistema operacional que
//! podem ser lidos pelos programas. São muito usadas para configuração
//! porque:
//! - Permitem configurar sem modificar o código
//! - São fáceis de usar em containers Docker
//! - Podem ser diferentes em cada ambiente (dev, prod, etc)
//!
//! # Como usar
//!
//! No Linux/Mac:
//! ```bash
//! export SYNTRA_VAULT_PATH=/data/vault
//! export SYNTRA_BIND_ADDR=0.0.0.0:3002
//! ./engine
//! ```
//!
//! No Windows (PowerShell):
//! ```powershell
//! $env:SYNTRA_VAULT_PATH="C:\data\vault"
//! $env:SYNTRA_BIND_ADDR="0.0.0.0:3002"
//! ./engine.exe
//! ```

/// Extensão de arquivo usada para arquivos processados (essências).
///
/// # O que é uma essência?
///
/// Uma essência é o resultado do processo de destilação (compressão + deduplicação).
/// Os arquivos .syntra contêm os dados comprimidos mais metadados necessários
/// para reconstruir o arquivo original.
///
/// # Por que .syntra?
///
/// - É único e não conflita com outras extensões
/// - Fácil de reconhecer como arquivo do sistema Syntra
/// - Curto e fácil de digitar
pub const ESSENCE_EXTENSION: &str = "syntra";

/// Estrutura de configuração do motor.
///
/// Esta estrutura contém todas as configurações necessárias para o funcionamento
/// do motor Syntra. Os valores são lidos das variáveis de ambiente na inicialização.
///
/// # Campos da Estrutura
///
/// Cada campo representa uma configuração diferente:
/// - Caminhos de diretórios (vault, watch)
/// - Configurações de rede (bind_addr)
/// - Configurações de banco de dados
/// - Limites e multiplicadores
pub struct Config {
    /// Caminho do diretório onde as essências são armazenadas.
    ///
    /// O "vault" (cofre) é onde os arquivos processados são salvos.
    /// Pode ser um caminho relativo (ex: "essence_vault") ou absoluto (ex: "/data/vault").
    /// O padrão é "essence_vault" no diretório atual.
    pub vault_path: String,

    /// Caminho do arquivo de índice do Sled (banco de dados embutido).
    ///
    /// Sled é um banco de dados chave-valor embutido usado para indexação rápida.
    /// O índice permite encontrar arquivos rapidamente pelo hash.
    /// O padrão é "metadata_index.sled" no diretório atual.
    pub sled_index_path: String,

    /// URL de conexão do banco de dados de monitoramento.
    ///
    /// Formatos suportados:
    /// - SQLite: sqlite://caminho/arquivo.db
    /// - PostgreSQL: postgres://usuario:senha@host:porta/banco
    ///
    /// O banco de monitoramento registra informações sobre arquivos processados
    /// para auditoria e análise posterior.
    pub db_url: String,

    /// Caminho do diretório monitorado para ingestão automática.
    ///
    /// Qualquer arquivo colocado neste diretório será automaticamente processado.
    /// O sistema monitora o diretório e processa novos arquivos em background.
    /// O padrão é "watch_in" no diretório atual.
    pub watch_dir: String,

    /// Endereço e porta onde o servidor HTTP vai escutar.
    ///
    /// Formato: "IP:PORTA"
    /// - "0.0.0.0:3002" escuta em todas as interfaces de rede
    /// - "127.0.0.1:3002" escuta apenas localmente
    /// - ":3002" é abreviação de "0.0.0.0:3002"
    pub bind_addr: String,

    /// Tamanho máximo do corpo das requisições HTTP em megabytes.
    ///
    /// Este limite previne ataques de esgotamento de memória.
    /// Se alguém tentar enviar um arquivo maior que este limite,
    /// a requisição será rejeitada.
    /// O padrão é 4096 MB (4 GB).
    pub max_body_size_mb: usize,

    /// Multiplicador para o limite de operações I/O em relação ao limite de CPU.
    pub compress_multiplier: usize,
}

impl Config {
    /// Cria uma nova configuração lendo as variáveis de ambiente.
    ///
    /// # O que é impl?
    ///
    /// `impl` é a palavra-chave usada em Rust para implementar métodos
    /// em uma struct. `impl Config` significa que estamos implementando
    /// métodos associados à estrutura Config.
    ///
    /// # Valores Padrão
    ///
    /// Se uma variável de ambiente não estiver definida, o valor padrão é usado:
    ///
    /// | Variável | Padrão | Descrição |
    /// |----------|-------|-----------|
    /// | SYNTRA_VAULT_PATH | essence_vault | Diretório de armazenamento |
    /// | SYNTRA_SLED_INDEX | metadata_index.sled | Índice do Sled |
    /// | SYNTRA_DB_URL | postgres://... | Banco de dados |
    /// | SYNTRA_WATCH_DIR | watch_in | Diretório monitorado |
    /// | SYNTRA_BIND_ADDR | 0.0.0.0:3002 | Endereço do servidor |
    /// | SYNTRA_MAX_BODY_MB | 4096 | Tamanho máximo de upload |
    /// | SYNTRA_COMPRESS_MULTIPLIER | 4 | Multiplicador de I/O |
    ///
    /// # Retorna
    ///
    /// Uma nova instância de Config com todos os valores lidos.
    pub fn from_env() -> Self {
        Self {
            // Lê SYNTRA_VAULT_PATH ou usa "essence_vault" como padrão
            //
            // std::env::var tenta ler a variável de ambiente.
            // unwrap_or_else é executado se a variável não existir.
            vault_path: std::env::var("SYNTRA_VAULT_PATH")
                .unwrap_or_else(|_| "essence_vault".to_string()),

            // Lê SYNTRA_SLED_INDEX ou usa "metadata_index.sled" como padrão
            sled_index_path: std::env::var("SYNTRA_SLED_INDEX")
                .unwrap_or_else(|_| "metadata_index.sled".to_string()),

            // Lê SYNTRA_DB_URL ou usa PostgreSQL padrão
            //
            // O padrão conecta a um PostgreSQL local chamado syntra_monitoring
            db_url: std::env::var("SYNTRA_DB_URL")
                .unwrap_or_else(|_| "postgres://syntra:syntra123@localhost:5432/syntra_monitoring".to_string()),

            // Lê SYNTRA_WATCH_DIR ou usa "watch_in" como padrão
            watch_dir: std::env::var("SYNTRA_WATCH_DIR")
                .unwrap_or_else(|_| "watch_in".to_string()),

            // Lê SYNTRA_BIND_ADDR ou usa "0.0.0.0:3002" como padrão
            bind_addr: std::env::var("SYNTRA_BIND_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:3002".to_string()),

            // Lê SYNTRA_MAX_BODY_MB ou usa 4096 (4GB) como padrão
            //
            // .ok() transforma Result em Option
            // .and_then() processa o Option se for Some
            // .parse() tenta converter String para o tipo especificado
            // .unwrap_or() fornece o valor padrão se falhar
            max_body_size_mb: std::env::var("SYNTRA_MAX_BODY_MB")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(4096), // 4GB padrão

            // Lê SYNTRA_COMPRESS_MULTIPLIER ou usa 4 como padrão
            compress_multiplier: std::env::var("SYNTRA_COMPRESS_MULTIPLIER")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(4), // 4x I/O vs CPU
        }
    }

    /// Retorna `true` se estiver usando PostgreSQL.
    ///
    /// # O que é str::starts_with?
    ///
    /// Verifica se a string começa com o prefixo especificado.
    /// "postgresql://" é uma forma alternativa de escrever "postgres://".
    ///
    /// # Retorna
    ///
    /// `true` se a URL do banco começar com "postgres://" ou "postgresql://".
    pub fn is_postgres(&self) -> bool {
        self.db_url.starts_with("postgres://") || self.db_url.starts_with("postgresql://")
    }

    /// Retorna `true` se estiver usando SQLite.
    ///
    /// # Retorna
    ///
    /// `true` se a URL do banco começar com "sqlite://".
    pub fn is_sqlite(&self) -> bool {
        self.db_url.starts_with("sqlite://")
    }
}
