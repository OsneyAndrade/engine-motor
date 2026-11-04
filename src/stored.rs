use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::blockfile::{self, BlockPlan, Reader, Source, StreamSource};
use crate::codec::{self, Codec};
use crate::container;
use crate::proto::{FileMetrics, ProcessedFile};
use crate::service::EngineError;
use crate::transform;

pub type FileReader = Reader<StreamSource<File>>;

pub struct BlockFile {
    pub path: PathBuf,
    pub reader: FileReader,
}

pub enum Stored {
    Legacy(Box<ProcessedFile>),
    Blocks(Box<BlockFile>),
}

impl Stored {
    pub fn meta(&self) -> ObjectMeta {
        match self {
            Stored::Legacy(env) => ObjectMeta::from_envelope(env),
            Stored::Blocks(bf) => ObjectMeta::from_reader(&bf.reader),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BlockStats {
    pub count: usize,
    pub size: usize,
    pub stored: usize,
    pub plans: Vec<(String, usize)>,
}

#[derive(Debug, Clone)]
pub struct ObjectMeta {
    pub version: i32,
    pub original_name: String,
    pub original_names: Vec<String>,
    pub original_mime: String,
    pub content_class: String,
    pub original_size: u64,
    pub processed_size: u64,
    pub savings_pct: f32,
    pub file_hash: Vec<u8>,
    pub codec: i32,
    pub codec_level: i32,
    pub plan_label: String,
    pub transforms: String,
    pub dictionary_id: Option<String>,
    pub plan_reason: String,
    pub verified: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub file_metrics: Option<FileMetrics>,
    pub blocks: Option<BlockStats>,
}

pub trait PlanLabelled {
    fn plan_label(&self) -> String;
}

impl PlanLabelled for ProcessedFile {
    fn plan_label(&self) -> String {
        self.file_metrics
            .as_ref()
            .map(|m| m.algorithm_used.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                Codec::try_from(self.codec)
                    .map(|c| codec::name(c).to_string())
                    .unwrap_or_else(|_| "desconhecido".to_string())
            })
    }
}

impl PlanLabelled for ObjectMeta {
    fn plan_label(&self) -> String {
        self.plan_label.clone()
    }
}

pub fn block_plan_label(plan: &BlockPlan) -> String {
    let mut s = String::new();
    if !plan.transforms.is_empty() {
        s.push_str(&transform::chain_label(&plan.transforms));
        s.push('+');
    }
    s.push_str(codec::name(plan.codec));
    if codec::info(plan.codec).map(|c| c.max_level > 0).unwrap_or(false) {
        s.push_str(&format!("-{}", plan.level));
    }
    if plan.dictionary.is_some() {
        s.push_str("+dict");
    }
    s
}

pub fn blocks_label(dominant: Option<&BlockPlan>) -> String {
    match dominant {
        Some(plan) => format!("blocos:{}", block_plan_label(plan)),
        None => "blocos:stored".to_string(),
    }
}

impl ObjectMeta {
    pub fn from_envelope(env: &ProcessedFile) -> Self {
        let codec_name = Codec::try_from(env.codec)
            .map(|c| codec::name(c).to_string())
            .unwrap_or_else(|_| "desconhecido".to_string());
        let _ = codec_name;
        ObjectMeta {
            version: env.version,
            original_name: env.original_name.clone(),
            original_names: env.original_names.clone(),
            original_mime: env.original_mime.clone(),
            content_class: env.content_class.clone(),
            original_size: env.original_size,
            processed_size: env.processed_size,
            savings_pct: env.savings_pct,
            file_hash: env.file_hash.clone(),
            codec: env.codec,
            codec_level: env.codec_level,
            plan_label: env.plan_label(),
            transforms: transform::chain_label(&env.transforms),
            dictionary_id: env.dictionary.as_ref().map(|d| d.id.clone()),
            plan_reason: env.plan_reason.clone(),
            verified: env.verified,
            created_at: env.created_at,
            updated_at: env.updated_at,
            file_metrics: env.file_metrics.clone(),
            blocks: None,
        }
    }

    pub fn from_reader<S: Source>(reader: &Reader<S>) -> Self {
        let meta = reader.metadata();
        let plans = reader.plans();
        let mut per_plan = vec![0usize; plans.len()];
        let mut stored = 0usize;
        for i in 0..reader.block_count() {
            if let Some(info) = reader.block_info(i) {
                let idx = info.plan_idx as usize;
                if let Some(slot) = per_plan.get_mut(idx) {
                    *slot += 1;
                }
                if plans.get(idx).map(BlockPlan::is_stored).unwrap_or(false) {
                    stored += 1;
                }
            }
        }

        let dominant = per_plan
            .iter()
            .enumerate()
            .filter(|(i, n)| **n > 0 && !plans[*i].is_stored())
            .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(&a.0)))
            .map(|(i, _)| i);
        let dominant_plan = dominant.map(|i| &plans[i]);

        let original_size = reader.original_size();
        let file_len = reader.file_len();
        let savings = if original_size > 0 {
            (1.0 - file_len as f64 / original_size as f64) * 100.0
        } else {
            0.0
        };

        ObjectMeta {
            version: 4,
            original_name: meta.original_name,
            original_names: meta.original_names,
            original_mime: meta.original_mime,
            content_class: meta.content_class,
            original_size,
            processed_size: file_len,
            savings_pct: savings as f32,
            file_hash: reader.file_hash().to_vec(),
            codec: dominant_plan.map(|p| p.codec as i32).unwrap_or(Codec::Stored as i32),
            codec_level: dominant_plan.map(|p| p.level).unwrap_or(0),
            plan_label: blocks_label(dominant_plan),
            transforms: transform::chain_label(
                dominant_plan.map(|p| p.transforms.as_slice()).unwrap_or(&[]),
            ),
            dictionary_id: dominant_plan
                .and_then(|p| p.dictionary.as_ref())
                .map(|d| d.id.clone()),
            plan_reason: meta.plan_reason,
            verified: meta.verified,
            created_at: meta.created_at,
            updated_at: meta.updated_at,
            file_metrics: meta.file_metrics,
            blocks: Some(BlockStats {
                count: reader.block_count(),
                size: reader.block_size(),
                stored,
                plans: plans
                    .iter()
                    .zip(per_plan.iter())
                    .map(|(p, n)| (block_plan_label(p), *n))
                    .collect(),
            }),
        }
    }

