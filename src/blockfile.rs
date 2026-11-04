use std::borrow::Cow;
use std::fmt;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::sync::Arc;

use prost::Message;

use crate::codec::{self, Codec};
use crate::container::DictionaryResolver;
use crate::dict_store::Dictionary;
use crate::planner::{self, Plan};
use crate::proto::{BlockPlanV4, DictionaryRef, FileMetrics, FooterV4};
use crate::transform::{Transform, TransformStep, WIDTHS};

pub const MAGIC: [u8; 4] = *b"SYNT";
pub const FORMAT_VERSION: u8 = 4;
pub const HEADER_LEN: usize = 7;
pub const TRAILER_LEN: usize = 24;
pub const HASH_LEN: usize = 16;
pub const MIN_BLOCK_LOG2: u8 = 12;
pub const MAX_BLOCK_LOG2: u8 = 26;
pub const DEFAULT_BLOCK_LOG2: u8 = 20;
pub const MAX_FOOTER_LEN: usize = 64 * 1024 * 1024;
pub const MAX_PLANS: usize = 65_536;
pub const MAX_CHAIN: usize = 8;
pub const MAX_NAMES: usize = 1024;
pub const MAX_DICTIONARY_ID: usize = 256;

const ENTRY_BYTES: usize = 4 + 4 + 2 + HASH_LEN;
const FOOTER_RESERVE: usize = 1024 * 1024;
const KNOWN_FLAGS: u8 = 0;
const RESTORE_RESERVE_MAX: u64 = 64 * 1024 * 1024;

pub const MAX_BLOCKS: usize = (MAX_FOOTER_LEN - FOOTER_RESERVE) / ENTRY_BYTES;

pub fn is_v4(bytes: &[u8]) -> bool {
    bytes.len() >= MAGIC.len() && bytes[..MAGIC.len()] == MAGIC
}

pub fn slack_len(n: usize) -> usize {
    n.saturating_add(n / 8).saturating_add(4096)
}

fn short_hash(bytes: &[u8]) -> [u8; HASH_LEN] {
    let full = blake3::hash(bytes);
    let mut out = [0u8; HASH_LEN];
    out.copy_from_slice(&full.as_bytes()[..HASH_LEN]);
    out
}

fn le_u32(buf: &[u8], i: usize) -> u32 {
    match buf.get(i * 4..i * 4 + 4) {
        Some(b) => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        None => 0,
    }
}

fn le_u16(buf: &[u8], i: usize) -> u16 {
    match buf.get(i * 2..i * 2 + 2) {
        Some(b) => u16::from_le_bytes([b[0], b[1]]),
        None => 0,
    }
}

fn check_log2(log2: u8) -> Result<(), BlockError> {
    if (MIN_BLOCK_LOG2..=MAX_BLOCK_LOG2).contains(&log2) {
        Ok(())
    } else {
        Err(BlockError::BlockSizeInvalid { log2 })
    }
}

#[derive(Debug)]
pub enum BlockError {
    BadMagic,
    UnsupportedVersion { found: u8 },
    UnsupportedFlags { found: u8 },
    BlockSizeInvalid { log2: u8 },
    Truncated { detail: &'static str },
    FooterTooLarge { len: u64, max: u64 },
    FooterCorrupted(String),
    LayoutInvalid(String),
    PlanInvalid { plan: usize, reason: String },
    BlockOutOfRange { index: usize, count: usize },
    BlockCorrupted { index: usize, expected: String, found: String },
    DictionaryMissing { block: usize, id: String },
    DictionaryMismatch { block: usize, id: String },
    BlockDecode { index: usize, reason: String },
    Fidelity { expected: String, found: String },
    Io(String),
    InvalidInput(String),
}

impl fmt::Display for BlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockError::BadMagic => write!(f, "assinatura inválida: o arquivo não começa com SYNT"),
            BlockError::UnsupportedVersion { found } => write!(
                f,
                "versão {found} do formato em blocos não é suportada (esperado {FORMAT_VERSION})"
            ),
            BlockError::UnsupportedFlags { found } => {
                write!(f, "flags {found:#04x} desconhecidas no cabeçalho")
            }
            BlockError::BlockSizeInvalid { log2 } => write!(
                f,
                "log2 do tamanho de bloco ({log2}) fora da faixa {MIN_BLOCK_LOG2}..={MAX_BLOCK_LOG2}"
            ),
            BlockError::Truncated { detail } => write!(f, "arquivo truncado: {detail}"),
            BlockError::FooterTooLarge { len, max } => {
                write!(f, "rodapé declarado com {len} bytes excede o limite de {max}")
            }
            BlockError::FooterCorrupted(m) => write!(f, "rodapé corrompido: {m}"),
            BlockError::LayoutInvalid(m) => write!(f, "estrutura inconsistente: {m}"),
            BlockError::PlanInvalid { plan, reason } => write!(f, "plano {plan} inválido: {reason}"),
            BlockError::BlockOutOfRange { index, count } => {
                write!(f, "bloco {index} fora da faixa (o arquivo tem {count})")
            }
            BlockError::BlockCorrupted { index, expected, found } => write!(
                f,
                "bloco {index} corrompido: hash do payload não confere (esperado {expected}, obtido {found})"
            ),
            BlockError::DictionaryMissing { block, id } => write!(
                f,
                "dicionário '{id}' necessário ao bloco {block} não está disponível neste nó"
            ),
            BlockError::DictionaryMismatch { block, id } => write!(
                f,
                "dicionário '{id}' encontrado não corresponde ao usado no bloco {block}"
            ),
            BlockError::BlockDecode { index, reason } => {
                write!(f, "falha ao reconstruir o bloco {index}: {reason}")
            }
            BlockError::Fidelity { expected, found } => write!(
                f,
                "reconstrução não é bit a bit idêntica ao original (hash esperado {expected}, obtido {found})"
            ),
            BlockError::Io(m) => write!(f, "erro de E/S: {m}"),
            BlockError::InvalidInput(m) => write!(f, "uso inválido do escritor: {m}"),
        }
    }
}

