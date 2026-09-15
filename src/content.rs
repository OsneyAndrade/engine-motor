use crate::transform;

static MIME_EXTENSIONS: phf::Map<&'static str, &'static str> = phf::phf_map! {
    "json" => "application/json",
    "jsonl" => "application/x-ndjson",
    "ndjson" => "application/x-ndjson",
    "csv" => "text/csv",
    "tsv" => "text/tab-separated-values",
    "log" => "text/plain",
    "txt" => "text/plain",
    "md" => "text/markdown",
    "xml" => "application/xml",
    "html" => "text/html",
    "htm" => "text/html",
    "css" => "text/css",
    "js" => "text/javascript",
    "ts" => "text/typescript",
    "yaml" => "application/yaml",
    "yml" => "application/yaml",
    "toml" => "application/toml",
    "sql" => "application/sql",
    "rs" => "text/x-rust",
    "py" => "text/x-python",
    "go" => "text/x-go",
    "java" => "text/x-java",
    "c" => "text/x-c",
    "h" => "text/x-c",
    "cpp" => "text/x-c++",
    "svg" => "image/svg+xml",
    "parquet" => "application/vnd.apache.parquet",
    "orc" => "application/vnd.apache.orc",
    "avro" => "application/vnd.apache.avro",
    "arrow" => "application/vnd.apache.arrow",
    "protobuf" => "application/x-protobuf",
    "pb" => "application/x-protobuf",
    "npy" => "application/x-numpy",
    "bin" => "application/octet-stream",
    "dat" => "application/octet-stream",
    "pdf" => "application/pdf",
    "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "png" => "image/png",
    "jpg" => "image/jpeg",
    "jpeg" => "image/jpeg",
    "gif" => "image/gif",
    "webp" => "image/webp",
    "bmp" => "image/bmp",
    "tiff" => "image/tiff",
    "tif" => "image/tiff",
    "avif" => "image/avif",
    "heic" => "image/heic",
    "heif" => "image/heif",
    "wav" => "audio/wav",
    "flac" => "audio/flac",
    "mp3" => "audio/mpeg",
    "aac" => "audio/aac",
    "opus" => "audio/opus",
    "mp4" => "video/mp4",
    "mkv" => "video/x-matroska",
    "webm" => "video/webm",
    "zip" => "application/zip",
    "gz" => "application/gzip",
    "bz2" => "application/x-bzip2",
    "xz" => "application/x-xz",
    "zst" => "application/zstd",
    "7z" => "application/x-7z-compressed",
    "rar" => "application/vnd.rar",
    "syntra" => "application/x-syntra-essence",
};

