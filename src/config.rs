use tracing::warn;

use crate::planner::{Effort, Limits};

pub const ESSENCE_EXTENSION: &str = "syntra";

pub const DEFAULT_BLOCK_LOG2: u8 = 20;
pub const DEFAULT_MAX_INFLIGHT_MB: usize = 128;
pub const DEFAULT_INFLIGHT_WAIT_MS: u64 = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerFormat {
    V3,
    V4,
}

impl ContainerFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            ContainerFormat::V3 => "v3",
            ContainerFormat::V4 => "v4",
        }
    }

    pub fn version(self) -> i32 {
        match self {
            ContainerFormat::V3 => 3,
            ContainerFormat::V4 => 4,
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "v3" | "3" => Some(ContainerFormat::V3),
            "v4" | "4" => Some(ContainerFormat::V4),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BlockSettings {
    pub format: ContainerFormat,
    pub block_log2: u8,
    pub max_inflight_mb: usize,
    pub inflight_wait_ms: u64,
}

impl Default for BlockSettings {
    fn default() -> Self {
        BlockSettings {
            format: ContainerFormat::V3,
            block_log2: DEFAULT_BLOCK_LOG2,
            max_inflight_mb: DEFAULT_MAX_INFLIGHT_MB,
            inflight_wait_ms: DEFAULT_INFLIGHT_WAIT_MS,
        }
    }
}

impl BlockSettings {
    pub fn from_env() -> Self {
        let format = match std::env::var("SYNTRA_CONTAINER_FORMAT") {
            Ok(v) if !v.trim().is_empty() => ContainerFormat::from_name(&v).unwrap_or_else(|| {
                warn!("SYNTRA_CONTAINER_FORMAT='{v}' inválido (use v3 ou v4); usando v3");
                ContainerFormat::V3
            }),
            _ => ContainerFormat::V3,
        };

        let block_log2 = match std::env::var("SYNTRA_BLOCK_SIZE_LOG2") {
            Ok(v) if !v.trim().is_empty() => match v.trim().parse::<u8>() {
                Ok(n) if (crate::blockfile::MIN_BLOCK_LOG2..=crate::blockfile::MAX_BLOCK_LOG2)
                    .contains(&n) =>
                {
                    n
                }
                _ => {
                    warn!(
                        "SYNTRA_BLOCK_SIZE_LOG2='{v}' fora da faixa {}..={}; usando {DEFAULT_BLOCK_LOG2}",
                        crate::blockfile::MIN_BLOCK_LOG2,
                        crate::blockfile::MAX_BLOCK_LOG2
                    );
                    DEFAULT_BLOCK_LOG2
                }
            },
            _ => DEFAULT_BLOCK_LOG2,
        };

        BlockSettings {
            format,
            block_log2,
            max_inflight_mb: env_usize("SYNTRA_MAX_INFLIGHT_MB", DEFAULT_MAX_INFLIGHT_MB),
            inflight_wait_ms: env_usize("SYNTRA_INFLIGHT_WAIT_MS", DEFAULT_INFLIGHT_WAIT_MS as usize)
                as u64,
        }
    }

    pub fn block_size(&self) -> usize {
        1usize << self.block_log2
    }

    pub fn inflight_units(&self) -> usize {
        let bytes = self.max_inflight_mb.saturating_mul(1024 * 1024);
        (bytes >> self.block_log2).max(1)
    }

    pub fn retry_after_secs(&self) -> u64 {
        self.inflight_wait_ms.div_ceil(1000).max(1)
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub vault_path: String,
    pub sled_index_path: String,
    pub dict_path: String,
    pub db_url: String,
    pub watch_dir: String,

    pub bind_addr: String,
    pub max_body_size_mb: usize,
    pub max_decompressed_mb: usize,
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
        let max_body_size_mb = env_usize("SYNTRA_MAX_BODY_MB", 4096);
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
            max_body_size_mb,
            max_decompressed_mb: env_usize("SYNTRA_MAX_DECOMPRESSED_MB", max_body_size_mb),
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

    pub fn max_decompressed_bytes(&self) -> u64 {
        (self.max_decompressed_mb as u64).saturating_mul(1024 * 1024)
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