impl std::error::Error for BlockError {}

impl BlockError {
    pub fn code(&self) -> &'static str {
        match self {
            BlockError::BadMagic => "bad_magic",
            BlockError::UnsupportedVersion { .. } => "unsupported_version",
            BlockError::UnsupportedFlags { .. } => "unsupported_flags",
            BlockError::BlockSizeInvalid { .. } => "block_size_invalid",
            BlockError::Truncated { .. } => "truncated",
            BlockError::FooterTooLarge { .. } => "footer_too_large",
            BlockError::FooterCorrupted(_) => "footer_corrupted",
            BlockError::LayoutInvalid(_) => "layout_invalid",
            BlockError::PlanInvalid { .. } => "plan_invalid",
            BlockError::BlockOutOfRange { .. } => "block_out_of_range",
            BlockError::BlockCorrupted { .. } => "block_corrupted",
            BlockError::DictionaryMissing { .. } => "dictionary_unavailable",
            BlockError::DictionaryMismatch { .. } => "dictionary_mismatch",
            BlockError::BlockDecode { .. } => "block_decode_failed",
            BlockError::Fidelity { .. } => "fidelity_mismatch",
            BlockError::Io(_) => "io_error",
            BlockError::InvalidInput(_) => "invalid_input",
        }
    }

    pub fn block_index(&self) -> Option<usize> {
        match self {
            BlockError::BlockCorrupted { index, .. }
            | BlockError::BlockDecode { index, .. }
            | BlockError::BlockOutOfRange { index, .. } => Some(*index),
            BlockError::DictionaryMissing { block, .. }
            | BlockError::DictionaryMismatch { block, .. } => Some(*block),
            _ => None,
        }
    }
}