pub fn detect_mime(data: &[u8], filename: &str) -> String {
    if let Some((_, ext)) = filename.rsplit_once('.') {
        let ext = ext.to_ascii_lowercase();
        if let Some(&mime) = MIME_EXTENSIONS.get(ext.as_str()) {
            return mime.to_string();
        }
    }
    if !data.is_empty() {
        if let Some(kind) = infer::get(data) {
            return kind.mime_type().to_string();
        }
    }
    if !data.is_empty() && printable_ratio(sample_of(data)) > 0.90 {
        return "text/plain".to_string();
    }
    "application/octet-stream".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentClass {
    Tabular,
    StructuredText,
    PlainText,
    NumericBinary,
    Binary,
    PreCompressed,
    Media,
    Incompressible,
}

impl ContentClass {
    pub fn as_str(self) -> &'static str {
        match self {
            ContentClass::Tabular => "tabular",
            ContentClass::StructuredText => "structured_text",
            ContentClass::PlainText => "plain_text",
            ContentClass::NumericBinary => "numeric_binary",
            ContentClass::Binary => "binary",
            ContentClass::PreCompressed => "pre_compressed",
            ContentClass::Media => "media",
            ContentClass::Incompressible => "incompressible",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContentProfile {
    pub mime: String,
    pub class: ContentClass,
    pub size: u64,
    pub entropy: f64,
    pub printable: f64,
    pub tabular: Option<(u8, usize)>,
    pub stride: Option<u32>,
    pub dictionary_key: String,
}

impl ContentProfile {
    #[cfg(test)]
    pub fn is_text(&self) -> bool {
        matches!(
            self.class,
            ContentClass::Tabular | ContentClass::StructuredText | ContentClass::PlainText
        )
    }
}

pub fn classify(data: &[u8], filename: &str, mime_hint: Option<&str>) -> ContentProfile {
    let mime = match mime_hint {
        Some(m) if !m.is_empty() && m != "application/octet-stream" => m.to_string(),
        _ => detect_mime(data, filename),
    };

    let sample = sample_of(data);
    let entropy = shannon_entropy(sample);
    let printable = printable_ratio(sample);

    let stride = detect_stride(sample);
    let mime_is_text = mime.starts_with("text/") || is_structured_text_mime(&mime);
    let mut tabular = None;

    let class = if data.is_empty() {
        ContentClass::Binary
    } else if is_pre_compressed_mime(&mime) || has_compressed_magic(data) {
        ContentClass::PreCompressed
    } else if is_media_mime(&mime) {
        ContentClass::Media
    } else if entropy >= 7.94 {
        ContentClass::Incompressible
    } else if stride.is_some() && !mime_is_text {
        ContentClass::NumericBinary
    } else if printable > 0.88 {
        if let Some(t) = transform::csv_detect(data) {
            tabular = Some(t);
            ContentClass::Tabular
        } else if mime_is_text || looks_structured(sample) {
            ContentClass::StructuredText
        } else {
            ContentClass::PlainText
        }
    } else if stride.is_some() {
        ContentClass::NumericBinary
    } else {
        ContentClass::Binary
    };

    let stride = if class == ContentClass::NumericBinary { stride } else { None };

    let dictionary_key = dictionary_key_for(&mime, class);

    ContentProfile {
        mime,
        class,
        size: data.len() as u64,
        entropy,
        printable,
        tabular,
        stride,
        dictionary_key,
    }
}

fn dictionary_key_for(mime: &str, class: ContentClass) -> String {
    match class {
        ContentClass::Incompressible | ContentClass::Media | ContentClass::PreCompressed => {
            String::new()
        }
        _ => mime.to_string(),
    }
}

const SAMPLE_BUDGET: usize = 3 * 1024 * 1024;

fn sample_of(data: &[u8]) -> &[u8] {
    if data.len() <= SAMPLE_BUDGET {
        return data;
    }
    let start = (data.len() - SAMPLE_BUDGET) / 2;
    &data[start..start + SAMPLE_BUDGET]
}

pub fn shannon_entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut freq = [0u64; 256];
    for &b in data {
        freq[b as usize] += 1;
    }
    let len = data.len() as f64;
    let mut h = 0.0;
    for &c in freq.iter() {
        if c > 0 {
            let p = c as f64 / len;
            h -= p * p.log2();
        }
    }
    h
}

fn printable_ratio(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let ok = data
        .iter()
        .filter(|&&b| matches!(b, 0x09 | 0x0A | 0x0D | 0x20..=0x7E) || b >= 0x80)
        .count();
    ok as f64 / data.len() as f64
}

fn looks_structured(sample: &[u8]) -> bool {
    let head = &sample[..sample.len().min(64 * 1024)];
    if head.is_empty() {
        return false;
    }
    let markers = head
        .iter()
        .filter(|&&b| matches!(b, b'{' | b'}' | b'[' | b']' | b'<' | b'>' | b':' | b'"' | b'='))
        .count();
    markers as f64 / head.len() as f64 > 0.04
}

fn is_structured_text_mime(mime: &str) -> bool {
    mime.contains("json")
        || mime.contains("xml")
        || mime.contains("yaml")
        || mime.contains("toml")
        || mime.contains("html")
        || mime.contains("sql")
        || mime.contains("csv")
        || mime.contains("tab-separated")
}

fn is_pre_compressed_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/zip"
            | "application/gzip"
            | "application/x-bzip2"
            | "application/x-xz"
            | "application/zstd"
            | "application/x-7z-compressed"
            | "application/vnd.rar"
            | "application/x-syntra-essence"
    ) || mime.starts_with("application/vnd.openxmlformats")
        || mime.starts_with("application/vnd.oasis.opendocument")
}

fn is_media_mime(mime: &str) -> bool {
    mime.starts_with("video/")
        || mime.starts_with("audio/")
        || matches!(
            mime,
            "image/jpeg"
                | "image/png"
                | "image/gif"
                | "image/webp"
                | "image/avif"
                | "image/heic"
                | "image/heif"
        )
}

fn has_compressed_magic(data: &[u8]) -> bool {
    const MAGICS: &[&[u8]] = &[
        b"PK\x03\x04",
        b"\x1f\x8b",
        b"BZh",
        b"\xfd7zXZ\x00",
        b"\x28\xb5\x2f\xfd",
        b"7z\xbc\xaf\x27\x1c",
        b"Rar!\x1a\x07",
        b"\x04\x22\x4d\x18",
    ];
    MAGICS.iter().any(|m| data.starts_with(m))
}

