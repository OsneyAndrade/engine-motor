use std::io::{self, Cursor, Read, Write};

pub use crate::proto::Codec;

#[derive(Debug, Clone, Copy)]
pub struct CodecInfo {
    pub codec: Codec,
    pub name: &'static str,
    pub min_level: i32,
    pub max_level: i32,
    pub default_level: i32,
    pub supports_dictionary: bool,
    pub family: &'static str,
    pub notes: &'static str,
}

pub const CATALOG: &[CodecInfo] = &[
    CodecInfo {
        codec: Codec::Stored,
        name: "stored",
        min_level: 0,
        max_level: 0,
        default_level: 0,
        supports_dictionary: false,
        family: "passthrough",
        notes: "Sem compressão. Usado quando nenhum codec consegue reduzir o dado.",
    },
    CodecInfo {
        codec: Codec::Lz4,
        name: "lz4",
        min_level: 0,
        max_level: 0,
        default_level: 0,
        supports_dictionary: false,
        family: "lz77",
        notes: "Latência mínima, densidade baixa. Para hot path e dados volumosos.",
    },
    CodecInfo {
        codec: Codec::Deflate,
        name: "deflate",
        min_level: 0,
        max_level: 9,
        default_level: 6,
        supports_dictionary: false,
        family: "lz77+huffman",
        notes: "Compatível com zlib/gzip/PNG/PDF FlateDecode.",
    },
    CodecInfo {
        codec: Codec::Zstd,
        name: "zstd",
        min_level: -7,
        max_level: 22,
        default_level: 10,
        supports_dictionary: true,
        family: "lz77+fse",
        notes: "Melhor relação densidade/velocidade. Aceita dicionário treinado.",
    },
    CodecInfo {
        codec: Codec::Bzip2,
        name: "bzip2",
        min_level: 1,
        max_level: 9,
        default_level: 9,
        supports_dictionary: false,
        family: "bwt+mtf+huffman",
        notes: "BWT. Forte em texto muito repetitivo, lento na reconstrução.",
    },
    CodecInfo {
        codec: Codec::Brotli,
        name: "brotli",
        min_level: 0,
        max_level: 11,
        default_level: 11,
        supports_dictionary: false,
        family: "lz77+huffman+context",
        notes: "Dicionário estático de texto/web embutido. Excelente para texto.",
    },
    CodecInfo {
        codec: Codec::Xz,
        name: "xz",
        min_level: 0,
        max_level: 9,
        default_level: 6,
        supports_dictionary: false,
        family: "lzma2",
        notes: "Densidade máxima genérica, custo alto de CPU e memória.",
    },
];

pub fn info(codec: Codec) -> Option<&'static CodecInfo> {
    CATALOG.iter().find(|c| c.codec == codec)
}

pub fn name(codec: Codec) -> &'static str {
    info(codec).map(|c| c.name).unwrap_or("unspecified")
}

pub fn from_name(s: &str) -> Option<Codec> {
    let key = s.trim().to_ascii_lowercase();
    match key.as_str() {
        "none" | "passthrough" | "raw" => Some(Codec::Stored),
        "ultra_fast" => Some(Codec::Lz4),
        "zlib" | "gzip" | "flate" => Some(Codec::Deflate),
        "lzma" | "lzma2" => Some(Codec::Xz),
        "bz2" => Some(Codec::Bzip2),
        _ => CATALOG
            .iter()
            .find(|c| c.name == key)
            .map(|c| c.codec),
    }
}

pub fn supports_dictionary(codec: Codec) -> bool {
    info(codec).map(|c| c.supports_dictionary).unwrap_or(false)
}

pub fn default_level(codec: Codec) -> i32 {
    info(codec).map(|c| c.default_level).unwrap_or(0)
}

pub fn clamp_level(codec: Codec, level: i32) -> i32 {
    match info(codec) {
        Some(c) => level.clamp(c.min_level, c.max_level),
        None => 0,
    }
}

