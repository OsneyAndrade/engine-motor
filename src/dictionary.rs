use dashmap::DashMap;
use parking_lot::RwLock;
use redis::Commands;
use std::sync::atomic::{AtomicU64, Ordering, AtomicBool};
use tracing::{info, debug, error};

const MIN_SAMPLES: usize = 10;

const DICT_MAX_SIZE: usize = 112 * 1024; // 112KB


pub struct DictionaryManager {
    dictionaries: DashMap<String, Vec<u8>>,
    samples: DashMap<String, RwLock<Vec<Vec<u8>>>>,
    hits: AtomicU64,

    /// Cliente Redis opcional para Clustering
    redis: Option<redis::Client>,

    /// Flag de saúde do Redis (Circuit Breaker)
    ///
    /// Se Redis falhar múltiplas vezes, este flag é desligado
    /// para evitar tentativas de conexão desnecessárias.
    redis_online: AtomicBool,
}

impl DictionaryManager {

    pub fn new(redis: Option<redis::Client>) -> Self {
        Self {
            dictionaries: DashMap::new(),
            samples: DashMap::new(),
            hits: AtomicU64::new(0),
            redis,
            redis_online: AtomicBool::new(true),
        }
    }

    pub fn add_sample(&self, category: &str, data: &[u8]) {
        // Limita a amostra a 64KB
        let sample = if data.len() > 65536 { &data[..65536] } else { data };

        // Adiciona a amostra ao buffer da categoria
        self.samples
            .entry(category.to_string())
            .or_insert_with(|| RwLock::new(Vec::new()))
            .write()
            .push(sample.to_vec());

        // Verifica se já temos amostras suficientes para treinar
        let should_train = self
            .samples
            .get(category)
            .map(|e| e.read().len() >= MIN_SAMPLES)
            .unwrap_or(false);

        // Se temos amostras suficientes e ainda não temos dicionário, treina
        if should_train && !self.dictionaries.contains_key(category) {
            self.train(category);
        }
    }

    fn train(&self, category: &str) {
        // Obtém as amostras da categoria
        let samples_data = match self.samples.get(category) {
            Some(entry) => {
                let guard = entry.read();

                // Verifica se temos amostras suficientes
                if guard.len() < MIN_SAMPLES {
                    return;
                }

                // Clona os dados para processamento
                guard.clone()
            }
            None => return,
        };

        // Converte Vec<Vec<u8>> para Vec<&[u8]> (referências)
        let sample_refs: Vec<&[u8]> = samples_data.iter().map(|s| s.as_slice()).collect();

        // Usa o Zstd para treinar o dicionário
        match zstd::dict::from_samples(&sample_refs, DICT_MAX_SIZE) {
            Ok(dict) => {
                info!(
                    category = category,
                    dict_size = dict.len(),
                    samples = samples_data.len(),
                    "Dicionário treinado com sucesso"
                );

                // Salva o dicionário localmente
                self.dictionaries.insert(category.to_string(), dict.clone());

                // Distribui via Redis (Tier 3 - Clustering)
                if let Some(client) = &self.redis {
                    match client.get_connection() {
                        Ok(mut conn) => {
                            let key = format!("SYNTRA_DICT:{}", category);

                            // Salva o dicionário no Redis
                            let _: Result<(), _> = conn.set(&key, dict);

                            debug!(category = category, "Dicionário sincronizado com Redis");
                        }
                        Err(e) => error!("Falha ao conectar no Redis para sync: {}", e),
                    }
                }

                // Limpa as amostras usadas (economiza memória)
                if let Some(entry) = self.samples.get(category) {
                    entry.write().clear();
                }
            }
            Err(e) => {
                debug!(category = category, error = %e, "Falha ao treinar dicionário");
            }
        }
    }

    pub fn get_dictionary(&self, category: &str) -> Option<Vec<u8>> {
        // 1. Tenta cache local (mais rápido)
        if let Some(dict) = self.dictionaries.get(category) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Some(dict.value().clone());
        }

        // 2. Se falhar, tenta Redis (Tier 3) com Circuit Breaker
        if let Some(client) = &self.redis {
            // Só tenta se o Circuit Breaker estiver fechado (Redis online)
            if self.redis_online.load(Ordering::Relaxed) {
                match client.get_connection() {
                    Ok(mut conn) => {
                        let key = format!("SYNTRA_DICT:{}", category);

                        // Tenta buscar o dicionário no Redis
                        if let Ok(Some(dict)) = conn.get::<_, Option<Vec<u8>>>(&key) {
                            info!(category = category, "Dicionário recuperado do Redis");

                            // Salva no cache local para próxima vez
                            self.dictionaries.insert(category.to_string(), dict.clone());

                            self.hits.fetch_add(1, Ordering::Relaxed);
                            return Some(dict);
                        }
                    }
                    Err(e) => {
                        error!("Redis Offline — Ativando Circuit Breaker: {}", e);

                        // Abre o Circuit Breaker (marca Redis como offline)
                        self.redis_online.store(false, Ordering::Relaxed);
                    }
                }
            } else {
                // Circuit Breaker está aberto - tenta reconectar periodicamente
                match client.get_connection() {
                    Ok(mut conn) => {
                        // Testa se a conexão funciona com PING
                        if redis::cmd("PING").query::<String>(&mut conn).is_ok() {
                            info!("Redis reconectado — Restaurando Circuit Breaker");

                            // Fecha o Circuit Breaker (marca Redis como online)
                            self.redis_online.store(true, Ordering::Relaxed);

                            let key = format!("SYNTRA_DICT:{}", category);

                            // Tenta buscar o dicionário novamente
                            if let Ok(Some(dict)) = conn.get::<_, Option<Vec<u8>>>(&key) {
                                self.dictionaries.insert(category.to_string(), dict.clone());
                                self.hits.fetch_add(1, Ordering::Relaxed);
                                return Some(dict);
                            }
                        }
                    }
                    Err(_) => {
                        // Continua offline - tenta novamente na próxima chamada
                    }
                }
            }
        }

        None
    }

    pub fn try_reconnect_redis(&self) {
        if let Some(client) = &self.redis {
            // Só tenta se o Circuit Breaker estiver aberto (Redis offline)
            if !self.redis_online.load(Ordering::Relaxed) {
                if let Ok(mut conn) = client.get_connection() {
                    // Testa a conexão com PING
                    if redis::cmd("PING").query::<String>(&mut conn).is_ok() {
                        info!("Redis reconectado via try_reconnect_redis");

                        // Fecha o Circuit Breaker
                        self.redis_online.store(true, Ordering::Relaxed);
                    }
                }
            }
        }
    }

    /// Retorna o número de dicionários treinados.
    pub fn count(&self) -> usize {
        self.dictionaries.len()
    }

    /// Retorna o número total de hits de dicionário.
    pub fn total_hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    /// Retorna todas as categorias que têm dicionário treinado.
    pub fn categories(&self) -> Vec<String> {
        self.dictionaries.iter().map(|e| e.key().clone()).collect()
    }
}
