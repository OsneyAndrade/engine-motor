use std::fmt;
use std::sync::Arc;

use prost::Message;

use crate::codec::{self, Codec};
use crate::content::ContentProfile;
use crate::dict_store::Dictionary;
use crate::planner::{self, Planned};
use crate::proto::{Algorithm, DictionaryRef, FileMetrics, ProcessedFile};

pub const CONTAINER_VERSION: i32 = 3;

#[derive(Debug)]
pub enum ContainerError {
    Decode(String),
    Malformed(String),
    ChecksumMismatch { esperado: u64, obtido: u64 },
    MissingDictionary { id: String },
    DictionaryMismatch { id: String },
    Restore(String),
    FidelityMismatch { esperado: String, obtido: String },
}

impl fmt::Display for ContainerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContainerError::Decode(m) => write!(f, "envelope inválido: {m}"),
            ContainerError::Malformed(m) => write!(f, "envelope inconsistente: {m}"),
            ContainerError::ChecksumMismatch { esperado, obtido } => write!(
                f,
                "checksum do payload não confere (esperado {esperado:#018x}, obtido {obtido:#018x}); \
                 a essência provavelmente está corrompida"
            ),
            ContainerError::MissingDictionary { id } => write!(
                f,
                "dicionário '{id}' necessário para reconstruir não está disponível neste nó"
            ),
            ContainerError::DictionaryMismatch { id } => write!(
                f,
                "dicionário '{id}' encontrado não corresponde ao usado na compressão"
            ),
            ContainerError::Restore(m) => write!(f, "falha na reconstrução: {m}"),
            ContainerError::FidelityMismatch { esperado, obtido } => write!(
                f,
                "reconstrução não é bit-a-bit idêntica ao original \
                 (hash esperado {esperado}, obtido {obtido})"
            ),
        }
    }
}

impl std::error::Error for ContainerError {}

impl ContainerError {
    pub fn code(&self) -> &'static str {
        match self {
            ContainerError::Decode(_) => "envelope_invalid",
            ContainerError::Malformed(_) => "envelope_malformed",
            ContainerError::ChecksumMismatch { .. } => "envelope_corrupted",
            ContainerError::MissingDictionary { .. } => "dictionary_unavailable",
            ContainerError::DictionaryMismatch { .. } => "dictionary_mismatch",
            ContainerError::Restore(_) => "restore_failed",
            ContainerError::FidelityMismatch { .. } => "fidelity_mismatch",
        }
    }
}

pub trait DictionaryResolver {
    fn resolve(&self, id: &str) -> Option<Arc<Dictionary>>;
}

impl DictionaryResolver for crate::dict_store::DictionaryStore {
    fn resolve(&self, id: &str) -> Option<Arc<Dictionary>> {
        crate::dict_store::DictionaryStore::resolve(self, id)
    }
}

pub struct NoDictionaries;

impl DictionaryResolver for NoDictionaries {
    fn resolve(&self, _id: &str) -> Option<Arc<Dictionary>> {
        None
    }
}

pub struct Sealed {
    pub envelope: ProcessedFile,
    pub bytes: Vec<u8>,
}

