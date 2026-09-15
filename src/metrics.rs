
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

use crate::proto::Algorithm;

const LATENCY_BUCKETS: [f64; 10] = [1.0, 5.0, 10.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0];

pub struct EngineMetrics {
    /// Total de bytes recebidos para processamento (entrada)
    pub bytes_in: AtomicU64,

    /// Total de bytes após destilação (saída)
    pub bytes_out: AtomicU64,

    /// Número total de arquivos processados com sucesso
    pub files_processed: AtomicU64,

    /// Número total de erros ocorridos durante o processamento
    pub errors: AtomicU64,

    /// Contador de vezes que o algoritmo Zstd Fast foi usado
    pub model_fast_count: AtomicU64,

    /// Contador de vezes que o algoritmo Zstd Balanced foi usado
    pub model_balanced_count: AtomicU64,

    /// Contador de vezes que o algoritmo Zstd Max foi usado
    pub model_max_count: AtomicU64,

    /// Contador de vezes que o algoritmo LZ4 foi usado
    pub model_ultra_fast_count: AtomicU64,

    /// Contador de vezes que Passthrough (sem compressão) foi usado
    pub passthrough_count: AtomicU64,

    /// Contadores para cada bucket do histograma de latência
    ///
    /// Cada posição do vetor corresponde a um bucket em LATENCY_BUCKETS.
    /// O valor em cada posição é quantas operações caíram naquele bucket.
    pub latency_buckets: Vec<AtomicU64>,

    /// Soma total de todos os tempos de processamento (em ms)
    pub latency_sum_ms: AtomicU64,

    /// Total de bytes economizados por deduplicação de blocos
    pub dedup_bytes_saved: AtomicU64,

    /// Número de vezes que um bloco duplicado foi encontrado
    pub dedup_hits: AtomicU64,

    /// Momento em que o motor foi iniciado
    ///
    /// Usado para calcular o uptime (tempo de atividade).
    pub start_time: Instant,

    /// Monitor de recursos do sistema (CPU e RAM)
    ///
    /// Mutex permite acesso exclusivo quando precisamos ler os valores.
    pub system_monitor: Mutex<System>,

    /// Número de arquivos rejeitados (sem compressão ou economia < 5%)
    pub rejected_count: AtomicU64,

    /// Total de bytes dos arquivos rejeitados
    pub rejected_bytes: AtomicU64,

    /// Último valor de uso de CPU reportado pelo Load Balancer
    pub last_cpu_usage: Mutex<f32>,
}

impl EngineMetrics {
    pub fn new() -> Self {
        // Cria os contadores para cada bucket do histograma
        let mut buckets = Vec::with_capacity(LATENCY_BUCKETS.len());
        for _ in 0..LATENCY_BUCKETS.len() {
            buckets.push(AtomicU64::new(0));
        }

        Self {
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            files_processed: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            model_fast_count: AtomicU64::new(0),
            model_balanced_count: AtomicU64::new(0),
            model_max_count: AtomicU64::new(0),
            model_ultra_fast_count: AtomicU64::new(0),
            passthrough_count: AtomicU64::new(0),
            latency_buckets: buckets,
            latency_sum_ms: AtomicU64::new(0),
            dedup_bytes_saved: AtomicU64::new(0),
            dedup_hits: AtomicU64::new(0),

            // Registra o momento de inicialização
            start_time: Instant::now(),

            // Inicializa o monitor de sistema focado em CPU e memória
            system_monitor: Mutex::new(System::new_with_specifics(
                RefreshKind::nothing()
                    .with_cpu(CpuRefreshKind::everything())
                    .with_memory(MemoryRefreshKind::everything()),
            )),
            rejected_count: AtomicU64::new(0),
            rejected_bytes: AtomicU64::new(0),
            last_cpu_usage: Mutex::new(0.0),
        }
    }

    pub fn load_from_db(&self, db_stats: &serde_json::Value, total_duration_ms: f64) {
        use std::sync::atomic::Ordering;

        // Carrega contadores principais - usa fetch_add para não sobrescrever
        // valores que possam ter sido definidos antes
        if let Some(bytes_in) = db_stats["bytes_in"].as_u64() {
            self.bytes_in.fetch_add(bytes_in, Ordering::Relaxed);
        }
        if let Some(bytes_out) = db_stats["bytes_out"].as_u64() {
            self.bytes_out.fetch_add(bytes_out, Ordering::Relaxed);
        }
        if let Some(files_processed) = db_stats["files_processed"].as_u64() {
            self.files_processed.fetch_add(files_processed, Ordering::Relaxed);
        }

        // Carrega soma de latência (convertendo f64 para u64)
        if total_duration_ms > 0.0 {
            self.latency_sum_ms.fetch_add(total_duration_ms as u64, Ordering::Relaxed);

            // Estima buckets do histograma baseado na média
            // Esta é uma aproximação já que não temos os dados originais
            let count = self.files_processed.load(Ordering::Relaxed);
            if count > 0 {
                let avg_ms = total_duration_ms / count as f64;
                // Distribui através dos buckets de forma aproximada
                for bucket in &LATENCY_BUCKETS {
                    if avg_ms <= *bucket {
                        // Cada arquivo conta para todos os buckets >= média
                        for b in &self.latency_buckets {
                            b.fetch_add(count, Ordering::Relaxed);
                        }
                        break;
                    }
                }
            }
        }
    }