pub fn encode(data: &[u8], codec: Codec, level: i32, dict: Option<&[u8]>) -> io::Result<Vec<u8>> {
    let level = clamp_level(codec, level);

    match codec {
        Codec::Stored => Ok(data.to_vec()),

        Codec::Lz4 => Ok(lz4_flex::compress_prepend_size(data)),

        Codec::Zstd => {
            let mut encoder = match dict {
                Some(d) if !d.is_empty() => {
                    zstd::Encoder::with_dictionary(Vec::with_capacity(data.len() / 3 + 64), level, d)?
                }
                _ => zstd::Encoder::new(Vec::with_capacity(data.len() / 3 + 64), level)?,
            };
            if level >= 10 && data.len() > 1 << 20 {
                let _ = encoder.long_distance_matching(true);
            }
            let _ = encoder.include_contentsize(true);
            encoder.set_pledged_src_size(Some(data.len() as u64))?;
            encoder.write_all(data)?;
            encoder.finish()
        }

        Codec::Brotli => {
            let mut params = brotli::enc::BrotliEncoderParams::default();
            params.quality = level;
            params.lgwin = 24;
            params.size_hint = data.len();
            let mut out = Vec::with_capacity(data.len() / 3 + 64);
            let mut input = Cursor::new(data);
            brotli::BrotliCompress(&mut input, &mut out, &params)?;
            Ok(out)
        }

        Codec::Xz => {
            let mut enc = xz2::write::XzEncoder::new(
                Vec::with_capacity(data.len() / 3 + 64),
                level as u32,
            );
            enc.write_all(data)?;
            enc.finish()
        }

        Codec::Bzip2 => {
            let mut enc = bzip2::write::BzEncoder::new(
                Vec::with_capacity(data.len() / 3 + 64),
                bzip2::Compression::new(level as u32),
            );
            enc.write_all(data)?;
            enc.finish()
        }

        Codec::Deflate => {
            let mut enc = flate2::write::DeflateEncoder::new(
                Vec::with_capacity(data.len() / 3 + 64),
                flate2::Compression::new(level as u32),
            );
            enc.write_all(data)?;
            enc.finish()
        }

        Codec::Unspecified => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "codec não especificado",
        )),
    }
}

const MAX_DECOMPRESSED: u64 = 64 * 1024 * 1024 * 1024;

pub fn decode(
    data: &[u8],
    codec: Codec,
    dict: Option<&[u8]>,
    expected_len: Option<u64>,
) -> io::Result<Vec<u8>> {
    let cap = expected_len
        .filter(|n| *n <= MAX_DECOMPRESSED)
        .unwrap_or(0) as usize;

    match codec {
        Codec::Stored => Ok(data.to_vec()),

        Codec::Lz4 => lz4_flex::decompress_size_prepended(data).map_err(io::Error::other),

        Codec::Zstd => {
            let mut out = Vec::with_capacity(cap);
            match dict {
                Some(d) if !d.is_empty() => {
                    let mut decoder = zstd::Decoder::with_dictionary(Cursor::new(data), d)?;
                    decoder.window_log_max(31)?;
                    read_limited(&mut decoder, &mut out)?;
                }
                _ => {
                    let mut decoder = zstd::Decoder::new(Cursor::new(data))?;
                    decoder.window_log_max(31)?;
                    read_limited(&mut decoder, &mut out)?;
                }
            }
            Ok(out)
        }

        Codec::Brotli => {
            let mut out = Vec::with_capacity(cap);
            let mut input = Cursor::new(data);
            brotli::BrotliDecompress(&mut input, &mut out)?;
            Ok(out)
        }

        Codec::Xz => {
            let mut out = Vec::with_capacity(cap);
            let mut decoder = xz2::read::XzDecoder::new(Cursor::new(data));
            read_limited(&mut decoder, &mut out)?;
            Ok(out)
        }

        Codec::Bzip2 => {
            let mut out = Vec::with_capacity(cap);
            let mut decoder = bzip2::read::BzDecoder::new(Cursor::new(data));
            read_limited(&mut decoder, &mut out)?;
            Ok(out)
        }

        Codec::Deflate => {
            let mut out = Vec::with_capacity(cap);
            let mut decoder = flate2::read::DeflateDecoder::new(Cursor::new(data));
            read_limited(&mut decoder, &mut out)?;
            Ok(out)
        }

        Codec::Unspecified => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "codec não especificado no envelope",
        )),
    }
}

