use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sled::Db;

use crate::config::ESSENCE_EXTENSION;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectRef {
    pub refs: u32,
    pub original_size: u64,
    pub essence_size: u64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub names: Vec<String>,
}

const MAX_TRACKED_NAMES: usize = 32;

pub struct VaultManager {
    base_dir: PathBuf,
    index: Arc<Db>,
}

impl VaultManager {
    pub fn new(base_dir: impl AsRef<Path>, index_path: impl AsRef<Path>) -> Result<Self> {
        let base_dir = base_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&base_dir)
            .with_context(|| format!("criando vault em {}", base_dir.display()))?;
        let index = sled::open(index_path.as_ref())
            .with_context(|| format!("abrindo índice em {}", index_path.as_ref().display()))?;
        Ok(Self {
            base_dir,
            index: Arc::new(index),
        })
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn path_for(&self, hash: &[u8; 32]) -> PathBuf {
        let h = hex::encode(hash);
        self.base_dir
            .join(&h[0..2])
            .join(&h[2..4])
            .join(format!("{}.{}", h, ESSENCE_EXTENSION))
    }

    pub fn ensure_path(&self, hash: &[u8; 32]) -> Result<PathBuf> {
        let path = self.path_for(hash);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("criando shard {}", parent.display()))?;
        }
        Ok(path)
    }

    pub fn parse_id(id: &str) -> Result<[u8; 32]> {
        let cleaned = id
            .strip_suffix(&format!(".{}", ESSENCE_EXTENSION))
            .unwrap_or(id);
        let hex_part = cleaned.split('_').next().unwrap_or(cleaned);

        if hex_part.len() != 64 || !hex_part.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(anyhow!(
                "identificador inválido: esperado hash BLAKE3 de 64 dígitos hexadecimais"
            ));
        }

        let mut out = [0u8; 32];
        hex::decode_to_slice(hex_part.to_ascii_lowercase(), &mut out)
            .map_err(|_| anyhow!("identificador não é hexadecimal válido"))?;
        Ok(out)
    }

    pub fn get_ref(&self, hash: &[u8; 32]) -> Result<Option<ObjectRef>> {
        match self.index.get(hash)? {
            Some(raw) => Ok(serde_json::from_slice(&raw).ok()),
            None => Ok(None),
        }
    }

    pub fn record(
        &self,
        hash: &[u8; 32],
        name: &str,
        original_size: u64,
        essence_size: u64,
        now_ms: i64,
    ) -> Result<(ObjectRef, bool)> {
        let existing = self.get_ref(hash)?;
        let is_new = existing.is_none();

        let record = match existing {
            Some(mut r) => {
                r.refs = r.refs.saturating_add(1);
                r.updated_at_ms = now_ms;
                if !name.is_empty() && !r.names.iter().any(|n| n == name) {
                    if r.names.len() < MAX_TRACKED_NAMES {
                        r.names.push(name.to_string());
                    }
                }
                r
            }
            None => ObjectRef {
                refs: 1,
                original_size,
                essence_size,
                created_at_ms: now_ms,
                updated_at_ms: now_ms,
                names: if name.is_empty() {
                    Vec::new()
                } else {
                    vec![name.to_string()]
                },
            },
        };

        self.write_ref(hash, &record)?;
        Ok((record, is_new))
    }

    fn write_ref(&self, hash: &[u8; 32], record: &ObjectRef) -> Result<()> {
        self.index.insert(hash, serde_json::to_vec(record)?)?;
        Ok(())
    }

    pub fn release(&self, hash: &[u8; 32], now_ms: i64) -> Result<Option<u32>> {
        let Some(mut record) = self.get_ref(hash)? else {
            let path = self.path_for(hash);
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
            return Ok(None);
        };

        if record.refs > 1 {
            record.refs -= 1;
            record.updated_at_ms = now_ms;
            self.write_ref(hash, &record)?;
            return Ok(Some(record.refs));
        }

        let path = self.path_for(hash);
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("removendo {}", path.display()))?;
        }
        self.index.remove(hash)?;
        Ok(None)
    }

    pub fn purge(&self, hash: &[u8; 32]) -> Result<bool> {
        let path = self.path_for(hash);
        let existed = path.exists();
        if existed {
            std::fs::remove_file(&path)?;
        }
        self.index.remove(hash)?;
        Ok(existed)
    }

    pub fn contains(&self, hash: &[u8; 32]) -> bool {
        self.path_for(hash).exists()
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn flush(&self) -> Result<()> {
        self.index.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault(nome: &str) -> (VaultManager, PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("syntra-vault-{}-{}", nome, std::process::id()));
        let idx = std::env::temp_dir().join(format!("syntra-idx-{}-{}", nome, std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&idx);
        let v = VaultManager::new(&base, &idx).unwrap();
        (v, base, idx)
    }

    fn cleanup(base: &Path, idx: &Path) {
        let _ = std::fs::remove_dir_all(base);
        let _ = std::fs::remove_dir_all(idx);
    }

    #[test]
    fn caminho_e_shardeado_por_prefixo_do_hash() {
        let (v, base, idx) = vault("shard");
        let mut h = [0u8; 32];
        h[0] = 0xab;
        h[1] = 0xcd;
        let p = v.path_for(&h);
        assert!(p.starts_with(&base));
        assert!(p.to_string_lossy().contains("/ab/cd/"));
        assert!(p.to_string_lossy().ends_with(".syntra"));
        cleanup(&base, &idx);
    }

    #[test]
    fn parse_id_rejeita_travessia_de_caminho() {
        for malicioso in [
            "../../../../etc/passwd",
            "aa/../../etc/passwd",
            "..",
            "",
            "zz",
            "aabb",
            "/absolute/path",
            "aa\0bb",
            "ção",
        ] {
            assert!(
                VaultManager::parse_id(malicioso).is_err(),
                "aceitou id malicioso: {malicioso:?}"
            );
        }
    }

    #[test]
    fn parse_id_aceita_formatos_validos_e_legados() {
        let h = "a".repeat(64);
        assert!(VaultManager::parse_id(&h).is_ok());
        assert!(VaultManager::parse_id(&format!("{h}.syntra")).is_ok());
        assert!(VaultManager::parse_id(&format!("{h}_1700000000000")).is_ok());
        assert_eq!(
            VaultManager::parse_id(&h.to_uppercase()).unwrap(),
            VaultManager::parse_id(&h).unwrap(),
            "hash deve ser case-insensitive"
        );
    }

    #[test]
    fn contador_de_referencias_dedupica_envios_repetidos() {
        let (v, base, idx) = vault("refs");
        let h = [7u8; 32];

        let (r1, novo1) = v.record(&h, "relatorio.csv", 1000, 300, 1).unwrap();
        assert!(novo1);
        assert_eq!(r1.refs, 1);

        let (r2, novo2) = v.record(&h, "relatorio-copia.csv", 1000, 300, 2).unwrap();
        assert!(!novo2, "segundo envio do mesmo conteúdo não é novo objeto");
        assert_eq!(r2.refs, 2);
        assert_eq!(r2.names.len(), 2);

        let (r3, _) = v.record(&h, "relatorio.csv", 1000, 300, 3).unwrap();
        assert_eq!(r3.refs, 3);
        assert_eq!(r3.names.len(), 2);

        cleanup(&base, &idx);
    }

    #[test]
    fn release_remove_o_objeto_somente_na_ultima_referencia() {
        let (v, base, idx) = vault("release");
        let h = [9u8; 32];
        let path = v.ensure_path(&h).unwrap();
        std::fs::write(&path, b"essencia").unwrap();

        v.record(&h, "a.bin", 10, 8, 1).unwrap();
        v.record(&h, "b.bin", 10, 8, 2).unwrap();

        assert_eq!(v.release(&h, 3).unwrap(), Some(1));
        assert!(path.exists(), "não deveria remover com referência pendente");

        assert_eq!(v.release(&h, 4).unwrap(), None);
        assert!(!path.exists(), "deveria remover na última referência");
        assert!(v.get_ref(&h).unwrap().is_none());

        cleanup(&base, &idx);
    }
}