#[allow(clippy::too_many_arguments)]
pub fn build(
    planned: Planned,
    profile: &ContentProfile,
    original_name: &str,
    file_hash: &[u8; 32],
    dictionary: Option<&Dictionary>,
    duration_ms: f64,
    verified: bool,
    reconstruct_ms: f64,
) -> ProcessedFile {
    let original_size = profile.size;
    let payload = planned.payload;
    let processed_size = payload.len() as u64;

    let savings_pct = if original_size > 0 {
        (1.0 - processed_size as f64 / original_size as f64) * 100.0
    } else {
        0.0
    };

    let dictionary_ref = if planned.plan.use_dictionary {
        dictionary.map(|d| DictionaryRef {
            id: d.id.clone(),
            digest: d.digest.to_vec(),
            size: d.size(),
        })
    } else {
        None
    };

    let throughput_mbps = if duration_ms > 0.0 {
        (original_size as f64 / (1024.0 * 1024.0)) / (duration_ms / 1000.0)
    } else {
        0.0
    };

    let now = now_millis();

    let mut envelope = ProcessedFile {
        version: CONTAINER_VERSION,
        original_name: original_name.to_string(),
        original_names: vec![original_name.to_string()],
        original_mime: profile.mime.clone(),
        original_size,
        processed_size,
        savings_pct: savings_pct as f32,

        algorithm: legacy_algorithm(planned.plan.codec) as i32,
        compression_level: planned.plan.level,
        dictionary_id: dictionary_ref.as_ref().map(|d| d.id.clone()).unwrap_or_default(),

        data: payload,
        blocks: Vec::new(),

        file_metrics: Some(FileMetrics {
            throughput_mbps,
            compression_ratio: if original_size > 0 {
                processed_size as f64 / original_size as f64
            } else {
                1.0
            },
            dedup_savings_bytes: 0,
            processing_time_ms: duration_ms,
            algorithm_used: planned.plan.label(),
            strategy_reason: planned.reason.clone(),
            candidates_tried: planned.candidates_tried,
            reconstruct_ms,
        }),

        pointer_to: String::new(),
        file_hash: file_hash.to_vec(),
        ref_count: 1,
        envelope_checksum: 0,
        created_at: now,
        updated_at: now,

        codec: planned.plan.codec as i32,
        codec_level: planned.plan.level,
        transforms: planned.plan.transforms.clone(),
        dictionary: dictionary_ref,
        content_class: profile.class.as_str().to_string(),
        plan_reason: planned.reason,
        verified,
    };

    envelope.envelope_checksum = payload_checksum(&envelope.data);
    envelope
}

pub fn seal(mut envelope: ProcessedFile) -> Result<Sealed, ContainerError> {
    envelope.envelope_checksum = payload_checksum(&envelope.data);
    let mut bytes = Vec::with_capacity(envelope.encoded_len());
    envelope
        .encode(&mut bytes)
        .map_err(|e| ContainerError::Malformed(e.to_string()))?;
    Ok(Sealed { envelope, bytes })
}

fn payload_checksum(payload: &[u8]) -> u64 {
    let h = blake3::hash(payload);
    u64::from_le_bytes(h.as_bytes()[..8].try_into().unwrap_or([0u8; 8]))
}

fn legacy_algorithm(c: Codec) -> Algorithm {
    match c {
        Codec::Stored => Algorithm::Passthrough,
        Codec::Lz4 => Algorithm::Lz4,
        Codec::Zstd => Algorithm::ZstdBalanced,
        _ => Algorithm::Auto,
    }
}

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub fn parse(bytes: &[u8]) -> Result<ProcessedFile, ContainerError> {
    let envelope =
        ProcessedFile::decode(bytes).map_err(|e| ContainerError::Decode(e.to_string()))?;

    if envelope.envelope_checksum != 0 {
        let obtido = payload_checksum(&envelope.data);
        if obtido != envelope.envelope_checksum {
            return Err(ContainerError::ChecksumMismatch {
                esperado: envelope.envelope_checksum,
                obtido,
            });
        }
    }

    Ok(envelope)
}

pub struct Restored {
    pub data: Vec<u8>,
    pub duration_ms: f64,
    pub fidelity_checked: bool,
}

