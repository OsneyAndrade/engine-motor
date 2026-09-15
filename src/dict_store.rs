use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use dashmap::DashMap;
use parking_lot::Mutex;
use redis::Commands;
use tracing::{debug, error, info, warn};

const MIN_SAMPLES: usize = 12;
const MIN_SAMPLE_BYTES: usize = 48 * 1024;
const DICT_MAX_SIZE: usize = 112 * 1024;
const DICT_MIN_SIZE: usize = 4 * 1024;
const SAMPLE_TO_DICT_RATIO: usize = 8;
const SAMPLE_MAX: usize = 64 * 1024;
const SAMPLE_BUDGET: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub struct Dictionary {
    pub id: String,
    pub digest: [u8; 32],
    pub bytes: Vec<u8>,
}

impl Dictionary {
    fn new(class: &str, bytes: Vec<u8>) -> Self {
        let digest: [u8; 32] = blake3::hash(&bytes).into();
        let id = format!("{}.{}", sanitize(class), hex::encode(&digest[..8]));
        Dictionary { id, digest, bytes }
    }

    pub fn size(&self) -> u32 {
        self.bytes.len() as u32
    }
}

#[derive(Default)]
struct SampleBuf {
    items: Vec<Vec<u8>>,
    bytes: usize,
}

pub struct DictionaryStore {
    dir: PathBuf,
    active: DashMap<String, Arc<Dictionary>>,
    by_id: DashMap<String, Arc<Dictionary>>,
    samples: DashMap<String, Mutex<SampleBuf>>,
    hits: AtomicU64,
    trained: AtomicU64,
    enabled: bool,

    redis: Option<redis::Client>,
    redis_online: AtomicBool,
}

