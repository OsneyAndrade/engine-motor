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
            let params = brotli::enc::BrotliEncoderParams {
                quality: level,
                lgwin: 24,
                size_hint: data.len(),
                ..Default::default()
            };
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

pub const MAX_DECOMPRESSED: u64 = 64 * 1024 * 1024 * 1024;
pub const XZ_MEMLIMIT: u64 = 256 * 1024 * 1024;
const READ_CHUNK: usize = 128 * 1024;
const CAPACITY_PER_INPUT_BYTE: usize = 64;
const LZ4_MAX_EXPANSION: usize = 255;
const CAPACITY_FLOOR: usize = 1024 * 1024;

#[derive(Debug)]
pub struct DecompressedTooLarge {
    pub limit: u64,
}

impl std::fmt::Display for DecompressedTooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a saída descomprimida excede o teto de {} bytes", self.limit)
    }
}

impl std::error::Error for DecompressedTooLarge {}

pub fn too_large(limit: u64) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, DecompressedTooLarge { limit })
}

pub fn too_large_limit(e: &io::Error) -> Option<u64> {
    e.get_ref()
        .and_then(|inner| inner.downcast_ref::<DecompressedTooLarge>())
        .map(|t| t.limit)
}

#[cfg(test)]
pub fn decode(
    data: &[u8],
    codec: Codec,
    dict: Option<&[u8]>,
    expected_len: Option<u64>,
) -> io::Result<Vec<u8>> {
    decode_limited(data, codec, dict, expected_len, MAX_DECOMPRESSED)
}

pub fn decode_limited(
    data: &[u8],
    codec: Codec,
    dict: Option<&[u8]>,
    expected_len: Option<u64>,
    max_out: u64,
) -> io::Result<Vec<u8>> {
    let limit = max_out.min(MAX_DECOMPRESSED);
    let cap = initial_capacity(expected_len, limit, data.len());

    match codec {
        Codec::Stored => {
            if data.len() as u64 > limit {
                return Err(too_large(limit));
            }
            Ok(data.to_vec())
        }

        Codec::Lz4 => {
            let (declared, body) =
                lz4_flex::block::uncompressed_size(data).map_err(io::Error::other)?;
            if declared as u64 > limit {
                return Err(too_large(limit));
            }
            let plausible = body
                .len()
                .saturating_mul(LZ4_MAX_EXPANSION)
                .saturating_add(CAPACITY_FLOOR);
            if declared > plausible {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "prefixo LZ4 maior do que o bloco comprimido pode produzir",
                ));
            }
            lz4_flex::block::decompress(body, declared).map_err(io::Error::other)
        }

        Codec::Zstd => {
            if let Ok(Some(declared)) = zstd::zstd_safe::get_frame_content_size(data) {
                if declared > limit {
                    return Err(too_large(limit));
                }
            }
            let window_log = zstd_window_log_max(limit);
            check_zstd_windows(data, window_log)?;
            let mut out = Vec::with_capacity(cap);
            match dict {
                Some(d) if !d.is_empty() => {
                    let mut decoder = zstd::Decoder::with_dictionary(Cursor::new(data), d)?;
                    decoder.window_log_max(window_log)?;
                    read_capped(&mut decoder, &mut out, limit)?;
                }
                _ => {
                    let mut decoder = zstd::Decoder::new(Cursor::new(data))?;
                    decoder.window_log_max(window_log)?;
                    read_capped(&mut decoder, &mut out, limit)?;
                }
            }
            Ok(out)
        }

        Codec::Brotli => {
            let mut out = Vec::with_capacity(cap);
            let mut input = Cursor::new(data);
            let mut sink = CappedSink {
                out: &mut out,
                limit: usize::try_from(limit).unwrap_or(usize::MAX),
                exceeded: false,
            };
            let result = brotli::BrotliDecompress(&mut input, &mut sink);
            if sink.exceeded {
                return Err(too_large(limit));
            }
            result?;
            Ok(out)
        }

        Codec::Xz => {
            let mut out = Vec::with_capacity(cap);
            let stream = xz2::stream::Stream::new_stream_decoder(XZ_MEMLIMIT, 0)
                .map_err(io::Error::other)?;
            let mut decoder = xz2::read::XzDecoder::new_stream(Cursor::new(data), stream);
            read_capped(&mut decoder, &mut out, limit)?;
            Ok(out)
        }

        Codec::Bzip2 => {
            let mut out = Vec::with_capacity(cap);
            let mut decoder = bzip2::read::BzDecoder::new(Cursor::new(data));
            read_capped(&mut decoder, &mut out, limit)?;
            Ok(out)
        }

        Codec::Deflate => {
            let mut out = Vec::with_capacity(cap);
            let mut decoder = flate2::read::DeflateDecoder::new(Cursor::new(data));
            read_capped(&mut decoder, &mut out, limit)?;
            Ok(out)
        }

        Codec::Unspecified => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "codec não especificado no envelope",
        )),
    }
}