impl From<io::Error> for BlockError {
    fn from(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            BlockError::Truncated { detail: "leitura além do fim do arquivo" }
        } else {
            BlockError::Io(e.to_string())
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlockPlan {
    pub transforms: Vec<TransformStep>,
    pub codec: Codec,
    pub level: i32,
    pub dictionary: Option<DictionaryRef>,
}

impl BlockPlan {
    pub fn stored() -> Self {
        BlockPlan {
            transforms: Vec::new(),
            codec: Codec::Stored,
            level: 0,
            dictionary: None,
        }
    }

    pub fn new(codec: Codec, level: i32) -> Self {
        BlockPlan {
            transforms: Vec::new(),
            codec,
            level: codec::clamp_level(codec, level),
            dictionary: None,
        }
    }

    pub fn with_steps(mut self, steps: &[(Transform, u32)]) -> Self {
        self.transforms = steps
            .iter()
            .map(|(t, w)| crate::transform::step(*t, *w, 0))
            .collect();
        self
    }

    pub fn with_dictionary(mut self, dict: &Dictionary) -> Self {
        self.dictionary = Some(dictionary_ref(dict));
        self
    }

    pub fn is_stored(&self) -> bool {
        self.codec == Codec::Stored && self.transforms.is_empty()
    }

    pub fn label(&self) -> String {
        let mut s = String::new();
        if !self.transforms.is_empty() {
            s.push_str(&crate::transform::chain_label(&self.transforms));
            s.push('+');
        }
        s.push_str(codec::name(self.codec));
        if codec::info(self.codec).map(|c| c.max_level > 0).unwrap_or(false) {
            s.push_str(&format!(":{}", self.level));
        }
        if self.dictionary.is_some() {
            s.push_str("+dict");
        }
        s
    }

    fn normalized(mut self) -> Self {
        for s in self.transforms.iter_mut() {
            s.input_len = 0;
        }
        self
    }

    fn validate(&self) -> Result<(), String> {
        let info = codec::info(self.codec).ok_or_else(|| "codec não especificado".to_string())?;
        if self.level < info.min_level || self.level > info.max_level {
            return Err(format!(
                "nível {} fora da faixa {}..={} do codec {}",
                self.level, info.min_level, info.max_level, info.name
            ));
        }
        if self.transforms.len() > MAX_CHAIN {
            return Err(format!(
                "cadeia de {} transforms excede o limite de {MAX_CHAIN}",
                self.transforms.len()
            ));
        }
        for (i, s) in self.transforms.iter().enumerate() {
            let t = Transform::try_from(s.transform)
                .map_err(|_| format!("transform {i} desconhecido: {}", s.transform))?;
            match t {
                Transform::Unspecified => {
                    return Err(format!("transform {i} não especificado"));
                }
                Transform::Delta | Transform::ByteSplit => {
                    if !WIDTHS.contains(&s.width) {
                        return Err(format!("transform {i} com largura inválida: {}", s.width));
                    }
                }
                Transform::Rle | Transform::CsvColumnar => {}
            }
            if s.input_len != 0 {
                return Err(format!("transform {i} traz input_len, que no v4 deve ser 0"));
            }
        }
        if let Some(d) = &self.dictionary {
            if d.id.is_empty() || d.id.len() > MAX_DICTIONARY_ID {
                return Err("id do dicionário vazio ou longo demais".to_string());
            }
            if d.digest.len() != 32 {
                return Err("digest do dicionário deve ter 32 bytes".to_string());
            }
            if !codec::supports_dictionary(self.codec) {
                return Err(format!("codec {} não aceita dicionário", info.name));
            }
        }
        Ok(())
    }

    fn to_proto(&self) -> BlockPlanV4 {
        BlockPlanV4 {
            transforms: self.transforms.clone(),
            codec: self.codec as i32,
            codec_level: self.level,
            dictionary: self.dictionary.clone(),
        }
    }

    fn from_proto(p: &BlockPlanV4) -> Result<Self, String> {
        let codec_id = Codec::try_from(p.codec)
            .map_err(|_| format!("codec desconhecido: {}", p.codec))?;
        let plan = BlockPlan {
            transforms: p.transforms.clone(),
            codec: codec_id,
            level: p.codec_level,
            dictionary: p.dictionary.clone(),
        };
        plan.validate()?;
        Ok(plan)
    }
}

fn dictionary_ref(dict: &Dictionary) -> DictionaryRef {
    DictionaryRef {
        id: dict.id.clone(),
        digest: dict.digest.to_vec(),
        size: dict.size(),
    }
}

#[derive(Debug, Clone)]
pub struct CompressedBlock {
    pub plan: BlockPlan,
    pub payload: Vec<u8>,
}

fn stored_block(original: &[u8]) -> CompressedBlock {
    CompressedBlock {
        plan: BlockPlan::stored(),
        payload: original.to_vec(),
    }
}

pub fn compress_block(
    original: &[u8],
    plan: &Plan,
    dict: Option<&Dictionary>,
) -> io::Result<CompressedBlock> {
    if original.is_empty() || plan.is_stored() {
        return Ok(stored_block(original));
    }

    let dict_in_use = plan.use_dictionary
        && codec::supports_dictionary(plan.codec)
        && dict.map(|d| !d.bytes.is_empty()).unwrap_or(false);
    let dict_bytes = if dict_in_use {
        dict.map(|d| d.bytes.as_slice())
    } else {
        None
    };

    let mut exec = plan.clone();
    exec.use_dictionary = dict_in_use;
    let (payload, steps) = planner::execute(original, &exec, dict_bytes)?;

    if payload.len() >= original.len() {
        return Ok(stored_block(original));
    }

    let block_plan = BlockPlan {
        transforms: steps,
        codec: plan.codec,
        level: codec::clamp_level(plan.codec, plan.level),
        dictionary: if dict_in_use { dict.map(dictionary_ref) } else { None },
    }
    .normalized();

    Ok(CompressedBlock {
        plan: block_plan,
        payload,
    })
}

fn check_declared_size(payload: &[u8], codec_id: Codec, limit: usize) -> io::Result<()> {
    let too_big = |declared: u64| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("tamanho declarado no payload ({declared}) excede o limite do bloco ({limit})"),
        )
    };
    match codec_id {
        Codec::Lz4 => {
            if let Some(head) = payload.get(..4) {
                let declared = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as u64;
                if declared > limit as u64 {
                    return Err(too_big(declared));
                }
            }
        }
        Codec::Zstd => match zstd::zstd_safe::get_frame_content_size(payload) {
            Ok(Some(declared)) if declared > limit as u64 => return Err(too_big(declared)),
            Ok(None) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "frame zstd sem tamanho de conteúdo declarado",
                ));
            }
            _ => {}
        },
        _ => {}
    }
    Ok(())
}