    pub fn is_blocks(&self) -> bool {
        self.blocks.is_some()
    }
}

fn io_error(path: &Path, e: std::io::Error, id: &str) -> EngineError {
    if e.kind() == std::io::ErrorKind::NotFound {
        EngineError::not_found(format!("objeto {id} não encontrado no vault"))
    } else {
        EngineError::Io(format!("{}: {e}", path.display()))
    }
}

pub fn open_path(path: &Path, id: &str) -> Result<Stored, EngineError> {
    let mut file = File::open(path).map_err(|e| io_error(path, e, id))?;
    let mut head = [0u8; blockfile::MAGIC.len()];
    let mut got = 0usize;
    while got < head.len() {
        match file.read(&mut head[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(io_error(path, e, id)),
        }
    }

    if blockfile::is_v4(&head[..got]) {
        let reader = Reader::from_reader(file)?;
        return Ok(Stored::Blocks(Box::new(BlockFile {
            path: path.to_path_buf(),
            reader,
        })));
    }

    file.seek(SeekFrom::Start(0))
        .map_err(|e| io_error(path, e, id))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| io_error(path, e, id))?;
    Ok(Stored::Legacy(Box::new(container::parse(&bytes)?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockfile::{compress_block, Metadata, Writer};
    use crate::planner::Plan;

    fn plano(codec: Codec, level: i32) -> Plan {
        Plan {
            transforms: Vec::new(),
            codec,
            level,
            use_dictionary: false,
        }
    }

    fn arquivo_v4(blocos: &[(&[u8], Plan)], log2: u8) -> Vec<u8> {
        let mut w = Writer::new(Vec::new(), log2).unwrap();
        for (dados, p) in blocos {
            let cb = compress_block(dados, p, None).unwrap();
            w.push_compressed(dados, cb).unwrap();
        }
        let (out, _) = w
            .finish(&Metadata {
                original_name: "a.bin".into(),
                original_names: vec!["a.bin".into()],
                original_mime: "application/octet-stream".into(),
                content_class: "binary".into(),
                ..Default::default()
            })
            .unwrap();
        out
    }

    #[test]
    fn rotulo_do_plano_em_blocos_segue_o_formato_documentado() {
        assert_eq!(blocks_label(None), "blocos:stored");
        let p = BlockPlan::new(Codec::Zstd, 3);
        assert_eq!(blocks_label(Some(&p)), "blocos:zstd-3");
        let l = BlockPlan::new(Codec::Lz4, 0);
        assert_eq!(blocks_label(Some(&l)), "blocos:lz4");
    }

    #[test]
    fn meta_do_v4_resume_plano_dominante_e_blocos_armazenados() {
        let texto = vec![b'a'; 4096];
        let mut ruido = vec![0u8; 4096];
        let mut x = 0x1234_5678u32;
        for b in ruido.iter_mut() {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *b = (x >> 9) as u8;
        }
        let bytes = arquivo_v4(
            &[
                (&texto, plano(Codec::Zstd, 3)),
                (&texto, plano(Codec::Zstd, 3)),
                (&ruido, plano(Codec::Zstd, 3)),
                (&texto[..100], plano(Codec::Zstd, 3)),
            ],
            12,
        );
        let leitor = Reader::from_bytes(&bytes).unwrap();
        let meta = ObjectMeta::from_reader(&leitor);
        assert_eq!(meta.version, 4);
        assert_eq!(meta.plan_label, "blocos:zstd-3");
        assert_eq!(meta.codec, Codec::Zstd as i32);
        assert_eq!(meta.codec_level, 3);
        assert_eq!(meta.original_size, 4096 * 3 + 100);
        assert_eq!(meta.processed_size, bytes.len() as u64);
        let b = meta.blocks.unwrap();
        assert_eq!(b.count, 4);
        assert_eq!(b.size, 4096);
        assert_eq!(b.stored, 1);
        assert_eq!(meta.original_name, "a.bin");
        assert_eq!(meta.file_hash.len(), 32);
    }

    #[test]
    fn arquivo_todo_armazenado_vira_blocos_stored() {
        let mut ruido = vec![0u8; 5000];
        let mut x = 0xDEAD_BEEFu32;
        for b in ruido.iter_mut() {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *b = (x >> 9) as u8;
        }
        let bytes = arquivo_v4(&[(&ruido[..4096], plano(Codec::Zstd, 3)), (&ruido[4096..], plano(Codec::Zstd, 3))], 12);
        let meta = ObjectMeta::from_reader(&Reader::from_bytes(&bytes).unwrap());
        assert_eq!(meta.plan_label, "blocos:stored");
        assert_eq!(meta.codec, Codec::Stored as i32);
    }

    #[test]
    fn arquivo_vazio_tem_zero_blocos() {
        let bytes = arquivo_v4(&[], 20);
        let meta = ObjectMeta::from_reader(&Reader::from_bytes(&bytes).unwrap());
        assert_eq!(meta.original_size, 0);
        assert_eq!(meta.blocks.unwrap().count, 0);
        assert_eq!(meta.savings_pct, 0.0);
    }

    #[test]
    fn open_path_distingue_v4_de_v3_pelo_prefixo() {
        let dir = std::env::temp_dir().join(format!("syntra-stored-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let v4 = dir.join("a.syntra");
        std::fs::write(&v4, arquivo_v4(&[(&[7u8; 100], plano(Codec::Zstd, 3))], 12)).unwrap();
        match open_path(&v4, "x").unwrap() {
            Stored::Blocks(bf) => assert_eq!(bf.reader.original_size(), 100),
            Stored::Legacy(_) => panic!("era v4"),
        }

        let dados = vec![b'z'; 3000];
        let payload = codec::encode(&dados, Codec::Zstd, 3, None).unwrap();
        let env = ProcessedFile {
            version: 3,
            original_name: "v3.bin".into(),
            original_size: dados.len() as u64,
            processed_size: payload.len() as u64,
            codec: Codec::Zstd as i32,
            codec_level: 3,
            data: payload,
            file_hash: blake3::hash(&dados).as_bytes().to_vec(),
            ..Default::default()
        };
        let sealed = container::seal(env).unwrap();
        let v3 = dir.join("b.syntra");
        std::fs::write(&v3, &sealed.bytes).unwrap();
        match open_path(&v3, "x").unwrap() {
            Stored::Legacy(e) => {
                let meta = ObjectMeta::from_envelope(&e);
                assert_eq!(meta.version, 3);
                assert_eq!(meta.original_name, "v3.bin");
                assert!(!meta.is_blocks());
            }
            Stored::Blocks(_) => panic!("era v3"),
        }

        let ausente = dir.join("nao-existe.syntra");
        match open_path(&ausente, "abc") {
            Err(EngineError::NotFound { message }) => assert!(message.contains("abc")),
            _ => panic!("deveria ser not_found"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