fn initial_capacity(expected_len: Option<u64>, limit: u64, payload_len: usize) -> usize {
    let plausible = payload_len
        .saturating_mul(CAPACITY_PER_INPUT_BYTE)
        .saturating_add(CAPACITY_FLOOR);
    let wanted = expected_len.unwrap_or(0).min(limit);
    usize::try_from(wanted).unwrap_or(usize::MAX).min(plausible)
}

pub const ZSTD_WINDOW_LOG_FLOOR: u32 = 17;
pub const ZSTD_WINDOW_LOG_CEILING: u32 = 31;
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

#[derive(Debug)]
pub struct ZstdWindowTooLarge {
    pub window: u64,
    pub max: u64,
}

impl std::fmt::Display for ZstdWindowTooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "a janela do quadro zstd ({} bytes) excede o máximo de {} bytes permitido para este envelope",
            self.window, self.max
        )
    }
}

impl std::error::Error for ZstdWindowTooLarge {}

#[cfg(test)]
pub fn zstd_window_too_large(e: &io::Error) -> Option<(u64, u64)> {
    e.get_ref()
        .and_then(|inner| inner.downcast_ref::<ZstdWindowTooLarge>())
        .map(|w| (w.window, w.max))
}

pub fn zstd_window_log_max(limit: u64) -> u32 {
    let bits = if limit <= 1 {
        0
    } else {
        u64::BITS - (limit - 1).leading_zeros()
    };
    bits.clamp(ZSTD_WINDOW_LOG_FLOOR, ZSTD_WINDOW_LOG_CEILING)
}

pub fn zstd_frame_window(frame: &[u8]) -> Option<u64> {
    if frame.len() < 6 || frame[..4] != ZSTD_MAGIC {
        return None;
    }
    let descriptor = frame[4];
    if descriptor & 0x20 == 0 {
        let byte = frame[5];
        let base = 1u64 << (10 + u32::from(byte >> 3));
        return Some(base + (base / 8) * u64::from(byte & 7));
    }
    let dict_len = match descriptor & 3 {
        0 => 0,
        1 => 1,
        2 => 2,
        _ => 4,
    };
    let size_len = match descriptor >> 6 {
        0 => 1,
        1 => 2,
        2 => 4,
        _ => 8,
    };
    let start = 5 + dict_len;
    let raw = frame.get(start..start + size_len)?;
    let mut size = 0u64;
    for (i, byte) in raw.iter().enumerate() {
        size |= u64::from(*byte) << (8 * i);
    }
    if size_len == 2 {
        size += 256;
    }
    Some(size)
}

fn check_zstd_windows(data: &[u8], window_log: u32) -> io::Result<()> {
    let max = 1u64 << window_log;
    let mut rest = data;
    while !rest.is_empty() {
        if let Some(window) = zstd_frame_window(rest) {
            if window > max {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    ZstdWindowTooLarge { window, max },
                ));
            }
        }
        match zstd::zstd_safe::find_frame_compressed_size(rest) {
            Ok(len) if len > 0 && len <= rest.len() => rest = &rest[len..],
            _ => break,
        }
    }
    Ok(())
}

fn grow_within(out: &mut Vec<u8>, extra: usize, limit: usize) {
    if out.capacity() - out.len() >= extra {
        return;
    }
    let needed = out.len() + extra;
    let target = out.capacity().saturating_mul(2).max(needed).min(limit.max(needed));
    out.reserve_exact(target - out.len());
}

struct CappedSink<'a> {
    out: &'a mut Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl Write for CappedSink<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.out.len().saturating_add(buf.len()) > self.limit {
            self.exceeded = true;
            return Err(io::Error::other("teto de descompressão excedido"));
        }
        grow_within(self.out, buf.len(), self.limit);
        self.out.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn read_capped<R: Read>(reader: &mut R, out: &mut Vec<u8>, limit: u64) -> io::Result<()> {
    let limit_bytes = usize::try_from(limit).unwrap_or(usize::MAX);
    let mut chunk = vec![0u8; READ_CHUNK];
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if out.len().saturating_add(n) > limit_bytes {
            return Err(too_large(limit));
        }
        grow_within(out, n, limit_bytes);
        out.extend_from_slice(&chunk[..n]);
    }
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