pub fn decode_block(
    payload: &[u8],
    plan: &BlockPlan,
    dict: Option<&[u8]>,
    original_len: usize,
) -> io::Result<Vec<u8>> {
    let mismatch = |found: usize| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bloco reconstruído com {found} bytes, esperado {original_len}"),
        )
    };

    if plan.is_stored() {
        if payload.len() != original_len {
            return Err(mismatch(payload.len()));
        }
        return Ok(payload.to_vec());
    }

    check_declared_size(payload, plan.codec, slack_len(original_len))?;
    let out = planner::restore(
        payload,
        plan.codec,
        &plan.transforms,
        dict,
        Some(original_len as u64),
    )?;
    if out.len() != original_len {
        return Err(mismatch(out.len()));
    }
    Ok(out)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Metadata {
    pub original_name: String,
    pub original_names: Vec<String>,
    pub original_mime: String,
    pub content_class: String,
    pub plan_reason: String,
    pub verified: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub file_metrics: Option<FileMetrics>,
}

impl Metadata {
    fn from_footer(f: &FooterV4) -> Self {
        Metadata {
            original_name: f.original_name.clone(),
            original_names: f.original_names.clone(),
            original_mime: f.original_mime.clone(),
            content_class: f.content_class.clone(),
            plan_reason: f.plan_reason.clone(),
            verified: f.verified,
            created_at: f.created_at,
            updated_at: f.updated_at,
            file_metrics: f.file_metrics.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub file_hash: [u8; 32],
    pub original_size: u64,
    pub file_len: u64,
    pub block_count: usize,
}

pub struct Writer<W: Write> {
    out: W,
    log2: u8,
    plans: Vec<BlockPlan>,
    stored_lens: Vec<u8>,
    original_lens: Vec<u8>,
    plan_indices: Vec<u8>,
    payload_hashes: Vec<u8>,
    blocks: usize,
    hasher: blake3::Hasher,
    original_size: u64,
    payload_bytes: u64,
    sealed_by_short_block: bool,
}

impl<W: Write> Writer<W> {
    pub fn new(mut out: W, block_log2: u8) -> Result<Self, BlockError> {
        check_log2(block_log2)?;
        out.write_all(&MAGIC)?;
        out.write_all(&[FORMAT_VERSION, 0, block_log2])?;
        Ok(Writer {
            out,
            log2: block_log2,
            plans: Vec::new(),
            stored_lens: Vec::new(),
            original_lens: Vec::new(),
            plan_indices: Vec::new(),
            payload_hashes: Vec::new(),
            blocks: 0,
            hasher: blake3::Hasher::new(),
            original_size: 0,
            payload_bytes: 0,
            sealed_by_short_block: false,
        })
    }

    pub fn block_size(&self) -> usize {
        1usize << self.log2
    }

    pub fn block_count(&self) -> usize {
        self.blocks
    }

    pub fn add_plan(&mut self, plan: BlockPlan) -> Result<u16, BlockError> {
        let plan = plan.normalized();
        plan.validate().map_err(|reason| BlockError::PlanInvalid {
            plan: self.plans.len(),
            reason,
        })?;
        if let Some(i) = self.plans.iter().position(|p| *p == plan) {
            return Ok(i as u16);
        }
        if self.plans.len() >= MAX_PLANS {
            return Err(BlockError::InvalidInput(
                "tabela de planos cheia".to_string(),
            ));
        }
        self.plans.push(plan);
        Ok((self.plans.len() - 1) as u16)
    }

    pub fn push_block(
        &mut self,
        original: &[u8],
        plan_idx: u16,
        payload: &[u8],
    ) -> Result<(), BlockError> {
        if self.sealed_by_short_block {
            return Err(BlockError::InvalidInput(
                "só o último bloco pode ser menor que o tamanho de bloco".to_string(),
            ));
        }
        if original.is_empty() {
            return Err(BlockError::InvalidInput("bloco original vazio".to_string()));
        }
        if original.len() > self.block_size() {
            return Err(BlockError::InvalidInput(format!(
                "bloco de {} bytes excede o tamanho de bloco ({})",
                original.len(),
                self.block_size()
            )));
        }
        if plan_idx as usize >= self.plans.len() {
            return Err(BlockError::InvalidInput(format!(
                "plano {plan_idx} não registrado"
            )));
        }
        if payload.len() > slack_len(original.len()) {
            return Err(BlockError::InvalidInput(format!(
                "payload de {} bytes excede o limite para um bloco de {} bytes",
                payload.len(),
                original.len()
            )));
        }
        if self.blocks >= MAX_BLOCKS {
            return Err(BlockError::InvalidInput(format!(
                "limite de {MAX_BLOCKS} blocos atingido"
            )));
        }

        self.out.write_all(payload)?;
        self.hasher.update(original);

        self.stored_lens
            .extend_from_slice(&(payload.len() as u32).to_le_bytes());
        self.original_lens
            .extend_from_slice(&(original.len() as u32).to_le_bytes());
        self.plan_indices.extend_from_slice(&plan_idx.to_le_bytes());
        self.payload_hashes.extend_from_slice(&short_hash(payload));

        self.blocks += 1;
        self.original_size += original.len() as u64;
        self.payload_bytes += payload.len() as u64;
        if original.len() < self.block_size() {
            self.sealed_by_short_block = true;
        }
        Ok(())
    }

    pub fn push_compressed(
        &mut self,
        original: &[u8],
        block: CompressedBlock,
    ) -> Result<u16, BlockError> {
        let idx = self.add_plan(block.plan)?;
        self.push_block(original, idx, &block.payload)?;
        Ok(idx)
    }

    pub fn finish(mut self, meta: &Metadata) -> Result<(W, Summary), BlockError> {
        if meta.original_names.len() > MAX_NAMES {
            return Err(BlockError::InvalidInput(format!(
                "mais de {MAX_NAMES} nomes originais"
            )));
        }

        let file_hash: [u8; 32] = self.hasher.finalize().into();
        let footer = FooterV4 {
            header_flags: 0,
            block_size_log2: self.log2 as u32,
            original_size: self.original_size,
            file_hash: file_hash.to_vec(),
            plans: self.plans.iter().map(BlockPlan::to_proto).collect(),
            block_count: self.blocks as u32,
            stored_lens: std::mem::take(&mut self.stored_lens),
            original_lens: std::mem::take(&mut self.original_lens),
            plan_indices: std::mem::take(&mut self.plan_indices),
            payload_hashes: std::mem::take(&mut self.payload_hashes),
            original_name: meta.original_name.clone(),
            original_names: meta.original_names.clone(),
            original_mime: meta.original_mime.clone(),
            content_class: meta.content_class.clone(),
            plan_reason: meta.plan_reason.clone(),
            verified: meta.verified,
            created_at: meta.created_at,
            updated_at: meta.updated_at,
            file_metrics: meta.file_metrics.clone(),
        };

        let mut footer_bytes = Vec::with_capacity(footer.encoded_len());
        footer
            .encode(&mut footer_bytes)
            .map_err(|e| BlockError::FooterCorrupted(e.to_string()))?;
        if footer_bytes.len() > MAX_FOOTER_LEN {
            return Err(BlockError::FooterTooLarge {
                len: footer_bytes.len() as u64,
                max: MAX_FOOTER_LEN as u64,
            });
        }

        self.out.write_all(&footer_bytes)?;
        self.out
            .write_all(&(footer_bytes.len() as u32).to_le_bytes())?;
        self.out.write_all(&short_hash(&footer_bytes))?;
        self.out.write_all(&MAGIC)?;
        self.out.flush()?;

        let summary = Summary {
            file_hash,
            original_size: self.original_size,
            file_len: (HEADER_LEN + TRAILER_LEN) as u64
                + self.payload_bytes
                + footer_bytes.len() as u64,
            block_count: self.blocks,
        };
        Ok((self.out, summary))
    }
}

pub trait Source {
    fn size(&self) -> u64;
    fn read_at(&mut self, offset: u64, len: usize) -> Result<Cow<'_, [u8]>, BlockError>;
}

pub struct SliceSource<'a>(&'a [u8]);

impl Source for SliceSource<'_> {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }

    fn read_at(&mut self, offset: u64, len: usize) -> Result<Cow<'_, [u8]>, BlockError> {
        let end = offset
            .checked_add(len as u64)
            .filter(|e| *e <= self.0.len() as u64)
            .ok_or(BlockError::Truncated {
                detail: "leitura além do fim do arquivo",
            })?;
        Ok(Cow::Borrowed(&self.0[offset as usize..end as usize]))
    }
}

pub struct StreamSource<R> {
    inner: R,
    len: u64,
}

impl<R: Read + Seek> Source for StreamSource<R> {
    fn size(&self) -> u64 {
        self.len
    }

    fn read_at(&mut self, offset: u64, len: usize) -> Result<Cow<'_, [u8]>, BlockError> {
        offset
            .checked_add(len as u64)
            .filter(|e| *e <= self.len)
            .ok_or(BlockError::Truncated {
                detail: "leitura além do fim do arquivo",
            })?;
        self.inner.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; len];
        self.inner.read_exact(&mut buf)?;
        Ok(Cow::Owned(buf))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockInfo {
    pub index: usize,
    pub offset: u64,
    pub stored_len: u32,
    pub original_len: u32,
    pub plan_idx: u16,
    pub payload_hash: [u8; HASH_LEN],
}

pub struct Reader<S: Source> {
    src: S,
    file_len: u64,
    footer_len: usize,
    log2: u8,
    plans: Vec<BlockPlan>,
    offsets: Vec<u64>,
    footer: FooterV4,
}

type DictCache = Vec<(usize, Arc<Dictionary>)>;

impl<S: Source> fmt::Debug for Reader<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reader")
            .field("blocks", &self.footer.block_count)
            .field("original_size", &self.footer.original_size)
            .field("file_len", &self.file_len)
            .finish()
    }
}

impl<'a> Reader<SliceSource<'a>> {
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self, BlockError> {
        Reader::open(SliceSource(bytes))
    }
}

impl<R: Read + Seek> Reader<StreamSource<R>> {
    pub fn from_reader(mut reader: R) -> Result<Self, BlockError> {
        let len = reader.seek(SeekFrom::End(0))?;
        Reader::open(StreamSource { inner: reader, len })
    }
}