pub fn restore(
    envelope: &ProcessedFile,
    dicts: &dyn DictionaryResolver,
) -> Result<Restored, ContainerError> {
    let start = std::time::Instant::now();

    let dict_id = envelope
        .dictionary
        .as_ref()
        .map(|d| d.id.clone())
        .unwrap_or_else(|| envelope.dictionary_id.clone());

    let dict_bytes: Option<Arc<Dictionary>> = if dict_id.is_empty() {
        None
    } else {
        let found = dicts
            .resolve(&dict_id)
            .ok_or_else(|| ContainerError::MissingDictionary { id: dict_id.clone() })?;

        if let Some(reference) = envelope.dictionary.as_ref() {
            if reference.digest.len() == 32 && reference.digest != found.digest {
                return Err(ContainerError::DictionaryMismatch { id: dict_id });
            }
        }
        Some(found)
    };
    let dict_slice = dict_bytes.as_ref().map(|d| d.bytes.as_slice());

    let (codec_id, transforms) = if envelope.version >= 3 {
        let c = Codec::try_from(envelope.codec).map_err(|_| {
            ContainerError::Malformed(format!("codec desconhecido: {}", envelope.codec))
        })?;
        if c == Codec::Unspecified {
            return Err(ContainerError::Malformed(
                "envelope v3 sem codec definido".to_string(),
            ));
        }
        (c, envelope.transforms.clone())
    } else {
        let legacy = Algorithm::try_from(envelope.algorithm).map_err(|_| {
            ContainerError::Malformed(format!("algoritmo legado desconhecido: {}", envelope.algorithm))
        })?;
        let (c, _) = codec::from_legacy(legacy, envelope.compression_level);
        (c, Vec::new())
    };

    let data = planner::restore(
        &envelope.data,
        codec_id,
        &transforms,
        dict_slice,
        Some(envelope.original_size),
    )
    .map_err(|e| ContainerError::Restore(e.to_string()))?;

    let mut fidelity_checked = false;
    if envelope.file_hash.len() == 32 {
        let obtido: [u8; 32] = blake3::hash(&data).into();
        if obtido.as_slice() != envelope.file_hash.as_slice() {
            return Err(ContainerError::FidelityMismatch {
                esperado: hex::encode(&envelope.file_hash),
                obtido: hex::encode(obtido),
            });
        }
        fidelity_checked = true;
    }

    if envelope.original_size != 0 && data.len() as u64 != envelope.original_size {
        return Err(ContainerError::Malformed(format!(
            "tamanho reconstruído ({}) difere do declarado ({})",
            data.len(),
            envelope.original_size
        )));
    }

    Ok(Restored {
        data,
        duration_ms: start.elapsed().as_secs_f64() * 1000.0,
        fidelity_checked,
    })
}