    pub fn record_cpu_usage(&self, usage: f32) {
        if let Ok(mut last) = self.last_cpu_usage.lock() {
            *last = usage;
        }
    }

    pub fn record_strategy(&self, algo: Algorithm) {
        match algo {
            Algorithm::ZstdFast => {
                self.model_fast_count.fetch_add(1, Ordering::Relaxed);
            }
            Algorithm::ZstdBalanced => {
                self.model_balanced_count.fetch_add(1, Ordering::Relaxed);
            }
            Algorithm::ZstdMax => {
                self.model_max_count.fetch_add(1, Ordering::Relaxed);
            }
            Algorithm::Lz4 => {
                self.model_ultra_fast_count.fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                self.passthrough_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn record_latency(&self, ms: f64) {
        // Adiciona o tempo ao somatório total
        let prev = self.latency_sum_ms.fetch_add(ms as u64, Ordering::Relaxed);

        // Log obrigatório para debug
        tracing::info!(
            "record_latency: {:.2}ms, antes: {}, depois: {}",
            ms,
            prev,
            prev + (ms as u64)
        );

        for (i, &bucket) in LATENCY_BUCKETS.iter().enumerate() {
            if ms <= bucket {
                // Se o tempo é menor ou igual ao bucket, incrementa o contador
                self.latency_buckets[i].fetch_add(1, Ordering::Relaxed);
            }
        }
        // O bucket "+inf" (infinito) é implícito e contém todas as operações.
        // O contador total é mantido separadamente em `files_processed`.
    }

    pub fn to_prometheus(&self) -> String {
        // Calcula métricas básicas
        let uptime = self.start_time.elapsed().as_secs();
        let b_in = self.bytes_in.load(Ordering::Relaxed);
        let b_out = self.bytes_out.load(Ordering::Relaxed);

        // Ratio de compressão (quanto menor = melhor)
        let ratio = if b_in > 0 { b_out as f64 / b_in as f64 } else { 1.0 };
        let files = self.files_processed.load(Ordering::Relaxed);

        // Constrói a string com as métricas principais
        let mut prom = format!(
            "# HELP syntra_bytes_in_total Total bytes recebidos para destilação\n\
             # TYPE syntra_bytes_in_total counter\n\
             syntra_bytes_in_total {b_in}\n\
             # HELP syntra_essence_bytes_total Total bytes de essência gerados\n\
             # TYPE syntra_essence_bytes_total counter\n\
             syntra_essence_bytes_total {b_out}\n\
             # HELP syntra_distillation_ratio Ratio de destilação global\n\
             # TYPE syntra_distillation_ratio gauge\n\
             syntra_distillation_ratio {ratio:.4}\n\
             # HELP syntra_items_total Itens processados\n\
             # TYPE syntra_items_total counter\n\
             syntra_items_total {files}\n\
             # HELP syntra_errors_total Erros de processamento\n\
             # TYPE syntra_errors_total counter\n\
             syntra_errors_total {errors}\n\
             # HELP syntra_uptime_seconds Tempo ativo do motor\n\
             # TYPE syntra_uptime_seconds gauge\n\
             syntra_uptime_seconds {uptime}\n\
             # HELP syntra_model_usage Uso por modelo de processamento\n\
             # TYPE syntra_model_usage counter\n\
             syntra_model_usage{{model=\"fast\"}} {zf}\n\
             syntra_model_usage{{model=\"balanced\"}} {zb}\n\
             syntra_model_usage{{model=\"max_density\"}} {zm}\n\
             syntra_model_usage{{model=\"ultra_fast\"}} {l4}\n\
             syntra_model_usage{{model=\"passthrough\"}} {pt}\n",
            errors = self.errors.load(Ordering::Relaxed),
            zf = self.model_fast_count.load(Ordering::Relaxed),
            zb = self.model_balanced_count.load(Ordering::Relaxed),
            zm = self.model_max_count.load(Ordering::Relaxed),
            l4 = self.model_ultra_fast_count.load(Ordering::Relaxed),
            pt = self.passthrough_count.load(Ordering::Relaxed),
        );

        // Adiciona métricas de sistema (CPU e RAM)
        if let Ok(mut sys) = self.system_monitor.lock() {
            // Atualiza os dados de CPU e memória
            sys.refresh_cpu_all();
            sys.refresh_memory();

            prom.push_str(&format!(
                "# HELP syntra_system_cpu_usage Uso de CPU (0-100)\n\
                 # TYPE syntra_system_cpu_usage gauge\n\
                 syntra_system_cpu_usage {:.2}\n\
                 # HELP syntra_system_memory_used_bytes Memória RAM usada em bytes\n\
                 # TYPE syntra_system_memory_used_bytes gauge\n\
                 syntra_system_memory_used_bytes {}\n",
                sys.global_cpu_usage(),
                sys.used_memory()
            ));
        }

        // Adiciona o histograma de latência
        prom.push_str("# HELP syntra_processing_latency_ms Latência de geração de essência em ms\n");
        prom.push_str("# TYPE syntra_processing_latency_ms histogram\n");

        // Histogramas Prometheus são cumulativos
        let mut cumulative_count = 0;
        for (i, &bucket) in LATENCY_BUCKETS.iter().enumerate() {
            // Soma todos os buckets até este ponto
            cumulative_count += self.latency_buckets[i].load(Ordering::Relaxed);

            // Formato: syntra_processing_latency_ms_bucket{le="10.0"} 123
            prom.push_str(&format!(
                "syntra_processing_latency_ms_bucket{{le=\"{:.1}\"}} {}\n",
                bucket, cumulative_count
            ));
        }

        // Adiciona soma e contador total
        prom.push_str(&format!(
            "syntra_processing_latency_ms_sum {}\n",
            self.latency_sum_ms.load(Ordering::Relaxed)
        ));
        prom.push_str(&format!(
            "syntra_processing_latency_ms_count {}\n",
            files
        ));

        prom
    }


    pub fn to_json(
        &self,
        dict_count: usize,
        dict_hits: u64,
    ) -> serde_json::Value {
        // Calcula métricas básicas
        let uptime = self.start_time.elapsed().as_secs();
        let b_in = self.bytes_in.load(Ordering::Relaxed);
        let b_out = self.bytes_out.load(Ordering::Relaxed);

        // Porcentagem de economia (quanto menor o output, melhor)
        let savings = if b_in > 0 {
            (1.0 - b_out as f64 / b_in as f64) * 100.0
        } else {
            0.0
        };

        // Obtém métricas de sistema (CPU e RAM)
        let (cpu, ram) = if let Ok(mut sys) = self.system_monitor.lock() {
            sys.refresh_cpu_all();
            sys.refresh_memory();

            // Tenta usar o valor do Load Balancer se disponível
            let cpu_val = if let Ok(last) = self.last_cpu_usage.lock() {
                if *last > 0.0 { *last } else { sys.global_cpu_usage() }
            } else {
                sys.global_cpu_usage()
            };

            (cpu_val, sys.used_memory())
        } else {
            (0.0, 0)
        };

        serde_json::json!({
            "engine": "Syntra Engine",
            "uptime_seconds": uptime,
            "roi": {
                "total_bytes_in": b_in,
                "essence_bytes_out": b_out,
                "efficiency_pct": format!("{:.1}%", savings),
                "items_processed": self.files_processed.load(Ordering::Relaxed),
                "errors": self.errors.load(Ordering::Relaxed),
            },
            "deduplication": {
                "bytes_saved": self.dedup_bytes_saved.load(Ordering::Relaxed),
                "hits": self.dedup_hits.load(Ordering::Relaxed),
                "mode": "per_file_self_contained"
            },
            "context_models": {
                "active_trained_count": dict_count,
                "hits": dict_hits,
            },
            "processing_strategies": {
                "fast": self.model_fast_count.load(Ordering::Relaxed),
                "balanced": self.model_balanced_count.load(Ordering::Relaxed),
                "max_density": self.model_max_count.load(Ordering::Relaxed),
                "ultra_fast": self.model_ultra_fast_count.load(Ordering::Relaxed),
                "passthrough": self.passthrough_count.load(Ordering::Relaxed),
            },
            "system": {
                "cpu_usage_pct": format!("{:.1}%", cpu),
                "ram_used_bytes": ram,
                "ram_used_human": format!("{:.2} GiB", ram as f64 / 1024.0 / 1024.0 / 1024.0)
            },
            "latency": {
                "sum_ms": self.latency_sum_ms.load(Ordering::Relaxed),
                "count": self.files_processed.load(Ordering::Relaxed),
            },
            "rejected": {
                "count": self.rejected_count.load(Ordering::Relaxed),
                "bytes": self.rejected_bytes.load(Ordering::Relaxed),
            }
        })
    }
}