fn wire_varint(buf: &[u8], pos: &mut usize) -> Result<u64, String> {
    let mut value = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = *buf.get(*pos).ok_or("varint truncado")?;
        *pos += 1;
        if shift > 63 {
            return Err("varint fora de faixa".to_string());
        }
        value |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
    }
}

fn scan_fields<F>(buf: &[u8], limits: &[(u32, usize)], mut on_len: F) -> Result<(), String>
where
    F: FnMut(u32, &[u8]) -> Result<(), String>,
{
    let mut counts = vec![0usize; limits.len()];
    let mut pos = 0usize;
    while pos < buf.len() {
        let key = wire_varint(buf, &mut pos)?;
        let field = u32::try_from(key >> 3).map_err(|_| "número de campo fora de faixa")?;
        match key & 7 {
            0 => {
                wire_varint(buf, &mut pos)?;
            }
            1 => pos = skip(buf, pos, 8)?,
            5 => pos = skip(buf, pos, 4)?,
            2 => {
                let len = usize::try_from(wire_varint(buf, &mut pos)?)
                    .map_err(|_| "tamanho de campo fora de faixa")?;
                let end = skip(buf, pos, len)?;
                for (i, (f, max)) in limits.iter().enumerate() {
                    if *f == field {
                        counts[i] += 1;
                        if counts[i] > *max {
                            return Err(format!(
                                "campo {field} repetido mais de {max} vezes"
                            ));
                        }
                    }
                }
                on_len(field, &buf[pos..end])?;
                pos = end;
            }
            other => return Err(format!("tipo de fio {other} inválido")),
        }
    }
    Ok(())
}

fn skip(buf: &[u8], pos: usize, n: usize) -> Result<usize, String> {
    pos.checked_add(n)
        .filter(|e| *e <= buf.len())
        .ok_or_else(|| "campo truncado".to_string())
}

fn scan_footer(buf: &[u8]) -> Result<(), String> {
    scan_fields(buf, &[(5, MAX_PLANS), (12, MAX_NAMES)], |field, slice| {
        if field == 5 {
            scan_fields(slice, &[(1, MAX_CHAIN)], |_, _| Ok(()))
        } else {
            Ok(())
        }
    })
}

fn footer_error(m: &str) -> BlockError {
    BlockError::FooterCorrupted(m.to_string())
}

fn validate_footer(
    footer: &FooterV4,
    flags: u8,
    log2: u8,
    file_len: u64,
    footer_len: usize,
) -> Result<(Vec<BlockPlan>, Vec<u64>), BlockError> {
    if footer.header_flags != flags as u32 {
        return Err(footer_error("flags do cabeçalho divergem do rodapé"));
    }
    if footer.block_size_log2 != log2 as u32 {
        return Err(footer_error(
            "tamanho de bloco do cabeçalho diverge do rodapé",
        ));
    }
    if footer.file_hash.len() != 32 {
        return Err(footer_error("hash do arquivo com tamanho inválido"));
    }
    if footer.plans.len() > MAX_PLANS {
        return Err(footer_error("tabela de planos grande demais"));
    }

    let mut plans = Vec::with_capacity(footer.plans.len());
    for (i, p) in footer.plans.iter().enumerate() {
        plans.push(
            BlockPlan::from_proto(p)
                .map_err(|reason| BlockError::PlanInvalid { plan: i, reason })?,
        );
    }

    let n = footer.block_count as usize;
    if n > MAX_BLOCKS {
        return Err(footer_error("contagem de blocos acima do limite"));
    }
    let widths = [
        (footer.stored_lens.len(), 4usize),
        (footer.original_lens.len(), 4),
        (footer.plan_indices.len(), 2),
        (footer.payload_hashes.len(), HASH_LEN),
    ];
    for (len, width) in widths {
        if len != n * width {
            return Err(footer_error("tabela de blocos com tamanho incoerente"));
        }
    }

    let block_size = 1u64 << log2;
    if footer.original_size.div_ceil(block_size) != n as u64 {
        return Err(BlockError::LayoutInvalid(format!(
            "{n} blocos não correspondem a original_size {} com blocos de {block_size} bytes",
            footer.original_size
        )));
    }
    if n == 0 && footer.file_hash.as_slice() != blake3::hash(&[]).as_bytes().as_slice() {
        return Err(footer_error("arquivo vazio com hash diferente do BLAKE3 do vazio"));
    }

    let mut offsets = Vec::with_capacity(n + 1);
    let mut cursor = HEADER_LEN as u64;
    for i in 0..n {
        let original_len = le_u32(&footer.original_lens, i) as u64;
        let expected = if i + 1 < n {
            block_size
        } else {
            footer.original_size - (n as u64 - 1) * block_size
        };
        if original_len != expected {
            return Err(BlockError::LayoutInvalid(format!(
                "bloco {i}: original_len {original_len} diverge do esperado {expected}"
            )));
        }
        let plan_idx = le_u16(&footer.plan_indices, i) as usize;
        if plan_idx >= plans.len() {
            return Err(BlockError::LayoutInvalid(format!(
                "bloco {i}: plano {plan_idx} fora da tabela de {} planos",
                plans.len()
            )));
        }
        let stored_len = le_u32(&footer.stored_lens, i) as usize;
        if stored_len > slack_len(original_len as usize) {
            return Err(BlockError::LayoutInvalid(format!(
                "bloco {i}: stored_len {stored_len} acima do limite para {original_len} bytes"
            )));
        }
        offsets.push(cursor);
        cursor += stored_len as u64;
    }
    offsets.push(cursor);

    let total = cursor + footer_len as u64 + TRAILER_LEN as u64;
    if total != file_len {
        return Err(BlockError::LayoutInvalid(format!(
            "blocos, cabeçalho, rodapé e final somam {total} bytes, o arquivo tem {file_len}"
        )));
    }
    Ok((plans, offsets))
}

