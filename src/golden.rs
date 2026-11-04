use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use prost::Message;
use serde::{Deserialize, Serialize};

use crate::codec::{self, Codec};
use crate::container::{self, ContainerError, DictionaryResolver, NoDictionaries};
use crate::content;
use crate::dict_store::Dictionary;
use crate::planner::{self, Effort, ForcedPlan, Limits, Plan, Planned};
use crate::proto::{Algorithm, ProcessedFile};
use crate::transform::{self, Transform};

const LIMITE_ORIGINAL: usize = 64 * 1024;
const LIMITE_TOTAL: u64 = 2 * 1024 * 1024;
const VARIAVEL_SAIDA: &str = "SYNTRA_GOLDEN_OUT";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Caso {
    nome: String,
    original: String,
    envelope: String,
    versao: i32,
    plano: String,
    tamanho: u64,
    blake3: String,
    envelope_tamanho: u64,
    envelope_blake3: String,
    dicionario: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EntradaDicionario {
    id: String,
    arquivo: String,
    tamanho: u64,
    blake3: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifesto {
    formato: u32,
    dicionarios: Vec<EntradaDicionario>,
    casos: Vec<Caso>,
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }

    fn proximo(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn abaixo(&mut self, n: u64) -> u64 {
        self.proximo() % n
    }

    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| (self.proximo() >> 24) as u8).collect()
    }
}

const PALAVRAS: [&str; 40] = [
    "o", "motor", "comprime", "dados", "sem", "perdas", "e", "reconstroi", "cada", "byte",
    "do", "arquivo", "original", "com", "integridade", "verificada", "por", "hash", "blake3",
    "no", "cofre", "de", "essencias", "a", "compressao", "ocorre", "em", "blocos", "independentes",
    "ação", "rápida", "cliente", "armazenamento", "custo", "menor", "vazão", "alta", "plano",
    "medido", "estável",
];

fn texto() -> Vec<u8> {
    let mut rng = Rng::new(0x5EED_0001);
    let mut s = String::new();
    while s.len() < 48 * 1024 - 256 {
        let n = 6 + rng.abaixo(10) as usize;
        for i in 0..n {
            let w = PALAVRAS[rng.abaixo(PALAVRAS.len() as u64) as usize];
            if i == 0 {
                let mut cs = w.chars();
                if let Some(c) = cs.next() {
                    s.extend(c.to_uppercase());
                    s.push_str(cs.as_str());
                }
            } else {
                s.push(' ');
                s.push_str(w);
            }
        }
        s.push_str(if rng.abaixo(5) == 0 { ";\n" } else { ".\n" });
    }
    s.into_bytes()
}

fn csv() -> Vec<u8> {
    let mut rng = Rng::new(0x5EED_0002);
    let regioes = ["sudeste", "sul", "norte", "nordeste", "oeste"];
    let mut s = String::from("id,data,regiao,produto,valor,quantidade\n");
    let mut valor: u64 = 10_000;
    for i in 0..1100u64 {
        valor += rng.abaixo(900);
        s.push_str(&format!(
            "{},2026-{:02}-{:02},{},produto-{},{}.{:02},{}\n",
            i,
            1 + rng.abaixo(12),
            1 + rng.abaixo(28),
            regioes[rng.abaixo(regioes.len() as u64) as usize],
            rng.abaixo(60),
            valor / 100,
            valor % 100,
            1 + rng.abaixo(40)
        ));
    }
    s.into_bytes()
}

fn linha_evento(rng: &mut Rng, i: u64) -> String {
    format!(
        r#"{{"evento":"pedido.criado","id":{},"cliente":"acme-{}","itens":[{{"sku":"A-{}","qtd":{}}}],"status":"pendente","canal":"api","ts":{}}}"#,
        i,
        rng.abaixo(40),
        rng.abaixo(90),
        1 + rng.abaixo(7),
        1_790_000_000_000u64 + i * 113 + rng.abaixo(50)
    )
}

