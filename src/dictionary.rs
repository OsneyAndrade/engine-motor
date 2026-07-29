//! # Módulo de Gerenciamento de Dicionários Zstd
//!
//! Este módulo implementa o auto-treinamento de dicionários Zstd para
//! melhorar a compressão de dados repetitivos ou estruturados.
//!
//! # O que é um Dicionário Zstd?
//!
//! Um dicionário Zstd é uma amostra de dados "típicos" que é usada para
//! treinar o compressor. Quando o compressor conhece os padrões comuns
//! dos dados, ele pode comprimi-los muito mais eficientemente.
//!
//! # Como Funciona
//!
//! 1. **Coleta de Amostras**: Cada arquivo processado fornece uma amostra
//! 2. **Acúmulo**: Quando temos 10+ amostras de um tipo, iniciamos o treinamento
//! 3. **Treinamento**: O Zstd analisa as amostras e cria um dicionário
//! 4. **Uso**: Próximos arquivos do mesmo tipo usam o dicionário
//! 5. **Compartilhamento**: Dicionários podem ser compartilhados via Redis
//!
//! # Vantagens
//!
//! - **Melhor Compressão**: Dicionários podem melhorar a compressão em 20-50%
//! - **Automático**: Não precisa configurar manualmente
//! - **Específico por Tipo**: Cada MIME type tem seu próprio dicionário

use dashmap::DashMap;
use parking_lot::RwLock;
use redis::Commands;
use std::sync::atomic::{AtomicU64, Ordering, AtomicBool};
use tracing::{info, debug, error};

/// Número mínimo de amostras antes de treinar um dicionário.
///
/// # Por que 10?
///
/// - Menos de 10: Amostras insuficientes para criar um dicionário útil
/// - Mais de 10: Melhor dicionário, mas demora mais para treinar
const MIN_SAMPLES: usize = 10;

/// Tamanho máximo de um dicionário treinado.
///
/// # Por que 112KB?
///
/// - Zstd recomenda dicionários de até 112KB para bom balance
/// - Dicionários maiores melhoram pouco a compressão
/// - Dicionários maiores tornam a compressão mais lenta
const DICT_MAX_SIZE: usize = 112 * 1024; // 112KB

/// Gerenciador de dicionários Zstd auto-treinados.
///
/// Esta estrutura gerencia a coleta de amostras, treinamento de dicionários
/// e compartilhamento via Redis para ambientes distribuídos.
///
/// # Campos
///
/// - `dictionaries`: Dicionários já treinados (cache local)
/// - `samples`: Buffer de amostras aguardando treinamento
/// - `hits`: Contador de vezes que um dicionário foi usado
/// - `redis`: Cliente Redis opcional para compartilhamento
/// - `redis_online`: Flag indicando se Redis está acessível
pub struct DictionaryManager {
    /// Dicionários treinados localmente: categoria → bytes do dicionário
    dictionaries: DashMap<String, Vec<u8>>,

    /// Buffer de amostras para treinamento futuro
    ///
    /// Cada categoria (ex: "application/json") tem seu próprio buffer
    /// de amostras. Quando há amostras suficientes, o dicionário é treinado.
    samples: DashMap<String, RwLock<Vec<Vec<u8>>>>,

    /// Contagem de usos de dicionário
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
    /// Cria um novo gerenciador de dicionários.
    ///
    /// # Parâmetros
    ///
    /// * `redis` - Cliente Redis opcional para compartilhamento de dicionários
    ///
    /// # Retorna
    ///
    /// Uma nova instância de DictionaryManager
    pub fn new(redis: Option<redis::Client>) -> Self {
        Self {
            dictionaries: DashMap::new(),
            samples: DashMap::new(),
            hits: AtomicU64::new(0),
            redis,
            redis_online: AtomicBool::new(true),
        }
    }

    /// Adiciona uma amostra de dados para treinamento futuro.
    ///
    /// # O que faz
    ///
    /// 1. Adiciona os dados ao buffer de amostras da categoria
    /// 2. Se já temos amostras suficientes, inicia o treinamento
    ///
    /// # Por que limitar a 64KB?
    ///
    /// - Amostras maiores não melhoram muito o dicionário
    /// - Amostras menores são mais rápidas de processar
    ///
    /// # Parâmetros
    ///
    /// * `category` - Categoria MIME dos dados (ex: "application/json")
    /// * `data` - Amostra de dados a ser adicionada
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

    /// Treina um dicionário a partir das amostras coletadas.
    ///
    /// # O que faz
    ///
    /// 1. Coleta todas as amostras da categoria
    /// 2. Usa o Zstd para criar um dicionário a partir das amostras
    /// 3. Salva o dicionário localmente
    /// 4. (Opcional) Compartilha o dicionário via Redis
    /// 5. Limpa as amostras usadas
    ///
    /// # Por que isso é assíncrono na prática?
    ///
    /// O treinamento pode levar alguns segundos, então idealmente seria
    /// executado em background. Nesta implementação, é síncrono para
    /// simplificar.
    ///
    /// # Parâmetros
    ///
    /// * `category` - Categoria para treinar o dicionário
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

    /// Obtém um dicionário treinado para uma categoria específica.
    ///
    /// # Estratégia de Busca
    ///
    /// 1. Tenta o cache local primeiro (mais rápido)
    /// 2. Se não encontrar, tenta o Redis (compartilhamento global)
    /// 3. Se Redis estiver offline, usa Circuit Breaker para não tentar sempre
    ///
    /// # O que é Circuit Breaker?
    ///
    /// Se Redis falhar múltiplas vezes, paramos de tentar conectar
    /// por um tempo. Isso evita lentid causada por tentativas de conexão
    /// que vão falhar de qualquer forma.
    ///
    /// # Parâmetros
    ///
    /// * `category` - Categoria MIME do arquivo
    ///
    /// # Retorna
    ///
    /// `Some(dict)` se encontrar, `None` se não existir dicionário
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

    /// Tenta reconectar ao Redis (pode ser chamado periodicamente).
    ///
    /// # Quando usar
    ///
    /// Esta função pode ser chamada por um timer periódico para tentar
    /// reconectar ao Redis sem bloquear operações normais.
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