fn fetch<'a, S: Source>(
    src: &'a mut S,
    offsets: &[u64],
    footer: &FooterV4,
    index: usize,
) -> Result<Cow<'a, [u8]>, BlockError> {
    let count = footer.block_count as usize;
    if index >= count {
        return Err(BlockError::BlockOutOfRange { index, count });
    }
    let start = offsets[index];
    let len = le_u32(&footer.stored_lens, index) as usize;
    let data = src.read_at(start, len)?;
    let found = short_hash(&data);
    let expected = footer
        .payload_hashes
        .get(index * HASH_LEN..(index + 1) * HASH_LEN)
        .ok_or_else(|| footer_error("tabela de hashes truncada"))?;
    if found.as_slice() != expected {
        return Err(BlockError::BlockCorrupted {
            index,
            expected: hex::encode(expected),
            found: hex::encode(found),
        });
    }
    Ok(data)
}

fn dictionary_for(
    reference: &DictionaryRef,
    plan_idx: usize,
    block: usize,
    dicts: &dyn DictionaryResolver,
    cache: &mut DictCache,
) -> Result<Arc<Dictionary>, BlockError> {
    if let Some((_, d)) = cache.iter().find(|(i, _)| *i == plan_idx) {
        return Ok(d.clone());
    }
    let found = dicts.resolve(&reference.id).ok_or_else(|| BlockError::DictionaryMissing {
        block,
        id: reference.id.clone(),
    })?;
    if reference.digest.as_slice() != found.digest.as_slice() {
        return Err(BlockError::DictionaryMismatch {
            block,
            id: reference.id.clone(),
        });
    }
    cache.push((plan_idx, found.clone()));
    Ok(found)
}

