//! # Módulo de Compressão e Descompressão
//!
//! Este módulo fornece uma interface unificada para os algoritmos de compressão
//! suportados pelo motor Syntra. Ele abstrai os detalhes de cada biblioteca,
//! permitindo que o resto do código trabalhe com uma API simples.

use crate::proto::Algorithm;
use std::io::{self, Cursor};

/// Comprime (destila) um bloco de dados usando a estratégia especificada.
pub fn compress(
    data: &[u8],
    algorithm: Algorithm,
    level: i32,
    dict: Option<&[u8]>,
) -> io::Result<Vec<u8>> {
    match algorithm {
        // Passthrough: apenas copia os dados sem comprimir
        Algorithm::Passthrough => Ok(data.to_vec()),

        // LZ4: algoritmo de compressão muito rápido
        Algorithm::Lz4 => {
            let compressed = lz4_flex::compress_prepend_size(data);
            Ok(compressed)
        }

        // Zstd: família de algoritmos com diferentes níveis
        Algorithm::ZstdFast | Algorithm::ZstdBalanced | Algorithm::ZstdMax => {
            if let Some(d) = dict {
                // Com dicionário
                let mut encoder = zstd::Encoder::with_dictionary(Vec::new(), level, d)?;
                std::io::Write::write_all(&mut encoder, data)?;
                encoder.finish()
            } else {
                // Sem dicionário
                zstd::encode_all(Cursor::new(data), level)
            }
        }

        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Estratégia de processamento inválida",
        )),
    }
}

/// Reconstrói os dados originais a partir da essência comprimida.
pub fn decompress(data: &[u8], algorithm: Algorithm, _level: i32, dict: Option<&[u8]>) -> io::Result<Vec<u8>> {
    match algorithm {
        // Passthrough: apenas copia os dados
        Algorithm::Passthrough => Ok(data.to_vec()),

        // LZ4: descomprime usando o formato com tamanho prefixado
        Algorithm::Lz4 => lz4_flex::decompress_size_prepended(data)
            .map_err(io::Error::other),

        // Zstd: família de algoritmos
        Algorithm::ZstdFast | Algorithm::ZstdBalanced | Algorithm::ZstdMax => {
            if let Some(d) = dict {
                // Com dicionário
                let mut decoder = zstd::Decoder::with_dictionary(Cursor::new(data), d)?;
                let mut restored = Vec::new();
                std::io::copy(&mut decoder, &mut restored)?;
                Ok(restored)
            } else {
                // Sem dicionário
                zstd::decode_all(Cursor::new(data))
                    .map_err(io::Error::other)
            }
        }

        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Estratégia de reconstrução inválida",
        )),
    }
}
