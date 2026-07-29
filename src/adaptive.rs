//! # Módulo de Análise Adaptativa
//!
//! Este módulo implementa a inteligência do motor Syntra. Ele analisa os dados
//! de entrada e decide automaticamente qual algoritmo de compressão usar.

use crate::proto::Algorithm;

/// Resultado da análise adaptativa de dados.
pub struct AnalysisResult {
    pub algorithm: Algorithm,
    pub zstd_level: i32,
    #[allow(dead_code)]
    pub reason: &'static str,
}

/// Analisa os dados e retorna a estratégia ótima de processamento.
pub fn analyze(data: &[u8], mime: &str) -> AnalysisResult {
    let entropy = shannon_entropy(data);

    if entropy >= 7.8 {
        return AnalysisResult {
            algorithm: Algorithm::Passthrough,
            zstd_level: 0,
            reason: "Entropia extrema — bypass de processamento",
        };
    }

    if is_structured_text(mime) {
        if data.len() > 50 * 1024 * 1024 {
            return AnalysisResult {
                algorithm: Algorithm::Lz4,
                zstd_level: 0,
                reason: "Dados volumosos — Rota LZ4",
            };
        } else if entropy < 4.5 {
            return AnalysisResult {
                algorithm: Algorithm::Lz4,
                zstd_level: 0,
                reason: "Dados altamente repetitivos — Rota LZ4",
            };
        } else {
            return AnalysisResult {
                algorithm: Algorithm::ZstdBalanced,
                zstd_level: -5,
                reason: "Dados estruturados — Zstd -5",
            };
        }
    }

    if entropy < 4.8 {
        return AnalysisResult {
            algorithm: Algorithm::ZstdBalanced,
            zstd_level: -5,
            reason: "Binário baixa entropia — Zstd -5",
        };
    }

    if entropy < 7.8 {
        return AnalysisResult {
            algorithm: Algorithm::Lz4,
            zstd_level: 0,
            reason: "Entropia industrial — Rota LZ4",
        };
    }

    AnalysisResult {
        algorithm: Algorithm::ZstdFast,
        zstd_level: -7,
        reason: "Entropia extrema — Zstd -7",
    }
}

fn is_structured_text(mime: &str) -> bool {
    mime.starts_with("text/")
        || mime.contains("json")
        || mime.contains("xml")
        || mime.contains("csv")
        || mime.contains("yaml")
        || mime.contains("sql")
}

/// Calcula a entropia Shannon dos dados.
fn shannon_entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }

    let sample = if data.len() > 1_048_576 {
        &data[..1_048_576]
    } else {
        data
    };

    let mut freq = [0u64; 256];
    for &byte in sample {
        freq[byte as usize] += 1;
    }

    let len = sample.len() as f64;
    let mut entropy = 0.0;

    for &count in &freq {
        if count > 0 {
            let p = count as f64 / len;
            entropy -= p * p.log2();
        }
    }

    entropy
}
