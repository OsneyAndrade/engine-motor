use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

use crate::codec::{Codec, CATALOG};

const LATENCY_BUCKETS: [f64; 12] = [
    0.5, 1.0, 5.0, 10.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 30000.0,
];

pub struct EngineMetrics {
    pub bytes_in: AtomicU64,
    pub bytes_out: AtomicU64,
    pub files_processed: AtomicU64,
    pub reconstructions: AtomicU64,
    pub errors: AtomicU64,
    pub verify_failures: AtomicU64,

    codec_counts: Vec<AtomicU64>,

    latency_buckets: Vec<AtomicU64>,
    latency_sum_us: AtomicU64,
    latency_count: AtomicU64,

    pub dedup_bytes_saved: AtomicU64,
    pub dedup_hits: AtomicU64,

    pub start_time: Instant,
    pub system_monitor: Mutex<System>,
    pub last_cpu_usage: Mutex<f32>,
}

impl EngineMetrics {
    pub fn new() -> Self {
        Self {
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            files_processed: AtomicU64::new(0),
            reconstructions: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            verify_failures: AtomicU64::new(0),
            codec_counts: (0..CATALOG.len()).map(|_| AtomicU64::new(0)).collect(),
            latency_buckets: (0..=LATENCY_BUCKETS.len())
                .map(|_| AtomicU64::new(0))
                .collect(),
            latency_sum_us: AtomicU64::new(0),
            latency_count: AtomicU64::new(0),
            dedup_bytes_saved: AtomicU64::new(0),
            dedup_hits: AtomicU64::new(0),
            start_time: Instant::now(),
            system_monitor: Mutex::new(System::new_with_specifics(
                RefreshKind::nothing()
                    .with_cpu(CpuRefreshKind::everything())
                    .with_memory(MemoryRefreshKind::everything()),
            )),
            last_cpu_usage: Mutex::new(0.0),
        }
    }

    pub fn record_cpu_usage(&self, usage: f32) {
        if let Ok(mut last) = self.last_cpu_usage.lock() {
            *last = usage;
        }
    }