fn detect_stride(sample: &[u8]) -> Option<u32> {
    let probe = &sample[..sample.len().min(256 * 1024)];
    if probe.len() < 512 {
        return None;
    }

    let base = match_rate(probe, 1);
    let mut best: Option<(u32, f64)> = None;

    for w in [2u32, 4, 8] {
        let r = match_rate(probe, w as usize);
        if r > 0.25 && r > base * 1.6 {
            if best.map(|(_, br)| r > br).unwrap_or(true) {
                best = Some((w, r));
            }
        }
    }

    best.map(|(w, _)| w)
}

fn match_rate(data: &[u8], lag: usize) -> f64 {
    if data.len() <= lag {
        return 0.0;
    }
    let hits = data[lag..]
        .iter()
        .zip(data.iter())
        .filter(|(a, b)| a == b)
        .count();
    hits as f64 / (data.len() - lag) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifica_csv_como_tabular() {
        let mut s = String::from("id,nome,uf,valor\n");
        for i in 0..400 {
            s.push_str(&format!("{},cliente{},SP,{}\n", i, i, i * 3));
        }
        let p = classify(s.as_bytes(), "vendas.csv", None);
        assert_eq!(p.class, ContentClass::Tabular);
        assert!(p.tabular.is_some());
        assert_eq!(p.mime, "text/csv");
    }

    #[test]
    fn classifica_json_como_estruturado() {
        let mut s = String::from("[");
        for i in 0..400 {
            s.push_str(&format!(r#"{{"id":{},"ativo":true,"tag":"x"}},"#, i));
        }
        s.push(']');
        let p = classify(s.as_bytes(), "dados.json", None);
        assert_eq!(p.class, ContentClass::StructuredText);
    }

    #[test]
    fn classifica_aleatorio_como_incompressivel() {
        let mut x = 0x9E3779B9u32;
        let random: Vec<u8> = (0..200_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x >> 8) as u8
            })
            .collect();
        let p = classify(&random, "chave.bin", None);
        assert_eq!(p.class, ContentClass::Incompressible, "entropia={}", p.entropy);
        assert!(p.dictionary_key.is_empty());
    }

    #[test]
    fn detecta_container_ja_comprimido_por_magic() {
        let mut zip = b"PK\x03\x04".to_vec();
        zip.extend_from_slice(&[0u8; 1000]);
        let p = classify(&zip, "pacote.desconhecido", None);
        assert_eq!(p.class, ContentClass::PreCompressed);
    }

    #[test]
    fn detecta_stride_em_array_de_float() {
        let mut data = Vec::new();
        for i in 0..20_000u32 {
            data.extend_from_slice(&(i as f32 * 0.001).to_le_bytes());
        }
        let p = classify(&data, "serie.dat", None);
        assert_eq!(p.class, ContentClass::NumericBinary);
        assert_eq!(p.stride, Some(4));
    }

    #[test]
    fn nao_detecta_stride_em_texto_binarizado_sem_padrao() {
        let mut x = 7u32;
        let noisy: Vec<u8> = (0..100_000)
            .map(|_| {
                x = x.wrapping_mul(1103515245).wrapping_add(12345);
                ((x >> 16) % 40) as u8
            })
            .collect();
        let p = classify(&noisy, "ruido.dat", None);
        assert_eq!(p.stride, None);
        assert_eq!(p.class, ContentClass::Binary);
    }

    #[test]
    fn mime_por_extensao_tem_prioridade_sobre_magic() {
        let png = b"\x89PNG\r\n\x1a\n".to_vec();
        assert_eq!(detect_mime(&png, "x.png"), "image/png");
        assert_eq!(detect_mime(&png, "x.desconhecido"), "image/png");
    }

    #[test]
    fn texto_sem_extensao_nao_vira_numerico() {
        let mut s = String::new();
        for i in 0..800 {
            s.push_str(&format!(
                "2026-09-15T10:{:02}:{:02}Z INFO request_id=abc{} status=200 latency_ms={}\n",
                i % 60, i % 60, i, i % 97
            ));
        }
        let p = classify(s.as_bytes(), "aplicacao-log", None);
        assert!(p.is_text(), "classificou como {:?}", p.class);
        assert_eq!(p.stride, None);
    }

    #[test]
    fn csv_ganha_de_periodicidade() {
        let mut s = String::from("aa,bb\n");
        for _ in 0..500 {
            s.push_str("1234,5678\n");
        }
        let p = classify(s.as_bytes(), "fixo.csv", None);
        assert_eq!(p.class, ContentClass::Tabular);
    }

    #[test]
    fn entropia_de_vazio_e_zero() {
        assert_eq!(shannon_entropy(&[]), 0.0);
        let p = classify(&[], "vazio.bin", None);
        assert_eq!(p.size, 0);
    }
}