fn ndjson() -> Vec<u8> {
    let mut rng = Rng::new(0x5EED_0003);
    let mut s = String::new();
    for i in 0..300u64 {
        s.push_str(&linha_evento(&mut rng, i));
        s.push('\n');
    }
    s.into_bytes()
}

fn f32_sensor() -> Vec<u8> {
    let mut rng = Rng::new(0x5EED_0004);
    let mut out = Vec::with_capacity(16384 * 4);
    for i in 0..16384u32 {
        let base = 20.0f64 + 5.0 * (i as f64 / 200.0).sin();
        let ruido = (rng.abaixo(2000) as f64 - 1000.0) / 100_000.0;
        out.extend_from_slice(&((base + ruido) as f32).to_le_bytes());
    }
    out
}

fn i64_serie() -> Vec<u8> {
    let mut rng = Rng::new(0x5EED_0005);
    let mut out = Vec::with_capacity(8192 * 8);
    for i in 0..8192i64 {
        let v = 1_700_000_000_000i64 + i * 37 + rng.abaixo(5) as i64;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

fn pseudo_executavel() -> Vec<u8> {
    let mut rng = Rng::new(0x5EED_0006);
    let mut out: Vec<u8> = vec![0x7F, b'E', b'L', b'F', 2, 1, 1, 0];
    out.extend_from_slice(&[0u8; 8]);
    out.extend_from_slice(&[3, 0, 0x3E, 0, 1, 0, 0, 0]);
    out.extend_from_slice(&rng.bytes(40));
    let padroes: [&[u8]; 12] = [
        &[0x48, 0x89, 0xE5],
        &[0x48, 0x83, 0xEC],
        &[0x48, 0x8B, 0x45],
        &[0x48, 0x89, 0x7D],
        &[0xE8],
        &[0x48, 0x8D, 0x05],
        &[0x0F, 0x1F, 0x44, 0x00],
        &[0x85, 0xC0],
        &[0x74],
        &[0xC9, 0xC3],
        &[0x55],
        &[0x5D],
    ];
    while out.len() < 44 * 1024 {
        let p = padroes[rng.abaixo(padroes.len() as u64) as usize];
        out.extend_from_slice(p);
        if rng.abaixo(3) == 0 {
            out.extend_from_slice(&(rng.abaixo(4096) as u32).to_le_bytes());
        }
        if out.len() % 16 == 0 || rng.abaixo(9) == 0 {
            while out.len() % 16 != 0 {
                out.push(0xCC);
            }
        }
    }
    for i in 0..400u32 {
        out.extend_from_slice(format!("simbolo_{}\0", i * 7).as_bytes());
    }
    for i in 0..300u64 {
        out.extend_from_slice(&((i * 11) as u32).to_le_bytes());
        out.extend_from_slice(&[0x12, 0, 1, 0]);
        out.extend_from_slice(&(0x1000 + i * 48).to_le_bytes());
    }
    out.extend_from_slice(&[0u8; 2048]);
    out
}

fn aleatorio() -> Vec<u8> {
    Rng::new(0x5EED_0007).bytes(32 * 1024)
}

fn corridas() -> Vec<u8> {
    let mut rng = Rng::new(0x5EED_0008);
    let mut out = Vec::new();
    while out.len() < 40 * 1024 {
        let b = (rng.proximo() >> 32) as u8;
        let n = 1 + rng.abaixo(300) as usize;
        out.extend(std::iter::repeat_n(b, n));
    }
    out
}

fn treinar_dicionario() -> Arc<Dictionary> {
    let mut rng = Rng::new(0x5EED_0009);
    let amostras: Vec<Vec<u8>> = (0..400u64)
        .map(|i| linha_evento(&mut rng, 10_000 + i).into_bytes())
        .collect();
    let refs: Vec<&[u8]> = amostras.iter().map(|a| a.as_slice()).collect();
    let bytes = zstd::dict::from_samples(&refs, 6 * 1024).expect("treino do dicionário falhou");
    let digest: [u8; 32] = blake3::hash(&bytes).into();
    let id = format!("application_x-ndjson.{}", hex::encode(&digest[..8]));
    Arc::new(Dictionary { id, digest, bytes })
}

fn plano(passos: &[(Transform, u32)], codec: Codec, nivel: i32, dicionario: bool) -> Plan {
    Plan {
        transforms: passos
            .iter()
            .map(|(t, w)| transform::step(*t, *w, 0))
            .collect(),
        codec,
        level: codec::clamp_level(codec, nivel),
        use_dictionary: dicionario,
    }
}

fn planejado(dados: &[u8], mut plan: Plan, dict: Option<&Dictionary>) -> Planned {
    let (payload, steps) = planner::execute(dados, &plan, dict.map(|d| d.bytes.as_slice()))
        .expect("falha ao executar o plano");
    plan.transforms = steps;
    let reason = format!("corpus dourado: {}", plan.label());
    Planned {
        plan,
        payload,
        candidates_tried: 1,
        sampled_decision: false,
        reason,
    }
}

fn automatico(dados: &[u8], nome: &str, esforco: Effort) -> Planned {
    let profile = content::classify(dados, nome, None);
    planner::compress_best(
        dados,
        &profile,
        esforco,
        None,
        &ForcedPlan::default(),
        &Limits::default(),
    )
    .expect("falha no planejamento automático")
}

fn selar(
    dados: &[u8],
    nome: &str,
    planned: Planned,
    dict: Option<&Dictionary>,
) -> (Vec<u8>, String) {
    let profile = content::classify(dados, nome, None);
    let hash: [u8; 32] = blake3::hash(dados).into();
    let rotulo = planned.plan.label();
    let rt = container::verify_roundtrip(&planned, dados, dict.map(|d| d.bytes.as_slice()))
        .expect("verificação do round-trip falhou");
    let env = container::build(planned, &profile, nome, &hash, dict, 1.0, true, rt);
    let sealed = container::seal(env).expect("falha ao selar");
    (sealed.bytes, rotulo)
}

fn legado(
    nome: &str,
    dados: &[u8],
    algoritmo: Algorithm,
    codec_id: Codec,
    nivel: i32,
    com_checksum: bool,
    dict: Option<&Dictionary>,
) -> (Vec<u8>, String) {
    let payload = codec::encode(dados, codec_id, nivel, dict.map(|d| d.bytes.as_slice()))
        .expect("falha ao codificar o payload legado");
    let hash: [u8; 32] = blake3::hash(dados).into();
    let checksum = if com_checksum {
        u64::from_le_bytes(blake3::hash(&payload).as_bytes()[..8].try_into().unwrap())
    } else {
        0
    };
    let rotulo = format!(
        "v2 legado: {}{}",
        codec::name(codec_id),
        if dict.is_some() { "+dict(dictionary_id)" } else { "" }
    );
    let env = ProcessedFile {
        version: 2,
        original_name: nome.to_string(),
        original_mime: content::classify(dados, nome, None).mime,
        original_size: dados.len() as u64,
        processed_size: payload.len() as u64,
        algorithm: algoritmo as i32,
        compression_level: nivel,
        dictionary_id: dict.map(|d| d.id.clone()).unwrap_or_default(),
        data: payload,
        file_hash: hash.to_vec(),
        envelope_checksum: checksum,
        ..Default::default()
    };
    let mut bytes = Vec::new();
    env.encode(&mut bytes).expect("falha ao serializar v2");
    (bytes, rotulo)
}

struct Construido {
    originais: Vec<(String, Vec<u8>)>,
    dicionario: Arc<Dictionary>,
    casos: Vec<(Caso, Vec<u8>)>,
}

impl Construido {
    fn registrar(
        &mut self,
        nome: &str,
        original: &str,
        dados: &[u8],
        versao: i32,
        envelope: (Vec<u8>, String),
        dicionario: Option<&str>,
    ) {
        let (bytes, plano) = envelope;
        let caso = Caso {
            nome: nome.to_string(),
            original: format!("originais/{original}"),
            envelope: format!("envelopes/{nome}.v{versao}.envelope"),
            versao,
            plano,
            tamanho: dados.len() as u64,
            blake3: hex::encode(blake3::hash(dados).as_bytes()),
            envelope_tamanho: bytes.len() as u64,
            envelope_blake3: hex::encode(blake3::hash(&bytes).as_bytes()),
            dicionario: dicionario.map(|s| s.to_string()),
        };
        self.casos.push((caso, bytes));
    }

    fn v3(&mut self, nome: &str, original: &str, dados: &[u8], p: Plan) {
        let envelope = selar(dados, original, planejado(dados, p, None), None);
        self.registrar(nome, original, dados, 3, envelope, None);
    }

    fn auto(&mut self, nome: &str, original: &str, dados: &[u8], esforco: Effort) {
        let envelope = selar(dados, original, automatico(dados, original, esforco), None);
        self.registrar(nome, original, dados, 3, envelope, None);
    }
}

fn construir() -> Construido {
    let originais: Vec<(String, Vec<u8>)> = vec![
        ("texto.txt".to_string(), texto()),
        ("tabela.csv".to_string(), csv()),
        ("eventos.ndjson".to_string(), ndjson()),
        ("sensor_f32.bin".to_string(), f32_sensor()),
        ("serie_i64.bin".to_string(), i64_serie()),
        ("pseudo_exec.bin".to_string(), pseudo_executavel()),
        ("aleatorio.bin".to_string(), aleatorio()),
        ("corridas.bin".to_string(), corridas()),
        ("vazio.bin".to_string(), Vec::new()),
        ("um_byte.bin".to_string(), vec![0xA5]),
    ];
    for (nome, dados) in &originais {
        assert!(dados.len() <= LIMITE_ORIGINAL, "{nome} excede 64 KiB");
    }
    let por_nome = |n: &str| -> Vec<u8> {
        originais
            .iter()
            .find(|(nome, _)| nome == n)
            .map(|(_, d)| d.clone())
            .unwrap()
    };
    let texto = por_nome("texto.txt");
    let tabela = por_nome("tabela.csv");
    let eventos = por_nome("eventos.ndjson");
    let sensor = por_nome("sensor_f32.bin");
    let serie = por_nome("serie_i64.bin");
    let exec = por_nome("pseudo_exec.bin");
    let rand = por_nome("aleatorio.bin");
    let runs = por_nome("corridas.bin");
    let vazio = por_nome("vazio.bin");
    let um = por_nome("um_byte.bin");

    let dicionario = treinar_dicionario();
    let mut c = Construido {
        originais: originais.clone(),
        dicionario: dicionario.clone(),
        casos: Vec::new(),
    };

    let codecs = [
        ("stored", Codec::Stored, 0),
        ("lz4", Codec::Lz4, 0),
        ("deflate6", Codec::Deflate, 6),
        ("zstd3", Codec::Zstd, 3),
        ("zstd19", Codec::Zstd, 19),
        ("bzip2_9", Codec::Bzip2, 9),
        ("brotli11", Codec::Brotli, 11),
        ("xz6", Codec::Xz, 6),
    ];
    for (nome, codec_id, nivel) in codecs {
        c.v3(
            &format!("texto.{nome}"),
            "texto.txt",
            &texto,
            plano(&[], codec_id, nivel, false),
        );
    }

    c.v3(
        "serie_i64.delta8_zstd3",
        "serie_i64.bin",
        &serie,
        plano(&[(Transform::Delta, 8)], Codec::Zstd, 3, false),
    );
    c.v3(
        "serie_i64.delta8_bytesplit8_zstd19",
        "serie_i64.bin",
        &serie,
        plano(
            &[(Transform::Delta, 8), (Transform::ByteSplit, 8)],
            Codec::Zstd,
            19,
            false,
        ),
    );
    c.v3(
        "serie_i64.bytesplit8_xz6",
        "serie_i64.bin",
        &serie,
        plano(&[(Transform::ByteSplit, 8)], Codec::Xz, 6, false),
    );
    c.v3(
        "serie_i64.delta4_lz4",
        "serie_i64.bin",
        &serie,
        plano(&[(Transform::Delta, 4)], Codec::Lz4, 0, false),
    );
    c.v3(
        "sensor_f32.bytesplit4_zstd3",
        "sensor_f32.bin",
        &sensor,
        plano(&[(Transform::ByteSplit, 4)], Codec::Zstd, 3, false),
    );
    c.v3(
        "sensor_f32.bytesplit4_brotli9",
        "sensor_f32.bin",
        &sensor,
        plano(&[(Transform::ByteSplit, 4)], Codec::Brotli, 9, false),
    );
    c.v3(
        "sensor_f32.delta2_deflate6",
        "sensor_f32.bin",
        &sensor,
        plano(&[(Transform::Delta, 2)], Codec::Deflate, 6, false),
    );
    c.v3(
        "sensor_f32.delta1_bytesplit4_zstd12",
        "sensor_f32.bin",
        &sensor,
        plano(
            &[(Transform::Delta, 1), (Transform::ByteSplit, 4)],
            Codec::Zstd,
            12,
            false,
        ),
    );
    c.v3(
        "corridas.rle_zstd3",
        "corridas.bin",
        &runs,
        plano(&[(Transform::Rle, 1)], Codec::Zstd, 3, false),
    );
    c.v3(
        "corridas.rle_deflate6",
        "corridas.bin",
        &runs,
        plano(&[(Transform::Rle, 1)], Codec::Deflate, 6, false),
    );
    c.v3(
        "corridas.rle_stored",
        "corridas.bin",
        &runs,
        plano(&[(Transform::Rle, 1)], Codec::Stored, 0, false),
    );
    c.v3(
        "tabela.csv_columnar_zstd19",
        "tabela.csv",
        &tabela,
        plano(&[(Transform::CsvColumnar, 1)], Codec::Zstd, 19, false),
    );
    c.v3(
        "tabela.csv_columnar_delta1_brotli9",
        "tabela.csv",
        &tabela,
        plano(
            &[(Transform::CsvColumnar, 1), (Transform::Delta, 1)],
            Codec::Brotli,
            9,
            false,
        ),
    );
    c.v3(
        "tabela.csv_columnar_xz6",
        "tabela.csv",
        &tabela,
        plano(&[(Transform::CsvColumnar, 1)], Codec::Xz, 6, false),
    );
    c.v3(
        "tabela.zstd12",
        "tabela.csv",
        &tabela,
        plano(&[], Codec::Zstd, 12, false),
    );
    c.v3(
        "pseudo_exec.zstd19",
        "pseudo_exec.bin",
        &exec,
        plano(&[], Codec::Zstd, 19, false),
    );
    c.v3(
        "pseudo_exec.bytesplit4_zstd12",
        "pseudo_exec.bin",
        &exec,
        plano(&[(Transform::ByteSplit, 4)], Codec::Zstd, 12, false),
    );
    c.v3(
        "pseudo_exec.xz6",
        "pseudo_exec.bin",
        &exec,
        plano(&[], Codec::Xz, 6, false),
    );
    c.v3(
        "eventos.zstd12",
        "eventos.ndjson",
        &eventos,
        plano(&[], Codec::Zstd, 12, false),
    );

    let dict_id = dicionario.id.clone();
    let envelope = selar(
        &eventos,
        "eventos.ndjson",
        planejado(&eventos, plano(&[], Codec::Zstd, 12, true), Some(&dicionario)),
        Some(&dicionario),
    );
    c.registrar(
        "eventos.zstd12_dicionario",
        "eventos.ndjson",
        &eventos,
        3,
        envelope,
        Some(&dict_id),
    );
    let envelope = selar(
        &eventos,
        "eventos.ndjson",
        planejado(&eventos, plano(&[], Codec::Zstd, 3, true), Some(&dicionario)),
        Some(&dicionario),
    );
    c.registrar(
        "eventos.zstd3_dicionario",
        "eventos.ndjson",
        &eventos,
        3,
        envelope,
        Some(&dict_id),
    );

    c.v3("vazio.zstd3", "vazio.bin", &vazio, plano(&[], Codec::Zstd, 3, false));
    c.v3("vazio.xz6", "vazio.bin", &vazio, plano(&[], Codec::Xz, 6, false));
    c.v3(
        "vazio.rle_zstd3",
        "vazio.bin",
        &vazio,
        plano(&[(Transform::Rle, 1)], Codec::Zstd, 3, false),
    );
    c.v3("um_byte.zstd3", "um_byte.bin", &um, plano(&[], Codec::Zstd, 3, false));
    c.v3("um_byte.lz4", "um_byte.bin", &um, plano(&[], Codec::Lz4, 0, false));
    c.v3("um_byte.bzip2_9", "um_byte.bin", &um, plano(&[], Codec::Bzip2, 9, false));
    c.v3(
        "um_byte.brotli11",
        "um_byte.bin",
        &um,
        plano(&[], Codec::Brotli, 11, false),
    );
    c.v3(
        "um_byte.delta8_zstd3",
        "um_byte.bin",
        &um,
        plano(&[(Transform::Delta, 8)], Codec::Zstd, 3, false),
    );

    let todos: Vec<(String, Vec<u8>)> = c.originais.clone();
    for (original, dados) in &todos {
        let base = original.split('.').next().unwrap_or(original).to_string();
        c.auto(&format!("{base}.auto_balanced"), original, dados, Effort::Balanced);
    }
    for original in ["texto.txt", "tabela.csv", "sensor_f32.bin", "serie_i64.bin", "pseudo_exec.bin"] {
        let dados = por_nome(original);
        let base = original.split('.').next().unwrap_or(original).to_string();
        c.auto(&format!("{base}.auto_max"), original, &dados, Effort::Max);
        c.auto(&format!("{base}.auto_fast"), original, &dados, Effort::Fast);
    }

    let envelope = legado("texto.txt", &texto, Algorithm::Lz4, Codec::Lz4, 0, false, None);
    c.registrar("texto.v2_lz4", "texto.txt", &texto, 2, envelope, None);
    let envelope = legado(
        "tabela.csv",
        &tabela,
        Algorithm::ZstdBalanced,
        Codec::Zstd,
        3,
        true,
        None,
    );
    c.registrar("tabela.v2_zstd3_checksum", "tabela.csv", &tabela, 2, envelope, None);
    let envelope = legado(
        "eventos.ndjson",
        &eventos,
        Algorithm::ZstdBalanced,
        Codec::Zstd,
        12,
        true,
        Some(&dicionario),
    );
    c.registrar(
        "eventos.v2_zstd12_dicionario",
        "eventos.ndjson",
        &eventos,
        2,
        envelope,
        Some(&dict_id),
    );
    let envelope = legado(
        "aleatorio.bin",
        &rand,
        Algorithm::Passthrough,
        Codec::Stored,
        0,
        false,
        None,
    );
    c.registrar("aleatorio.v2_passthrough", "aleatorio.bin", &rand, 2, envelope, None);

    c
}

fn escrever(caminho: &Path, bytes: &[u8]) {
    if let Some(pai) = caminho.parent() {
        fs::create_dir_all(pai).expect("falha ao criar diretório");
    }
    fs::write(caminho, bytes).expect("falha ao gravar fixture");
}

fn diretorio_dourado() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/testdata/golden")
}

struct Dicionarios(Vec<Arc<Dictionary>>);

impl DictionaryResolver for Dicionarios {
    fn resolve(&self, id: &str) -> Option<Arc<Dictionary>> {
        self.0.iter().find(|d| d.id == id).cloned()
    }
}

fn carregar_manifesto(dir: &Path) -> Manifesto {
    let bytes = fs::read(dir.join("manifest.json")).expect("manifest.json ausente");
    serde_json::from_slice(&bytes).expect("manifest.json inválido")
}

fn carregar_dicionarios(dir: &Path, manifesto: &Manifesto) -> Dicionarios {
    let mut lista = Vec::new();
    for entrada in &manifesto.dicionarios {
        let bytes = fs::read(dir.join(&entrada.arquivo)).expect("dicionário ausente");
        assert_eq!(bytes.len() as u64, entrada.tamanho);
        let digest: [u8; 32] = blake3::hash(&bytes).into();
        assert_eq!(hex::encode(digest), entrada.blake3);
        assert!(entrada.id.ends_with(&hex::encode(&digest[..8])));
        lista.push(Arc::new(Dictionary {
            id: entrada.id.clone(),
            digest,
            bytes,
        }));
    }
    Dicionarios(lista)
}

#[test]
#[ignore]
fn gera_corpus() {
    let saida = PathBuf::from(
        std::env::var(VARIAVEL_SAIDA).unwrap_or_else(|_| panic!("defina {VARIAVEL_SAIDA}")),
    );
    let corpus = construir();

    let mut total: u64 = 0;
    for (nome, dados) in &corpus.originais {
        escrever(&saida.join("originais").join(nome), dados);
        total += dados.len() as u64;
    }

    let arquivo_dict = format!("dicionarios/{}.zdict", corpus.dicionario.id);
    escrever(&saida.join(&arquivo_dict), &corpus.dicionario.bytes);
    total += corpus.dicionario.bytes.len() as u64;

    let mut casos = Vec::new();
    for (caso, bytes) in &corpus.casos {
        escrever(&saida.join(&caso.envelope), bytes);
        total += bytes.len() as u64;
        casos.push(caso.clone());
    }

    let manifesto = Manifesto {
        formato: 1,
        dicionarios: vec![EntradaDicionario {
            id: corpus.dicionario.id.clone(),
            arquivo: arquivo_dict,
            tamanho: corpus.dicionario.bytes.len() as u64,
            blake3: hex::encode(corpus.dicionario.digest),
        }],
        casos,
    };
    let json = serde_json::to_string_pretty(&manifesto).expect("falha ao serializar manifesto");
    escrever(&saida.join("manifest.json"), json.as_bytes());
    total += json.len() as u64;

    assert!(total <= LIMITE_TOTAL, "corpus de {total} bytes excede 2 MiB");
    println!("corpus gerado em {}: {} casos, {} bytes", saida.display(), manifesto.casos.len(), total);
}

#[test]
fn restaura_todo_o_corpus() {
    let dir = diretorio_dourado();
    let manifesto = carregar_manifesto(&dir);
    let dicts = carregar_dicionarios(&dir, &manifesto);
    assert!(manifesto.casos.len() >= 40, "corpus pequeno demais: {}", manifesto.casos.len());

    let mut codecs: BTreeSet<&'static str> = BTreeSet::new();
    let mut transforms: BTreeSet<&'static str> = BTreeSet::new();
    let mut versoes: BTreeSet<i32> = BTreeSet::new();
    let mut com_dicionario = 0usize;

    for caso in &manifesto.casos {
        let original = fs::read(dir.join(&caso.original))
            .unwrap_or_else(|e| panic!("{}: original ilegível: {e}", caso.nome));
        assert_eq!(original.len() as u64, caso.tamanho, "{}: tamanho", caso.nome);
        assert_eq!(
            hex::encode(blake3::hash(&original).as_bytes()),
            caso.blake3,
            "{}: fixture do original alterada",
            caso.nome
        );

        let bytes = fs::read(dir.join(&caso.envelope))
            .unwrap_or_else(|e| panic!("{}: envelope ilegível: {e}", caso.nome));
        assert_eq!(bytes.len() as u64, caso.envelope_tamanho, "{}: tamanho do envelope", caso.nome);
        assert_eq!(
            hex::encode(blake3::hash(&bytes).as_bytes()),
            caso.envelope_blake3,
            "{}: fixture do envelope alterada",
            caso.nome
        );

        let envelope = container::parse(&bytes)
            .unwrap_or_else(|e| panic!("{}: parse falhou: {e}", caso.nome));
        assert_eq!(envelope.version, caso.versao, "{}: versão", caso.nome);

        let restaurado = container::restore(&envelope, &dicts)
            .unwrap_or_else(|e| panic!("{}: restore falhou: {e}", caso.nome));
        assert_eq!(restaurado.data, original, "{}: bytes divergentes", caso.nome);
        assert_eq!(
            hex::encode(blake3::hash(&restaurado.data).as_bytes()),
            caso.blake3,
            "{}: BLAKE3 divergente",
            caso.nome
        );
        assert!(restaurado.fidelity_checked, "{}: fidelidade não conferida", caso.nome);

        versoes.insert(envelope.version);
        if envelope.dictionary.is_some() || !envelope.dictionary_id.is_empty() {
            com_dicionario += 1;
        }
        let codec_id = if envelope.version >= 3 {
            Codec::try_from(envelope.codec).expect("codec inválido")
        } else {
            let legado = Algorithm::try_from(envelope.algorithm).expect("algoritmo inválido");
            codec::from_legacy(legado, envelope.compression_level).0
        };
        codecs.insert(codec::name(codec_id));
        for passo in &envelope.transforms {
            let t = Transform::try_from(passo.transform).expect("transform inválido");
            transforms.insert(transform::transform_name(t));
        }
    }

    for esperado in ["stored", "lz4", "deflate", "zstd", "bzip2", "brotli", "xz"] {
        assert!(codecs.contains(esperado), "corpus não cobre o codec {esperado}");
    }
    for esperado in ["delta", "byte_split", "rle", "csv_columnar"] {
        assert!(transforms.contains(esperado), "corpus não cobre o transform {esperado}");
    }
    assert!(versoes.contains(&2) && versoes.contains(&3), "faltam versões: {versoes:?}");
    assert!(com_dicionario >= 3, "poucos casos com dicionário: {com_dicionario}");
}

#[test]
fn corpus_respeita_os_limites_de_tamanho() {
    let dir = diretorio_dourado();
    let manifesto = carregar_manifesto(&dir);
    let mut total: u64 = fs::metadata(dir.join("manifest.json")).unwrap().len();
    let mut vistos = BTreeSet::new();
    for caso in &manifesto.casos {
        if vistos.insert(caso.original.clone()) {
            let n = fs::metadata(dir.join(&caso.original)).unwrap().len();
            assert!(n <= LIMITE_ORIGINAL as u64, "{} excede 64 KiB", caso.original);
            total += n;
        }
        total += fs::metadata(dir.join(&caso.envelope)).unwrap().len();
    }
    for d in &manifesto.dicionarios {
        total += fs::metadata(dir.join(&d.arquivo)).unwrap().len();
    }
    assert!(total <= LIMITE_TOTAL, "corpus de {total} bytes excede 2 MiB");
}

#[test]
fn corpus_com_dicionario_ausente_ou_trocado_falha_com_erro_claro() {
    let dir = diretorio_dourado();
    let manifesto = carregar_manifesto(&dir);
    let dicts = carregar_dicionarios(&dir, &manifesto);
    let caso = manifesto
        .casos
        .iter()
        .find(|c| c.versao == 3 && c.dicionario.is_some())
        .expect("sem caso v3 com dicionário");
    let bytes = fs::read(dir.join(&caso.envelope)).unwrap();
    let envelope = container::parse(&bytes).unwrap();

    match container::restore(&envelope, &NoDictionaries) {
        Err(ContainerError::MissingDictionary { id }) => {
            assert_eq!(Some(&id), caso.dicionario.as_ref());
        }
        outro => panic!("esperava MissingDictionary, veio {:?}", outro.map(|r| r.data.len())),
    }

    let original = &dicts.0[0];
    let trocado = Dictionary {
        id: original.id.clone(),
        digest: [9u8; 32],
        bytes: original.bytes.clone(),
    };
    let resolvedor = Dicionarios(vec![Arc::new(trocado)]);
    match container::restore(&envelope, &resolvedor) {
        Err(ContainerError::DictionaryMismatch { .. }) => {}
        outro => panic!("esperava DictionaryMismatch, veio {:?}", outro.map(|r| r.data.len())),
    }
}