pub fn verify_roundtrip(
    planned: &Planned,
    original: &[u8],
    dict: Option<&[u8]>,
) -> Result<f64, ContainerError> {
    let start = std::time::Instant::now();
    let back = planner::restore(
        &planned.payload,
        planned.plan.codec,
        &planned.plan.transforms,
        if planned.plan.use_dictionary { dict } else { None },
        Some(original.len() as u64),
    )
    .map_err(|e| ContainerError::Restore(e.to_string()))?;

    if back != original {
        return Err(ContainerError::FidelityMismatch {
            esperado: hex::encode(blake3::hash(original).as_bytes()),
            obtido: hex::encode(blake3::hash(&back).as_bytes()),
        });
    }
    Ok(start.elapsed().as_secs_f64() * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content;
    use crate::planner::{compress_best, Effort, ForcedPlan, Limits};

    fn amostra() -> Vec<u8> {
        let mut s = String::from("id,uf,valor\n");
        for i in 0..3000 {
            s.push_str(&format!("{},SP,{}.{:02}\n", i, i * 7, i % 100));
        }
        s.into_bytes()
    }

    fn processa(data: &[u8], effort: Effort) -> (ProcessedFile, Vec<u8>) {
        let profile = content::classify(data, "dados.csv", None);
        let planned = compress_best(
            data,
            &profile,
            effort,
            None,
            &ForcedPlan::default(),
            &Limits::default(),
        )
        .unwrap();
        let hash: [u8; 32] = blake3::hash(data).into();
        let rt = verify_roundtrip(&planned, data, None).unwrap();
        let env = build(planned, &profile, "dados.csv", &hash, None, 1.0, true, rt);
        let sealed = seal(env).unwrap();
        (sealed.envelope, sealed.bytes)
    }

    #[test]
    fn ciclo_completo_grava_e_reconstroi() {
        let data = amostra();
        let (_, bytes) = processa(&data, Effort::Max);

        let lido = parse(&bytes).unwrap();
        assert_eq!(lido.version, CONTAINER_VERSION);
        assert!(lido.verified);
        assert!(lido.processed_size < lido.original_size);
        assert!(!lido.plan_reason.is_empty());
        assert_eq!(lido.content_class, "tabular");

        let restaurado = restore(&lido, &NoDictionaries).unwrap();
        assert_eq!(restaurado.data, data);
        assert!(restaurado.fidelity_checked);
    }

    #[test]
    fn payload_corrompido_e_detectado_antes_de_descomprimir() {
        let data = amostra();
        let (env, _) = processa(&data, Effort::Balanced);

        let mut corrompido = env.clone();
        let n = corrompido.data.len() / 2;
        corrompido.data[n] ^= 0xFF;
        let mut bytes = Vec::new();
        corrompido.encode(&mut bytes).unwrap();

        match parse(&bytes) {
            Err(ContainerError::ChecksumMismatch { .. }) => {}
            outro => panic!("esperava ChecksumMismatch, veio {:?}", outro.map(|e| e.version)),
        }
    }

    #[test]
    fn hash_divergente_e_reportado_como_falha_de_fidelidade() {
        let data = amostra();
        let (mut env, _) = processa(&data, Effort::Balanced);
        env.file_hash = vec![0u8; 32];
        let sealed = seal(env).unwrap();
        let lido = parse(&sealed.bytes).unwrap();

        match restore(&lido, &NoDictionaries) {
            Err(ContainerError::FidelityMismatch { .. }) => {}
            _ => panic!("deveria ter detectado divergência de hash"),
        }
    }

    #[test]
    fn dicionario_ausente_gera_erro_claro() {
        let data = amostra();
        let (mut env, _) = processa(&data, Effort::Balanced);
        env.dictionary = Some(DictionaryRef {
            id: "text_csv.deadbeefdeadbeef".to_string(),
            digest: vec![7u8; 32],
            size: 1024,
        });
        let sealed = seal(env).unwrap();
        let lido = parse(&sealed.bytes).unwrap();

        match restore(&lido, &NoDictionaries) {
            Err(ContainerError::MissingDictionary { id }) => {
                assert_eq!(id, "text_csv.deadbeefdeadbeef");
            }
            _ => panic!("deveria reclamar de dicionário ausente"),
        }
    }

    #[test]
    fn envelope_legado_v2_continua_reconstruivel() {
        let data = amostra();
        let payload = codec::encode(&data, Codec::Lz4, 0, None).unwrap();
        let hash: [u8; 32] = blake3::hash(&data).into();

        let legado = ProcessedFile {
            version: 2,
            original_name: "antigo.csv".to_string(),
            original_mime: "text/csv".to_string(),
            original_size: data.len() as u64,
            processed_size: payload.len() as u64,
            algorithm: Algorithm::Lz4 as i32,
            compression_level: 0,
            data: payload,
            file_hash: hash.to_vec(),
            ..Default::default()
        };

        let mut bytes = Vec::new();
        legado.encode(&mut bytes).unwrap();
        let lido = parse(&bytes).unwrap();
        let restaurado = restore(&lido, &NoDictionaries).unwrap();
        assert_eq!(restaurado.data, data);
    }

    #[test]
    fn essencia_nunca_fica_maior_que_o_original() {
        let mut x = 0xCAFEBABEu32;
        let aleatorio: Vec<u8> = (0..150_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x >> 11) as u8
            })
            .collect();

        let profile = content::classify(&aleatorio, "r.bin", None);
        let planned = compress_best(
            &aleatorio,
            &profile,
            Effort::Max,
            None,
            &ForcedPlan::default(),
            &Limits::default(),
        )
        .unwrap();
        let hash: [u8; 32] = blake3::hash(&aleatorio).into();
        let env = build(planned, &profile, "r.bin", &hash, None, 1.0, false, 0.0);
        assert_eq!(env.processed_size, aleatorio.len() as u64);
        assert_eq!(env.codec, Codec::Stored as i32);

        let sealed = seal(env).unwrap();
        let restaurado = restore(&parse(&sealed.bytes).unwrap(), &NoDictionaries).unwrap();
        assert_eq!(restaurado.data, aleatorio);
    }

    #[test]
    fn entrada_vazia_sobrevive_ao_ciclo() {
        let profile = content::classify(&[], "vazio.bin", None);
        let planned = compress_best(
            &[],
            &profile,
            Effort::Balanced,
            None,
            &ForcedPlan::default(),
            &Limits::default(),
        )
        .unwrap();
        let hash: [u8; 32] = blake3::hash(&[]).into();
        let env = build(planned, &profile, "vazio.bin", &hash, None, 0.0, true, 0.0);
        let sealed = seal(env).unwrap();
        let restaurado = restore(&parse(&sealed.bytes).unwrap(), &NoDictionaries).unwrap();
        assert!(restaurado.data.is_empty());
    }
}