impl DictionaryStore {
    pub fn open(dir: impl AsRef<Path>, enabled: bool, redis: Option<redis::Client>) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("criando diretório de dicionários {}", dir.display()))?;

        let store = DictionaryStore {
            dir,
            active: DashMap::new(),
            by_id: DashMap::new(),
            samples: DashMap::new(),
            hits: AtomicU64::new(0),
            trained: AtomicU64::new(0),
            enabled,
            redis,
            redis_online: AtomicBool::new(true),
        };
        store.load_from_disk()?;
        Ok(store)
    }

    fn load_from_disk(&self) -> Result<()> {
        let mut newest: std::collections::HashMap<String, (std::time::SystemTime, String)> =
            Default::default();

        let entries = match std::fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) => {
                warn!("diretório de dicionários ilegível: {}", e);
                return Ok(());
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("dict") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(&path) else {
                warn!("falha ao ler dicionário {}", path.display());
                continue;
            };

            let digest: [u8; 32] = blake3::hash(&bytes).into();
            let expected_suffix = hex::encode(&digest[..8]);
            if !stem.ends_with(&expected_suffix) {
                error!(
                    "dicionário {} corrompido (digest não corresponde ao id); ignorado",
                    path.display()
                );
                continue;
            }

            let class = stem
                .rsplit_once('.')
                .map(|(c, _)| c.to_string())
                .unwrap_or_else(|| stem.to_string());

            let dict = Arc::new(Dictionary {
                id: stem.to_string(),
                digest,
                bytes,
            });
            self.by_id.insert(dict.id.clone(), dict.clone());

            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            let replace = newest
                .get(&class)
                .map(|(t, _)| mtime > *t)
                .unwrap_or(true);
            if replace {
                newest.insert(class.clone(), (mtime, dict.id.clone()));
            }
        }

        for (class, (_, id)) in newest {
            if let Some(dict) = self.by_id.get(&id) {
                self.active.insert(class, dict.value().clone());
            }
        }

        if !self.by_id.is_empty() {
            info!(
                total = self.by_id.len(),
                ativos = self.active.len(),
                "dicionários carregados do disco"
            );
        }
        Ok(())
    }

    pub fn active_for(&self, class: &str) -> Option<Arc<Dictionary>> {
        if !self.enabled || class.is_empty() {
            return None;
        }
        let key = sanitize(class);
        let found = self.active.get(&key).map(|d| d.value().clone());
        if found.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        }
        found
    }

    pub fn resolve(&self, id: &str) -> Option<Arc<Dictionary>> {
        if id.is_empty() {
            return None;
        }
        if let Some(d) = self.by_id.get(id) {
            return Some(d.value().clone());
        }

        let path = self.path_for(id);
        if let Ok(bytes) = std::fs::read(&path) {
            if let Some(dict) = self.accept(id, bytes) {
                return Some(dict);
            }
        }

        if let Some(bytes) = self.redis_get(id) {
            if let Some(dict) = self.accept(id, bytes.clone()) {
                let _ = self.write_file(id, &bytes);
                return Some(dict);
            }
        }

        None
    }

    fn accept(&self, id: &str, bytes: Vec<u8>) -> Option<Arc<Dictionary>> {
        let digest: [u8; 32] = blake3::hash(&bytes).into();
        if !id.ends_with(&hex::encode(&digest[..8])) {
            error!(dictionary_id = id, "digest do dicionário não corresponde ao id");
            return None;
        }
        let dict = Arc::new(Dictionary {
            id: id.to_string(),
            digest,
            bytes,
        });
        self.by_id.insert(id.to_string(), dict.clone());
        Some(dict)
    }

    pub fn observe(&self, class: &str, data: &[u8]) {
        if !self.enabled || class.is_empty() || data.is_empty() {
            return;
        }
        let key = sanitize(class);
        if self.active.contains_key(&key) {
            return;
        }

        let sample = &data[..data.len().min(SAMPLE_MAX)];
        let ready = {
            let entry = self
                .samples
                .entry(key.clone())
                .or_insert_with(|| Mutex::new(SampleBuf::default()));
            let mut buf = entry.lock();
            if buf.bytes + sample.len() <= SAMPLE_BUDGET {
                buf.items.push(sample.to_vec());
                buf.bytes += sample.len();
            }
            buf.items.len() >= MIN_SAMPLES && buf.bytes >= MIN_SAMPLE_BYTES
        };

        if ready {
            self.train(&key);
        }
    }

    fn train(&self, key: &str) {
        let (samples, total_bytes) = {
            let Some(entry) = self.samples.get(key) else {
                return;
            };
            let mut buf = entry.lock();
            if buf.items.len() < MIN_SAMPLES {
                return;
            }
            let total = buf.bytes;
            buf.bytes = 0;
            (std::mem::take(&mut buf.items), total)
        };

        let target = (total_bytes / SAMPLE_TO_DICT_RATIO).clamp(DICT_MIN_SIZE, DICT_MAX_SIZE);
        let refs: Vec<&[u8]> = samples.iter().map(|s| s.as_slice()).collect();
        let bytes = match zstd::dict::from_samples(&refs, target) {
            Ok(b) if !b.is_empty() => b,
            Ok(_) => {
                debug!(categoria = key, "treino devolveu dicionário vazio");
                return;
            }
            Err(e) => {
                debug!(categoria = key, erro = %e, "treino de dicionário falhou");
                return;
            }
        };

        let dict = Arc::new(Dictionary::new(key, bytes));

        if let Err(e) = self.write_file(&dict.id, &dict.bytes) {
            error!(categoria = key, erro = %e, "falha ao persistir dicionário; não será ativado");
            return;
        }

        self.by_id.insert(dict.id.clone(), dict.clone());
        self.active.insert(key.to_string(), dict.clone());
        self.trained.fetch_add(1, Ordering::Relaxed);
        self.redis_put(&dict.id, &dict.bytes);

        info!(
            categoria = key,
            dictionary_id = dict.id,
            tamanho = dict.bytes.len(),
            alvo = target,
            amostras = samples.len(),
            amostras_bytes = total_bytes,
            "dicionário treinado e persistido"
        );
    }

    fn path_for(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{}.dict", id))
    }

    fn write_file(&self, id: &str, bytes: &[u8]) -> Result<()> {
        let final_path = self.path_for(id);
        if final_path.exists() {
            return Ok(());
        }
        let tmp = final_path.with_extension("dict.tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &final_path)?;
        Ok(())
    }

    fn redis_key(id: &str) -> String {
        format!("SYNTRA_DICT:{}", id)
    }

    fn redis_get(&self, id: &str) -> Option<Vec<u8>> {
        let client = self.redis.as_ref()?;
        if !self.redis_online.load(Ordering::Relaxed) {
            match client.get_connection() {
                Ok(mut conn) => {
                    if redis::cmd("PING").query::<String>(&mut conn).is_err() {
                        return None;
                    }
                    info!("Redis reconectado");
                    self.redis_online.store(true, Ordering::Relaxed);
                }
                Err(_) => return None,
            }
        }
        match client.get_connection() {
            Ok(mut conn) => conn.get::<_, Option<Vec<u8>>>(Self::redis_key(id)).ok().flatten(),
            Err(e) => {
                error!("Redis offline: {}", e);
                self.redis_online.store(false, Ordering::Relaxed);
                None
            }
        }
    }

    fn redis_put(&self, id: &str, bytes: &[u8]) {
        let Some(client) = self.redis.as_ref() else {
            return;
        };
        match client.get_connection() {
            Ok(mut conn) => {
                let _: Result<(), _> = conn.set(Self::redis_key(id), bytes);
            }
            Err(e) => error!("falha ao publicar dicionário no Redis: {}", e),
        }
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn total_count(&self) -> usize {
        self.by_id.len()
    }

    pub fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    pub fn trained_count(&self) -> u64 {
        self.trained.load(Ordering::Relaxed)
    }

    pub fn categories(&self) -> Vec<(String, String, u32)> {
        self.active
            .iter()
            .map(|e| (e.key().clone(), e.value().id.clone(), e.value().size()))
            .collect()
    }
}

fn sanitize(class: &str) -> String {
    class
        .chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' | '-' => c,
            'A'..='Z' => c.to_ascii_lowercase(),
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("syntra-dict-test-{}-{}", nome, std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    fn amostra(i: usize) -> Vec<u8> {
        format!(
            r#"{{"evento":"pedido.criado","id":{},"cliente":"acme-{}","itens":[{{"sku":"A-{}","qtd":{}}}],"status":"pendente","canal":"api"}}"#,
            i, i % 40, i % 90, i % 7
        )
        .repeat(30)
        .into_bytes()
    }

    #[test]
    fn treina_persiste_e_resolve_por_id() {
        let dir = tmpdir("treino");
        let store = DictionaryStore::open(&dir, true, None).unwrap();

        assert!(store.active_for("application/json").is_none());
        for i in 0..MIN_SAMPLES + 4 {
            store.observe("application/json", &amostra(i));
        }

        let dict = store
            .active_for("application/json")
            .expect("dicionário deveria ter sido treinado");
        assert!(dict.bytes.len() > 0);
        assert!(store.path_for(&dict.id).exists(), "não persistiu em disco");

        assert_eq!(store.resolve(&dict.id).unwrap().digest, dict.digest);

        let store2 = DictionaryStore::open(&dir, true, None).unwrap();
        let recarregado = store2.resolve(&dict.id).expect("perdeu o dicionário no restart");
        assert_eq!(recarregado.bytes, dict.bytes);
        assert!(store2.active_for("application/json").is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dicionario_acompanha_o_volume_de_amostras() {
        let dir = tmpdir("dimensionamento");
        let store = DictionaryStore::open(&dir, true, None).unwrap();

        for i in 0..600 {
            let amostra = format!(
                r#"{{"pedido":"PED-{i:06}","cliente":{{"id":{i},"uf":"SP"}},"itens":[{{"sku":"SKU-{i:05}","qtd":2}}],"status":"confirmado","canal":"api"}}"#
            );
            store.observe("application/json", amostra.as_bytes());
        }
        let d = store
            .active_for("application/json")
            .expect("deveria treinar com volume suficiente");
        assert!(
            d.bytes.len() <= DICT_MAX_SIZE,
            "dicionário acima do teto: {}",
            d.bytes.len()
        );
        assert!(
            d.bytes.len() < DICT_MAX_SIZE / 2,
            "volume pequeno não deveria gerar dicionário grande: {} bytes",
            d.bytes.len()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn id_e_enderecado_por_conteudo() {
        let a = Dictionary::new("application/json", vec![1, 2, 3, 4]);
        let b = Dictionary::new("application/json", vec![1, 2, 3, 4]);
        let c = Dictionary::new("application/json", vec![9, 9, 9, 9]);
        assert_eq!(a.id, b.id, "mesmo conteúdo deve gerar o mesmo id");
        assert_ne!(a.id, c.id, "conteúdo diferente deve gerar id diferente");
        assert!(a.id.starts_with("application_json."));
    }

    #[test]
    fn dicionario_corrompido_no_disco_e_recusado() {
        let dir = tmpdir("corrompido");
        std::fs::create_dir_all(&dir).unwrap();
        let id = "application_json.0011223344556677";
        std::fs::write(dir.join(format!("{}.dict", id)), b"conteudo-errado").unwrap();

        let store = DictionaryStore::open(&dir, true, None).unwrap();
        assert!(store.resolve(id).is_none(), "aceitou dicionário corrompido");
        assert_eq!(store.total_count(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn desabilitado_nao_treina_nem_entrega() {
        let dir = tmpdir("desabilitado");
        let store = DictionaryStore::open(&dir, false, None).unwrap();
        for i in 0..MIN_SAMPLES + 4 {
            store.observe("application/json", &amostra(i));
        }
        assert!(store.active_for("application/json").is_none());
        assert_eq!(store.active_count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn categoria_vazia_e_ignorada() {
        let dir = tmpdir("vazia");
        let store = DictionaryStore::open(&dir, true, None).unwrap();
        for i in 0..MIN_SAMPLES + 4 {
            store.observe("", &amostra(i));
        }
        assert!(store.active_for("").is_none());
        assert_eq!(store.active_count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retreino_nao_invalida_dicionario_anterior() {
        let dir = tmpdir("retreino");
        let store = DictionaryStore::open(&dir, true, None).unwrap();
        for i in 0..MIN_SAMPLES + 2 {
            store.observe("text/csv", &amostra(i));
        }
        let primeiro = store.active_for("text/csv").unwrap();

        store.active.remove(&sanitize("text/csv"));
        for i in 0..MIN_SAMPLES + 2 {
            store.observe("text/csv", format!("linha-{};valor;{}\n", i, i * 3).repeat(400).as_bytes());
        }
        let segundo = store.active_for("text/csv").unwrap();

        assert_ne!(primeiro.id, segundo.id);
        assert!(store.resolve(&primeiro.id).is_some());
        assert!(store.resolve(&segundo.id).is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
