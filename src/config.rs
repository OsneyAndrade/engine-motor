pub const ESSENCE_EXTENSION: &str = "syntra";

pub struct Config {

    pub vault_path: String,

    pub sled_index_path: String,

    pub db_url: String,

    pub watch_dir: String,

    pub bind_addr: String,

    pub max_body_size_mb: usize,

    pub compress_multiplier: usize,
}

impl Config {

    pub fn from_env() -> Self {
        Self {

            vault_path: std::env::var("SYNTRA_VAULT_PATH")
                .unwrap_or_else(|_| "essence_vault".to_string()),

            sled_index_path: std::env::var("SYNTRA_SLED_INDEX")
                .unwrap_or_else(|_| "metadata_index.sled".to_string()),

            db_url: std::env::var("SYNTRA_DB_URL")
                .unwrap_or_else(|_| "postgres://syntra:syntra123@localhost:5432/syntra_monitoring".to_string()),

            watch_dir: std::env::var("SYNTRA_WATCH_DIR")
                .unwrap_or_else(|_| "watch_in".to_string()),

            bind_addr: std::env::var("SYNTRA_BIND_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:3002".to_string()),

            max_body_size_mb: std::env::var("SYNTRA_MAX_BODY_MB")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(4096),

            compress_multiplier: std::env::var("SYNTRA_COMPRESS_MULTIPLIER")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(4), // 4x I/O vs CPU
        }
    }

    pub fn is_postgres(&self) -> bool {
        self.db_url.starts_with("postgres://") || self.db_url.starts_with("postgresql://")
    }

    pub fn is_sqlite(&self) -> bool {
        self.db_url.starts_with("sqlite://")
    }
}