impl<S: Source> Reader<S> {
    fn open(mut src: S) -> Result<Self, BlockError> {
        let file_len = src.size();
        let head_len = file_len.min(HEADER_LEN as u64) as usize;
        let (version, flags, log2) = {
            let head = src.read_at(0, head_len)?;
            if head.len() >= MAGIC.len() && head[..MAGIC.len()] != MAGIC {
                return Err(BlockError::BadMagic);
            }
            if head.len() < HEADER_LEN {
                return Err(BlockError::Truncated {
                    detail: "cabeçalho incompleto",
                });
            }
            (head[4], head[5], head[6])
        };
        if version != FORMAT_VERSION {
            return Err(BlockError::UnsupportedVersion { found: version });
        }
        if flags & !KNOWN_FLAGS != 0 {
            return Err(BlockError::UnsupportedFlags { found: flags });
        }
        check_log2(log2)?;

        if file_len < (HEADER_LEN + TRAILER_LEN) as u64 {
            return Err(BlockError::Truncated {
                detail: "sem espaço para o final do arquivo",
            });
        }
        let trailer: [u8; TRAILER_LEN] = {
            let t = src.read_at(file_len - TRAILER_LEN as u64, TRAILER_LEN)?;
            <[u8; TRAILER_LEN]>::try_from(&t[..]).map_err(|_| BlockError::Truncated {
                detail: "final do arquivo incompleto",
            })?
        };
        if trailer[20..24] != MAGIC {
            return Err(BlockError::Truncated {
                detail: "o final do arquivo não tem a assinatura SYNT",
            });
        }
        let footer_len = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]) as usize;
        if footer_len > MAX_FOOTER_LEN {
            return Err(BlockError::FooterTooLarge {
                len: footer_len as u64,
                max: MAX_FOOTER_LEN as u64,
            });
        }
        if (HEADER_LEN + TRAILER_LEN) as u64 + footer_len as u64 > file_len {
            return Err(footer_error("tamanho do rodapé excede o arquivo"));
        }

        let footer = {
            let bytes = src.read_at(file_len - TRAILER_LEN as u64 - footer_len as u64, footer_len)?;
            if short_hash(&bytes).as_slice() != &trailer[4..20] {
                return Err(footer_error("hash do rodapé não confere"));
            }
            scan_footer(&bytes).map_err(BlockError::FooterCorrupted)?;
            FooterV4::decode(&*bytes).map_err(|e| BlockError::FooterCorrupted(e.to_string()))?
        };

        let (plans, offsets) = validate_footer(&footer, flags, log2, file_len, footer_len)?;
        Ok(Reader {
            src,
            file_len,
            footer_len,
            log2,
            plans,
            offsets,
            footer,
        })
    }

    pub fn block_count(&self) -> usize {
        self.footer.block_count as usize
    }

    pub fn block_log2(&self) -> u8 {
        self.log2
    }

    pub fn block_size(&self) -> usize {
        1usize << self.log2
    }

    pub fn original_size(&self) -> u64 {
        self.footer.original_size
    }

    pub fn file_len(&self) -> u64 {
        self.file_len
    }

    pub fn footer_len(&self) -> usize {
        self.footer_len
    }

    pub fn file_hash(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        out.copy_from_slice(&self.footer.file_hash);
        out
    }

    pub fn plans(&self) -> &[BlockPlan] {
        &self.plans
    }

    pub fn metadata(&self) -> Metadata {
        Metadata::from_footer(&self.footer)
    }

    pub fn block_info(&self, index: usize) -> Option<BlockInfo> {
        if index >= self.block_count() {
            return None;
        }
        let mut payload_hash = [0u8; HASH_LEN];
        payload_hash
            .copy_from_slice(&self.footer.payload_hashes[index * HASH_LEN..(index + 1) * HASH_LEN]);
        Some(BlockInfo {
            index,
            offset: self.offsets[index],
            stored_len: le_u32(&self.footer.stored_lens, index),
            original_len: le_u32(&self.footer.original_lens, index),
            plan_idx: le_u16(&self.footer.plan_indices, index),
            payload_hash,
        })
    }

    pub fn read_block(
        &mut self,
        index: usize,
        dicts: &dyn DictionaryResolver,
    ) -> Result<Vec<u8>, BlockError> {
        let mut cache = DictCache::new();
        self.read_cached(index, dicts, &mut cache)
    }

    fn read_cached(
        &mut self,
        index: usize,
        dicts: &dyn DictionaryResolver,
        cache: &mut DictCache,
    ) -> Result<Vec<u8>, BlockError> {
        let Reader {
            src,
            footer,
            offsets,
            plans,
            ..
        } = &mut *self;

        let payload = fetch(src, offsets, footer, index)?;
        let plan_idx = le_u16(&footer.plan_indices, index) as usize;
        let plan = plans.get(plan_idx).ok_or_else(|| {
            BlockError::LayoutInvalid(format!("bloco {index}: plano {plan_idx} fora da tabela"))
        })?;
        let dict = match &plan.dictionary {
            None => None,
            Some(reference) => Some(dictionary_for(reference, plan_idx, index, dicts, cache)?),
        };
        let original_len = le_u32(&footer.original_lens, index) as usize;

        decode_block(
            &payload,
            plan,
            dict.as_deref().map(|d| d.bytes.as_slice()),
            original_len,
        )
        .map_err(|e| BlockError::BlockDecode {
            index,
            reason: e.to_string(),
        })
    }

    pub fn restore_to<Wr: Write>(
        &mut self,
        dicts: &dyn DictionaryResolver,
        out: &mut Wr,
    ) -> Result<u64, BlockError> {
        let mut hasher = blake3::Hasher::new();
        let mut cache = DictCache::new();
        let mut written = 0u64;

        for index in 0..self.block_count() {
            let block = self.read_cached(index, dicts, &mut cache)?;
            hasher.update(&block);
            out.write_all(&block)?;
            written += block.len() as u64;
        }

        let found: [u8; 32] = hasher.finalize().into();
        if written != self.footer.original_size
            || found.as_slice() != self.footer.file_hash.as_slice()
        {
            return Err(BlockError::Fidelity {
                expected: hex::encode(&self.footer.file_hash),
                found: hex::encode(found),
            });
        }
        Ok(written)
    }

    pub fn restore_all(&mut self, dicts: &dyn DictionaryResolver) -> Result<Vec<u8>, BlockError> {
        let reserve = self.footer.original_size.min(RESTORE_RESERVE_MAX) as usize;
        let mut out = Vec::with_capacity(reserve);
        self.restore_to(dicts, &mut out)?;
        Ok(out)
    }

    pub fn verify_all(&mut self) -> Result<(), BlockError> {
        for index in 0..self.block_count() {
            let Reader {
                src,
                footer,
                offsets,
                ..
            } = &mut *self;
            fetch(src, offsets, footer, index)?;
        }
        Ok(())
    }

    pub fn corrupted_blocks(&mut self) -> Result<Vec<usize>, BlockError> {
        let mut bad = Vec::new();
        for index in 0..self.block_count() {
            let Reader {
                src,
                footer,
                offsets,
                ..
            } = &mut *self;
            match fetch(src, offsets, footer, index) {
                Ok(_) => {}
                Err(BlockError::BlockCorrupted { index, .. }) => bad.push(index),
                Err(other) => return Err(other),
            }
        }
        Ok(bad)
    }
}

#[cfg(test)]
#[path = "blockfile_tests.rs"]
mod tests;