fn read_limited<R: Read>(reader: &mut R, out: &mut Vec<u8>) -> io::Result<()> {
    let mut limited = reader.take(MAX_DECOMPRESSED + 1);
    limited.read_to_end(out)?;
    if out.len() as u64 > MAX_DECOMPRESSED {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "payload excede o limite de descompressão",
        ));
    }
    Ok(())
}

use crate::proto::Algorithm;

pub fn from_legacy(algorithm: Algorithm, level: i32) -> (Codec, i32) {
    match algorithm {
        Algorithm::Passthrough => (Codec::Stored, 0),
        Algorithm::Lz4 => (Codec::Lz4, 0),
        Algorithm::ZstdFast => (Codec::Zstd, level),
        Algorithm::ZstdBalanced => (Codec::Zstd, level),
        Algorithm::ZstdMax => (Codec::Zstd, level),
        Algorithm::Auto => (Codec::Zstd, level),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let mut v = Vec::new();
        for i in 0..20_000u32 {
            v.extend_from_slice(format!("linha {} valor={} status=ok\n", i, i % 97).as_bytes());
        }
        v
    }

    #[test]
    fn roundtrip_todos_os_codecs() {
        let data = sample();
        for c in CATALOG {
            let enc = encode(&data, c.codec, c.default_level, None)
                .unwrap_or_else(|e| panic!("encode {} falhou: {e}", c.name));
            let dec = decode(&enc, c.codec, None, Some(data.len() as u64))
                .unwrap_or_else(|e| panic!("decode {} falhou: {e}", c.name));
            assert_eq!(dec, data, "roundtrip divergente em {}", c.name);
        }
    }

    #[test]
    fn roundtrip_vazio() {
        for c in CATALOG {
            let enc = encode(&[], c.codec, c.default_level, None).unwrap();
            let dec = decode(&enc, c.codec, None, Some(0)).unwrap();
            assert!(dec.is_empty(), "codec {} não preservou entrada vazia", c.name);
        }
    }

    #[test]
    fn roundtrip_zstd_com_dicionario() {
        let data = sample();
        let samples: Vec<&[u8]> = (0..16).map(|_| &data[..4096]).collect();
        let dict = zstd::dict::from_samples(&samples, 16 * 1024).unwrap();
        let enc = encode(&data, Codec::Zstd, 10, Some(&dict)).unwrap();
        let dec = decode(&enc, Codec::Zstd, Some(&dict), Some(data.len() as u64)).unwrap();
        assert_eq!(dec, data);
    }

    #[test]
    fn niveis_fora_da_faixa_sao_clampados() {
        assert_eq!(clamp_level(Codec::Zstd, 999), 22);
        assert_eq!(clamp_level(Codec::Zstd, -999), -7);
        assert_eq!(clamp_level(Codec::Bzip2, 0), 1);
        let out = encode(b"abc", Codec::Bzip2, 0, None).unwrap();
        assert_eq!(decode(&out, Codec::Bzip2, None, Some(3)).unwrap(), b"abc");
    }

    #[test]
    fn nomes_e_aliases() {
        assert_eq!(from_name("ZSTD"), Some(Codec::Zstd));
        assert_eq!(from_name("passthrough"), Some(Codec::Stored));
        assert_eq!(from_name("lzma2"), Some(Codec::Xz));
        assert_eq!(from_name("inexistente"), None);
    }
}
