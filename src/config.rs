use tracing::warn;

use crate::planner::{Effort, Limits};

pub const ESSENCE_EXTENSION: &str = "syntra";

#[derive(Debug, Clone)]
pub struct Config {
    pub vault_path: String,
    pub sled_index_path: String,
    pub dict_path: String,
    pub db_url: String,
    pub watch_dir: String,

    pub bind_addr: String,
    pub max_body_size_mb: usize,
    pub cors_origins: Vec<String>,
    pub api_keys: Vec<String>,

    pub compress_multiplier: usize,

    pub default_effort: Effort,
    pub verify_on_write: bool,
    pub dictionaries_enabled: bool,
    pub redis_url: Option<String>,
    pub limits: Limits,

    pub watch_enabled: bool,
    pub watch_delete_source: bool,
}

impl Config {
    pub fn from_env() -> Self {
        let cfg = Config {
            vault_path: env_string("SYNTRA_VAULT_PATH", "essence_vault"),
            sled_index_path: env_string("SYNTRA_SLED_INDEX", "metadata_index.sled"),
            dict_path: env_string("SYNTRA_DICT_PATH", "dictionaries"),
            db_url: env_string(
                "SYNTRA_DB_URL",
                "postgres://syntra:syntra123@localhost:5432/syntra_monitoring",
            ),
            watch_dir: env_string("SYNTRA_WATCH_DIR", "watch_in"),

            bind_addr: env_string("SYNTRA_BIND_ADDR", "0.0.0.0:3002"),
            max_body_size_mb: env_usize("SYNTRA_MAX_BODY_MB", 4096),
            cors_origins: env_list("SYNTRA_CORS_ORIGINS"),
            api_keys: env_list("SYNTRA_API_KEYS"),

            compress_multiplier: env_usize("SYNTRA_COMPRESS_MULTIPLIER", 4).max(1),

            default_effort: env_effort("SYNTRA_DEFAULT_EFFORT", Effort::Balanced),
            verify_on_write: env_bool("SYNTRA_VERIFY_ON_WRITE", true),
            dictionaries_enabled: env_bool("SYNTRA_DICTIONARIES", true),
            redis_url: std::env::var("SYNTRA_REDIS_URL").ok().filter(|s| !s.is_empty()),
            limits: Limits {
                probe_threshold: env_usize("SYNTRA_PROBE_THRESHOLD_MB", 48) * 1024 * 1024,
                probe_sample: env_usize("SYNTRA_PROBE_SAMPLE_MB", 8) * 1024 * 1024,
                keep_output_max: env_usize("SYNTRA_KEEP_OUTPUT_MB", 16) * 1024 * 1024,
            },

            watch_enabled: env_bool("SYNTRA_WATCH_ENABLED", true),
            watch_delete_source: env_bool("SYNTRA_WATCH_DELETE_SOURCE", false),
        };

        if cfg.api_keys.is_empty() {
            warn!(
                "SYNTRA_API_KEYS não definido: a API está aberta. \
                 Defina chaves separadas por vírgula antes de expor o serviço."
            );
        }
        if cfg.cors_origins.is_empty() {
            warn!("SYNTRA_CORS_ORIGINS não definido: CORS liberado para qualquer origem.");
        }
        cfg
    }

    pub fn auth_enabled(&self) -> bool {
        !self.api_keys.is_empty()
    }
}

fn env_string(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn env_usize(key: &str, default: usize) -> usize {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => match v.trim().parse::<usize>() {
            Ok(n) if n > 0 => n,
            _ => {
                warn!("{key}='{v}' inválido; usando {default}");
                default
            }
        },
        _ => default,
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => match v.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            other => {
                warn!("{key}='{other}' inválido; usando {default}");
                default
            }
        },
        Err(_) => default,
    }
}

fn env_list(key: &str) -> Vec<String> {
    std::env::var(key)
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn env_effort(key: &str, default: Effort) -> Effort {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => Effort::from_name(&v).unwrap_or_else(|| {
            warn!("{key}='{v}' inválido; usando {}", default.as_str());
            default
        }),
        _ => default,
    }
}