    pub fn record_latency(&self, ms: f64) {
        if !ms.is_finite() || ms < 0.0 {
            return;
        }
        self.latency_sum_us
            .fetch_add((ms * 1000.0) as u64, Ordering::Relaxed);
        self.latency_count.fetch_add(1, Ordering::Relaxed);

        let idx = LATENCY_BUCKETS
            .iter()
            .position(|&b| ms <= b)
            .unwrap_or(LATENCY_BUCKETS.len());
        self.latency_buckets[idx].fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_codec(&self, codec_id: i32) {
        if let Ok(c) = Codec::try_from(codec_id) {
            if let Some(i) = CATALOG.iter().position(|info| info.codec == c) {
                self.codec_counts[i].fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn codec_usage(&self) -> Vec<(&'static str, u64)> {
        CATALOG
            .iter()
            .enumerate()
            .map(|(i, info)| (info.name, self.codec_counts[i].load(Ordering::Relaxed)))
            .collect()
    }

    fn latency_sum_ms(&self) -> f64 {
        self.latency_sum_us.load(Ordering::Relaxed) as f64 / 1000.0
    }

    fn system_snapshot(&self) -> (f32, u64) {
        let lb_cpu = self.last_cpu_usage.lock().ok().map(|v| *v).unwrap_or(0.0);
        match self.system_monitor.lock() {
            Ok(mut sys) => {
                sys.refresh_memory();
                let cpu = if lb_cpu > 0.0 {
                    lb_cpu
                } else {
                    sys.refresh_cpu_usage();
                    sys.global_cpu_usage()
                };
                (cpu, sys.used_memory())
            }
            Err(_) => (lb_cpu, 0),
        }
    }

    pub fn to_prometheus(&self) -> String {
        let b_in = self.bytes_in.load(Ordering::Relaxed);
        let b_out = self.bytes_out.load(Ordering::Relaxed);
        let ratio = if b_in > 0 { b_out as f64 / b_in as f64 } else { 1.0 };
        let (cpu, ram) = self.system_snapshot();

        let mut out = String::with_capacity(4096);

        macro_rules! metric {
            ($name:expr, $kind:expr, $help:expr, $value:expr) => {
                out.push_str(&format!(
                    "# HELP {n} {h}\n# TYPE {n} {k}\n{n} {v}\n",
                    n = $name,
                    h = $help,
                    k = $kind,
                    v = $value
                ));
            };
        }

        metric!("syntra_bytes_in_total", "counter", "Bytes recebidos para compressão", b_in);
        metric!("syntra_essence_bytes_total", "counter", "Bytes de essência gerados", b_out);
        metric!("syntra_distillation_ratio", "gauge", "Razão essência/original (menor é melhor)", format!("{ratio:.6}"));
        metric!("syntra_items_total", "counter", "Itens comprimidos", self.files_processed.load(Ordering::Relaxed));
        metric!("syntra_reconstructions_total", "counter", "Reconstruções realizadas", self.reconstructions.load(Ordering::Relaxed));
        metric!("syntra_errors_total", "counter", "Erros de processamento", self.errors.load(Ordering::Relaxed));
        metric!("syntra_verify_failures_total", "counter", "Round-trips que não reproduziram o original", self.verify_failures.load(Ordering::Relaxed));
        metric!("syntra_dedup_hits_total", "counter", "Envios atendidos por deduplicação", self.dedup_hits.load(Ordering::Relaxed));
        metric!("syntra_dedup_bytes_saved_total", "counter", "Bytes de escrita evitados por deduplicação", self.dedup_bytes_saved.load(Ordering::Relaxed));
        metric!("syntra_uptime_seconds", "gauge", "Tempo ativo do motor", self.start_time.elapsed().as_secs());
        metric!("syntra_system_cpu_usage", "gauge", "Uso de CPU (0-100)", format!("{cpu:.2}"));
        metric!("syntra_system_memory_used_bytes", "gauge", "RAM usada em bytes", ram);

        out.push_str("# HELP syntra_codec_usage_total Itens comprimidos por codec\n");
        out.push_str("# TYPE syntra_codec_usage_total counter\n");
        for (name, count) in self.codec_usage() {
            out.push_str(&format!(
                "syntra_codec_usage_total{{codec=\"{name}\"}} {count}\n"
            ));
        }

        out.push_str("# HELP syntra_processing_latency_ms Latência de processamento em ms\n");
        out.push_str("# TYPE syntra_processing_latency_ms histogram\n");
        let mut cumulative = 0u64;
        for (i, &bound) in LATENCY_BUCKETS.iter().enumerate() {
            cumulative += self.latency_buckets[i].load(Ordering::Relaxed);
            out.push_str(&format!(
                "syntra_processing_latency_ms_bucket{{le=\"{bound}\"}} {cumulative}\n"
            ));
        }
        cumulative += self.latency_buckets[LATENCY_BUCKETS.len()].load(Ordering::Relaxed);
        out.push_str(&format!(
            "syntra_processing_latency_ms_bucket{{le=\"+Inf\"}} {cumulative}\n"
        ));
        out.push_str(&format!(
            "syntra_processing_latency_ms_sum {:.3}\n",
            self.latency_sum_ms()
        ));
        out.push_str(&format!(
            "syntra_processing_latency_ms_count {}\n",
            self.latency_count.load(Ordering::Relaxed)
        ));

        out
    }

    pub fn to_json(&self, dict_active: usize, dict_total: usize, dict_hits: u64) -> serde_json::Value {
        let b_in = self.bytes_in.load(Ordering::Relaxed);
        let b_out = self.bytes_out.load(Ordering::Relaxed);
        let savings = if b_in > 0 {
            (1.0 - b_out as f64 / b_in as f64) * 100.0
        } else {
            0.0
        };
        let (cpu, ram) = self.system_snapshot();
        let count = self.latency_count.load(Ordering::Relaxed);
        let sum_ms = self.latency_sum_ms();

        serde_json::json!({
            "uptime_seconds": self.start_time.elapsed().as_secs(),
            "throughput": {
                "bytes_in": b_in,
                "bytes_out": b_out,
                "savings_pct": round2(savings),
                "items_processed": self.files_processed.load(Ordering::Relaxed),
                "reconstructions": self.reconstructions.load(Ordering::Relaxed),
                "errors": self.errors.load(Ordering::Relaxed),
                "verify_failures": self.verify_failures.load(Ordering::Relaxed),
            },
            "deduplication": {
                "hits": self.dedup_hits.load(Ordering::Relaxed),
                "bytes_saved": self.dedup_bytes_saved.load(Ordering::Relaxed),
                "mode": "content_addressed",
            },
            "dictionaries": {
                "active": dict_active,
                "total": dict_total,
                "hits": dict_hits,
            },
            "codec_usage": self
                .codec_usage()
                .into_iter()
                .map(|(k, v)| (k.to_string(), serde_json::json!(v)))
                .collect::<serde_json::Map<_, _>>(),
            "latency_ms": {
                "sum": round2(sum_ms),
                "count": count,
                "avg": if count > 0 { round2(sum_ms / count as f64) } else { 0.0 },
            },
            "system": {
                "cpu_usage_pct": round2(cpu as f64),
                "ram_used_bytes": ram,
                "ram_used_human": format!("{:.2} GiB", ram as f64 / (1024.0 * 1024.0 * 1024.0)),
            },
        })
    }
}

impl Default for EngineMetrics {
    fn default() -> Self {
        Self::new()
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histograma_e_cumulativo_e_consistente() {
        let m = EngineMetrics::new();
        for ms in [0.2, 0.7, 3.0, 20.0, 700.0, 90_000.0] {
            m.record_latency(ms);
        }

        let prom = m.to_prometheus();

        assert!(
            prom.contains("syntra_processing_latency_ms_bucket{le=\"+Inf\"} 6"),
            "+Inf errado:\n{prom}"
        );
        assert!(prom.contains("syntra_processing_latency_ms_count 6"));
        assert!(prom.contains("syntra_processing_latency_ms_bucket{le=\"0.5\"} 1"));
        assert!(prom.contains("syntra_processing_latency_ms_bucket{le=\"1\"} 2"));

        let mut anterior = 0u64;
        for linha in prom.lines().filter(|l| l.contains("_bucket{le=")) {
            let valor: u64 = linha.rsplit(' ').next().unwrap().parse().unwrap();
            assert!(valor >= anterior, "histograma não monotônico em: {linha}");
            anterior = valor;
        }
    }

    #[test]
    fn latencia_invalida_e_ignorada() {
        let m = EngineMetrics::new();
        m.record_latency(f64::NAN);
        m.record_latency(-5.0);
        m.record_latency(f64::INFINITY);
        assert_eq!(m.latency_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn contagem_por_codec() {
        let m = EngineMetrics::new();
        m.record_codec(Codec::Zstd as i32);
        m.record_codec(Codec::Zstd as i32);
        m.record_codec(Codec::Brotli as i32);
        m.record_codec(999);

        let uso: std::collections::HashMap<_, _> = m.codec_usage().into_iter().collect();
        assert_eq!(uso["zstd"], 2);
        assert_eq!(uso["brotli"], 1);
        assert_eq!(uso["xz"], 0);
    }

    #[test]
    fn json_tem_as_secoes_esperadas() {
        let m = EngineMetrics::new();
        m.bytes_in.store(1000, Ordering::Relaxed);
        m.bytes_out.store(250, Ordering::Relaxed);
        let j = m.to_json(2, 5, 7);
        assert_eq!(j["throughput"]["savings_pct"], 75.0);
        assert_eq!(j["dictionaries"]["active"], 2);
        assert_eq!(j["dictionaries"]["total"], 5);
        assert!(j["codec_usage"]["zstd"].is_number());
    }
}
