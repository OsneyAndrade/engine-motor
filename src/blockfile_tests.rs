use std::collections::BTreeSet;
use std::io::Cursor;

use super::*;
use crate::codec::CATALOG;
use crate::container::{self, ContainerError, NoDictionaries};
use crate::proto::{ProcessedFile, TransformStep};

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

    fn bytes_ate(&mut self, max: u64) -> Vec<u8> {
        let n = self.abaixo(max) as usize;
        self.bytes(n)
    }
}

struct Dicts(Vec<Arc<Dictionary>>);

impl DictionaryResolver for Dicts {
    fn resolve(&self, id: &str) -> Option<Arc<Dictionary>> {
        self.0.iter().find(|d| d.id == id).cloned()
    }
}

fn erro<T>(r: Result<T, BlockError>) -> BlockError {
    match r {
        Ok(_) => panic!("esperava erro, veio Ok"),
        Err(e) => e,
    }
}

fn hash_de(dados: &[u8]) -> [u8; 32] {
    blake3::hash(dados).into()
}

fn plano(codec_id: Codec, nivel: i32, passos: &[(Transform, u32)]) -> Plan {
    Plan {
        transforms: passos
            .iter()
            .map(|(t, w)| crate::transform::step(*t, *w, 0))
            .collect(),
        codec: codec_id,
        level: nivel,
        use_dictionary: false,
    }
}

fn texto(n: usize, rng: &mut Rng) -> Vec<u8> {
    const PALAVRAS: [&str; 16] = [
        "motor", "bloco", "dados", "plano", "hash", "cofre", "rodape", "payload", "indice",
        "codec", "nivel", "stored", "delta", "zstd", "fluxo", "verifica",
    ];
    let mut out = Vec::with_capacity(n + 16);
    while out.len() < n {
        out.extend_from_slice(PALAVRAS[rng.abaixo(16) as usize].as_bytes());
        out.push(if rng.abaixo(8) == 0 { b'\n' } else { b' ' });
    }
    out.truncate(n);
    out
}

fn serie_i64(n: usize, rng: &mut Rng) -> Vec<u8> {
    let mut out = Vec::with_capacity(n + 8);
    let mut v = 1_700_000_000_000i64 + rng.abaixo(1000) as i64;
    while out.len() < n {
        v += 30 + rng.abaixo(12) as i64;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.truncate(n);
    out
}

fn floats(n: usize, rng: &mut Rng) -> Vec<u8> {
    let mut out = Vec::with_capacity(n + 4);
    let mut i = 0u32;
    while out.len() < n {
        let v = 20.0f32 + (i as f32) * 0.001 + rng.abaixo(100) as f32 * 1e-6;
        out.extend_from_slice(&v.to_le_bytes());
        i += 1;
    }
    out.truncate(n);
    out
}

fn corridas(n: usize, rng: &mut Rng) -> Vec<u8> {
    let mut out = Vec::with_capacity(n + 300);
    while out.len() < n {
        let b = (rng.proximo() >> 32) as u8;
        let len = 1 + rng.abaixo(200) as usize;
        out.extend(std::iter::repeat_n(b, len));
    }
    out.truncate(n);
    out
}

fn csv_alinhado(linhas: usize, rng: &mut Rng) -> Vec<u8> {
    let mut out = Vec::with_capacity(linhas * 64);
    let mut base = rng.abaixo(1_000_000);
    for _ in 0..linhas {
        base += rng.abaixo(50);
        out.extend_from_slice(
            format!(
                "{:015},{:015},{:015},{:015}\n",
                base,
                base * 3,
                rng.abaixo(1000),
                2026
            )
            .as_bytes(),
        );
    }
    out
}

fn ndjson(n: usize, rng: &mut Rng) -> Vec<u8> {
    let mut out = Vec::with_capacity(n + 200);
    let mut i = 0u64;
    while out.len() < n {
        out.extend_from_slice(linha_evento(rng, i).as_bytes());
        out.push(b'\n');
        i += 1;
    }
    out.truncate(n);
    out
}

fn linha_evento(rng: &mut Rng, i: u64) -> String {
    format!(
        r#"{{"evento":"pedido.criado","id":{},"cliente":"acme-{}","itens":[{{"sku":"A-{}","qtd":{}}}],"status":"pendente","canal":"api"}}"#,
        i,
        rng.abaixo(40),
        rng.abaixo(90),
        1 + rng.abaixo(7)
    )
}

fn treinar_dicionario() -> Arc<Dictionary> {
    let mut rng = Rng::new(77);
    let amostras: Vec<Vec<u8>> = (0..400u64)
        .map(|i| linha_evento(&mut rng, 5000 + i).into_bytes())
        .collect();
    let refs: Vec<&[u8]> = amostras.iter().map(|a| a.as_slice()).collect();
    let bytes = zstd::dict::from_samples(&refs, 6 * 1024).unwrap();
    let digest = hash_de(&bytes);
    let id = format!("application_x-ndjson.{}", hex::encode(&digest[..8]));
    Arc::new(Dictionary { id, digest, bytes })
}

fn escrever(
    dados: &[u8],
    log2: u8,
    escolher: &dyn Fn(usize, &[u8]) -> Plan,
    dict: Option<&Dictionary>,
    meta: &Metadata,
) -> Vec<u8> {
    let mut w = Writer::new(Vec::new(), log2).unwrap();
    for (i, pedaco) in dados.chunks(1usize << log2).enumerate() {
        let p = escolher(i, pedaco);
        let c = compress_block(pedaco, &p, dict).unwrap();
        w.push_compressed(pedaco, c).unwrap();
    }
    let (bytes, resumo) = w.finish(meta).unwrap();
    assert_eq!(resumo.file_len as usize, bytes.len());
    assert_eq!(resumo.file_hash, hash_de(dados));
    assert_eq!(resumo.original_size, dados.len() as u64);
    assert_eq!(resumo.block_count, dados.len().div_ceil(1usize << log2));
    bytes
}

fn confere(bytes: &[u8], original: &[u8], dicts: &dyn DictionaryResolver) {
    assert!(is_v4(bytes));
    let mut r = Reader::from_bytes(bytes).unwrap();
    assert_eq!(r.original_size(), original.len() as u64);
    assert_eq!(r.file_hash(), hash_de(original));
    assert_eq!(r.file_len(), bytes.len() as u64);
    r.verify_all().unwrap();
    assert!(r.corrupted_blocks().unwrap().is_empty());

    let back = r.restore_all(dicts).unwrap();
    assert!(back == original, "restore_all divergente");

    let tamanho = r.block_size();
    for i in 0..r.block_count() {
        let b = r.read_block(i, dicts).unwrap();
        let ini = i * tamanho;
        assert!(b == original[ini..ini + b.len()], "bloco {i} divergente");
    }

    let mut rs = Reader::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(rs.block_count(), r.block_count());
    assert!(rs.restore_all(dicts).unwrap() == original);
    let mut saida = Vec::new();
    assert_eq!(rs.restore_to(dicts, &mut saida).unwrap(), original.len() as u64);
    assert!(saida == original);
}

fn plano_padrao(_: usize, _: &[u8]) -> Plan {
    plano(Codec::Zstd, 3, &[])
}

#[test]
fn magic_distingue_v4_de_qualquer_outra_coisa() {
    assert!(is_v4(b"SYNT\x04"));
    assert!(is_v4(b"SYNT"));
    assert!(!is_v4(b"SYN"));
    assert!(!is_v4(b""));
    assert!(!is_v4(b"\x08\x03SYNT"));
    assert!(!is_v4(b"synt\x04"));
}

#[test]
fn arquivo_vazio_tem_zero_blocos() {
    let bytes = escrever(&[], 12, &plano_padrao, None, &Metadata::default());
    let mut r = Reader::from_bytes(&bytes).unwrap();
    assert_eq!(r.block_count(), 0);
    assert_eq!(r.original_size(), 0);
    assert_eq!(r.file_hash(), hash_de(&[]));
    assert!(r.restore_all(&NoDictionaries).unwrap().is_empty());
    assert!(r.block_info(0).is_none());
    assert_eq!(
        bytes.len(),
        HEADER_LEN + r.footer_len() + TRAILER_LEN,
        "sem blocos o arquivo é só cabeçalho, rodapé e final"
    );
    confere(&bytes, &[], &NoDictionaries);
}

#[test]
fn um_byte_vira_um_bloco_stored() {
    let dados = [0x42u8];
    let bytes = escrever(&dados, 12, &plano_padrao, None, &Metadata::default());
    confere(&bytes, &dados, &NoDictionaries);
    let r = Reader::from_bytes(&bytes).unwrap();
    assert_eq!(r.block_count(), 1);
    assert!(r.plans()[0].is_stored());
    let info = r.block_info(0).unwrap();
    assert_eq!((info.stored_len, info.original_len), (1, 1));
}

#[test]
fn tamanhos_nao_multiplos_do_bloco() {
    let mut rng = Rng::new(1);
    for n in [1usize, 2, 4095, 4096, 4097, 8191, 8192, 8193, 3 * 4096 + 17, 10 * 4096 - 1] {
        let dados = texto(n, &mut rng);
        let bytes = escrever(&dados, 12, &plano_padrao, None, &Metadata::default());
        confere(&bytes, &dados, &NoDictionaries);
        let r = Reader::from_bytes(&bytes).unwrap();
        assert_eq!(r.block_count(), n.div_ceil(4096), "n={n}");
        let ultimo = r.block_info(r.block_count() - 1).unwrap();
        let resto = n % 4096;
        assert_eq!(
            ultimo.original_len as usize,
            if resto == 0 { 4096 } else { resto }
        );
    }
}

#[test]
fn exatamente_n_blocos() {
    let mut rng = Rng::new(2);
    for n in [1usize, 2, 5, 16] {
        let dados = texto(n * 4096, &mut rng);
        let bytes = escrever(&dados, 12, &plano_padrao, None, &Metadata::default());
        confere(&bytes, &dados, &NoDictionaries);
        let r = Reader::from_bytes(&bytes).unwrap();
        assert_eq!(r.block_count(), n);
        for i in 0..n {
            assert_eq!(r.block_info(i).unwrap().original_len, 4096);
        }
    }
}

#[test]
fn mil_blocos_pequenos_com_plano_misto() {
    let mut rng = Rng::new(3);
    let dados = texto(1000 * 4096, &mut rng);
    let escolher = |i: usize, _: &[u8]| match i % 4 {
        0 => plano(Codec::Lz4, 0, &[]),
        1 => plano(Codec::Zstd, 1, &[]),
        2 => plano(Codec::Deflate, 6, &[]),
        _ => plano(Codec::Stored, 0, &[]),
    };
    let bytes = escrever(&dados, 12, &escolher, None, &Metadata::default());
    confere(&bytes, &dados, &NoDictionaries);
    let r = Reader::from_bytes(&bytes).unwrap();
    assert_eq!(r.block_count(), 1000);
    assert_eq!(r.plans().len(), 4);
    let usados: BTreeSet<u16> = (0..1000).map(|i| r.block_info(i).unwrap().plan_idx).collect();
    assert_eq!(usados.len(), 4);
}

#[test]
fn todos_os_codecs_em_blocos() {
    let mut rng = Rng::new(4);
    let dados = texto(3 * 4096 + 1234, &mut rng);
    for info in CATALOG {
        let escolher = move |_: usize, _: &[u8]| plano(info.codec, info.default_level, &[]);
        let bytes = escrever(&dados, 12, &escolher, None, &Metadata::default());
        confere(&bytes, &dados, &NoDictionaries);
        let r = Reader::from_bytes(&bytes).unwrap();
        assert!(
            r.plans().iter().any(|p| p.codec == info.codec),
            "codec {} não foi usado de fato",
            info.name
        );
    }
}

#[test]
fn todos_os_niveis_extremos_de_cada_codec() {
    let mut rng = Rng::new(5);
    let dados = texto(2 * 4096 + 100, &mut rng);
    for info in CATALOG {
        let niveis: Vec<i32> = if info.codec == Codec::Xz {
            vec![info.min_level, info.default_level]
        } else {
            vec![info.min_level, info.default_level, info.max_level]
        };
        for nivel in niveis {
            let escolher = move |_: usize, _: &[u8]| plano(info.codec, nivel, &[]);
            let bytes = escrever(&dados, 12, &escolher, None, &Metadata::default());
            confere(&bytes, &dados, &NoDictionaries);
        }
    }
}

fn plano_registra_transform(r: &Reader<SliceSource<'_>>, t: Transform) -> bool {
    r.plans()
        .iter()
        .any(|p| p.transforms.iter().any(|s| s.transform == t as i32))
}

#[test]
fn transforms_delta_em_todas_as_larguras() {
    let mut rng = Rng::new(6);
    let dados = serie_i64(3 * 4096 + 1000, &mut rng);
    for w in WIDTHS {
        let escolher = move |_: usize, _: &[u8]| plano(Codec::Zstd, 3, &[(Transform::Delta, w)]);
        let bytes = escrever(&dados, 12, &escolher, None, &Metadata::default());
        confere(&bytes, &dados, &NoDictionaries);
        let r = Reader::from_bytes(&bytes).unwrap();
        if w == 8 {
            assert!(plano_registra_transform(&r, Transform::Delta));
        }
    }
}

#[test]
fn transforms_byte_split_em_todas_as_larguras() {
    let mut rng = Rng::new(7);
    let dados = floats(3 * 4096 + 1000, &mut rng);
    for w in WIDTHS {
        let escolher =
            move |_: usize, _: &[u8]| plano(Codec::Zstd, 3, &[(Transform::ByteSplit, w)]);
        let bytes = escrever(&dados, 12, &escolher, None, &Metadata::default());
        confere(&bytes, &dados, &NoDictionaries);
        let r = Reader::from_bytes(&bytes).unwrap();
        if w == 4 {
            assert!(plano_registra_transform(&r, Transform::ByteSplit));
        }
    }
}

#[test]
fn transform_rle_em_blocos() {
    let mut rng = Rng::new(8);
    let dados = corridas(4 * 4096 + 333, &mut rng);
    let escolher = |_: usize, _: &[u8]| plano(Codec::Zstd, 3, &[(Transform::Rle, 1)]);
    let bytes = escrever(&dados, 12, &escolher, None, &Metadata::default());
    confere(&bytes, &dados, &NoDictionaries);
    let r = Reader::from_bytes(&bytes).unwrap();
    assert!(plano_registra_transform(&r, Transform::Rle));
}

#[test]
fn transform_csv_colunar_em_blocos_com_linhas_inteiras() {
    let mut rng = Rng::new(9);
    let dados = csv_alinhado(64 * 3 + 10, &mut rng);
    assert_eq!(dados.len() % 64, 0);
    let escolher = |_: usize, _: &[u8]| plano(Codec::Zstd, 3, &[(Transform::CsvColumnar, 1)]);
    let bytes = escrever(&dados, 12, &escolher, None, &Metadata::default());
    confere(&bytes, &dados, &NoDictionaries);
    let r = Reader::from_bytes(&bytes).unwrap();
    assert!(plano_registra_transform(&r, Transform::CsvColumnar));
}

#[test]
fn cadeia_de_transforms_em_blocos() {
    let mut rng = Rng::new(10);
    let dados = serie_i64(5 * 4096, &mut rng);
    let escolher = |_: usize, _: &[u8]| {
        plano(
            Codec::Zstd,
            3,
            &[(Transform::Delta, 8), (Transform::ByteSplit, 8)],
        )
    };
    let bytes = escrever(&dados, 12, &escolher, None, &Metadata::default());
    confere(&bytes, &dados, &NoDictionaries);
    let r = Reader::from_bytes(&bytes).unwrap();
    assert!(r.plans().iter().any(|p| p.transforms.len() == 2));
    assert!(r
        .plans()
        .iter()
        .all(|p| p.transforms.iter().all(|s| s.input_len == 0)));
}

fn bloco_de_tipo(tipo: usize, rng: &mut Rng) -> (Vec<u8>, Plan) {
    match tipo % 6 {
        0 => (texto(4096, rng), plano(Codec::Zstd, 3, &[])),
        1 => (
            serie_i64(4096, rng),
            plano(Codec::Zstd, 3, &[(Transform::Delta, 8)]),
        ),
        2 => (
            floats(4096, rng),
            plano(Codec::Zstd, 3, &[(Transform::ByteSplit, 4)]),
        ),
        3 => (
            corridas(4096, rng),
            plano(Codec::Zstd, 3, &[(Transform::Rle, 1)]),
        ),
        4 => (rng.bytes(4096), plano(Codec::Lz4, 0, &[])),
        _ => (
            csv_alinhado(64, rng),
            plano(Codec::Zstd, 3, &[(Transform::CsvColumnar, 1)]),
        ),
    }
}

fn arquivo_misto(blocos: usize, sufixo: usize) -> (Vec<u8>, Vec<u8>) {
    let mut rng = Rng::new(11);
    let mut dados = Vec::new();
    let mut planos = Vec::new();
    for i in 0..blocos {
        let (d, p) = bloco_de_tipo(i, &mut rng);
        dados.extend_from_slice(&d);
        planos.push(p);
    }
    if sufixo > 0 {
        dados.extend_from_slice(&texto(sufixo, &mut rng));
        planos.push(plano(Codec::Zstd, 3, &[]));
    }
    let escolher = |i: usize, _: &[u8]| planos[i].clone();
    let meta = Metadata {
        original_name: "misto.bin".to_string(),
        original_names: vec!["misto.bin".to_string(), "alias.bin".to_string()],
        original_mime: "application/octet-stream".to_string(),
        content_class: "binary".to_string(),
        plan_reason: "teste".to_string(),
        verified: true,
        created_at: 1,
        updated_at: 2,
        file_metrics: None,
    };
    let bytes = escrever(&dados, 12, &escolher, None, &meta);
    (bytes, dados)
}

#[test]
fn plano_misto_por_bloco_e_acesso_aleatorio() {
    let (bytes, dados) = arquivo_misto(60, 777);
    confere(&bytes, &dados, &NoDictionaries);
    let mut r = Reader::from_bytes(&bytes).unwrap();
    assert!(r.plans().len() >= 6, "planos: {}", r.plans().len());
    assert!(r.plans().iter().any(|p| p.is_stored()));
    for t in [
        Transform::Delta,
        Transform::ByteSplit,
        Transform::Rle,
        Transform::CsvColumnar,
    ] {
        assert!(r.plans().iter().any(|p| p
            .transforms
            .iter()
            .any(|s| s.transform == t as i32)));
    }
    let mut rng = Rng::new(12);
    for _ in 0..200 {
        let i = rng.abaixo(r.block_count() as u64) as usize;
        let b = r.read_block(i, &NoDictionaries).unwrap();
        assert!(b == dados[i * 4096..i * 4096 + b.len()]);
    }
}

#[test]
fn blocos_incompressiveis_ficam_stored_sem_expandir() {
    let mut rng = Rng::new(13);
    let dados = rng.bytes(10 * 4096 + 500);
    let bytes = escrever(&dados, 12, &plano_padrao, None, &Metadata::default());
    confere(&bytes, &dados, &NoDictionaries);
    let r = Reader::from_bytes(&bytes).unwrap();
    for i in 0..r.block_count() {
        let info = r.block_info(i).unwrap();
        assert_eq!(info.stored_len, info.original_len, "bloco {i} expandiu");
        assert!(r.plans()[info.plan_idx as usize].is_stored());
    }
    assert_eq!(r.plans().len(), 1);
}

#[test]
fn compress_block_nunca_devolve_payload_maior_que_o_original() {
    let mut rng = Rng::new(14);
    let entradas: Vec<Vec<u8>> = vec![
        Vec::new(),
        vec![1],
        vec![0; 4096],
        rng.bytes(4096),
        rng.bytes(100),
        texto(4096, &mut rng),
        corridas(4096, &mut rng),
    ];
    for info in CATALOG {
        for entrada in &entradas {
            let p = plano(info.codec, info.default_level, &[]);
            let c = compress_block(entrada, &p, None).unwrap();
            assert!(
                c.payload.len() <= entrada.len(),
                "{} expandiu {} -> {}",
                info.name,
                entrada.len(),
                c.payload.len()
            );
            let volta = decode_block(&c.payload, &c.plan, None, entrada.len()).unwrap();
            assert!(&volta == entrada);
        }
    }
}

#[test]
fn compress_block_cai_para_stored_quando_o_payload_nao_reduz() {
    let mut rng = Rng::new(15);
    let aleatorio = rng.bytes(4096);
    let c = compress_block(&aleatorio, &plano(Codec::Xz, 6, &[]), None).unwrap();
    assert!(c.plan.is_stored());
    assert_eq!(c.payload, aleatorio);

    let rle_inutil = compress_block(&aleatorio, &plano(Codec::Stored, 0, &[(Transform::Rle, 1)]), None)
        .unwrap();
    assert!(rle_inutil.payload.len() <= aleatorio.len());
}

#[test]
fn dicionario_por_bloco_com_verificacao_de_digest() {
    let mut rng = Rng::new(16);
    let dict = treinar_dicionario();
    let dados = ndjson(5 * 4096, &mut rng);
    let escolher = |_: usize, _: &[u8]| Plan {
        transforms: Vec::new(),
        codec: Codec::Zstd,
        level: 3,
        use_dictionary: true,
    };
    let bytes = escrever(&dados, 12, &escolher, Some(&dict), &Metadata::default());
    let dicts = Dicts(vec![dict.clone()]);
    confere(&bytes, &dados, &dicts);

    let r = Reader::from_bytes(&bytes).unwrap();
    let ref_id = r.plans()[0].dictionary.as_ref().unwrap().id.clone();
    assert_eq!(ref_id, dict.id);

    let mut r = Reader::from_bytes(&bytes).unwrap();
    let e = erro(r.read_block(2, &NoDictionaries));
    assert_eq!(e.code(), "dictionary_unavailable");
    assert_eq!(e.block_index(), Some(2));
    let e = erro(r.restore_all(&NoDictionaries));
    assert_eq!(e.code(), "dictionary_unavailable");

    let trocado = Dicts(vec![Arc::new(Dictionary {
        id: dict.id.clone(),
        digest: [9u8; 32],
        bytes: dict.bytes.clone(),
    })]);
    let e = erro(r.read_block(0, &trocado));
    assert_eq!(e.code(), "dictionary_mismatch");
}

#[test]
fn hash_do_payload_vem_antes_da_resolucao_do_dicionario() {
    let mut rng = Rng::new(17);
    let dict = treinar_dicionario();
    let dados = ndjson(2 * 4096, &mut rng);
    let escolher = |_: usize, _: &[u8]| Plan {
        transforms: Vec::new(),
        codec: Codec::Zstd,
        level: 3,
        use_dictionary: true,
    };
    let mut bytes = escrever(&dados, 12, &escolher, Some(&dict), &Metadata::default());
    let info = Reader::from_bytes(&bytes).unwrap().block_info(1).unwrap();
    bytes[info.offset as usize + 3] ^= 0x10;
    let mut r = Reader::from_bytes(&bytes).unwrap();
    let e = erro(r.read_block(1, &NoDictionaries));
    assert_eq!(e.code(), "block_corrupted");
}

#[test]
fn metadados_sobrevivem_ao_ciclo() {
    let mut rng = Rng::new(18);
    let dados = texto(9000, &mut rng);
    let meta = Metadata {
        original_name: "relatorio.csv".to_string(),
        original_names: vec!["relatorio.csv".to_string(), "copia.csv".to_string()],
        original_mime: "text/csv".to_string(),
        content_class: "tabular".to_string(),
        plan_reason: "zstd-3 por bloco".to_string(),
        verified: true,
        created_at: 1_790_000_000_000,
        updated_at: 1_790_000_000_500,
        file_metrics: Some(crate::proto::FileMetrics {
            throughput_mbps: 321.5,
            compression_ratio: 0.31,
            dedup_savings_bytes: 0,
            processing_time_ms: 12.5,
            algorithm_used: "zstd:3".to_string(),
            strategy_reason: "teste".to_string(),
            candidates_tried: 1,
            reconstruct_ms: 3.0,
        }),
    };
    let bytes = escrever(&dados, 12, &plano_padrao, None, &meta);
    let r = Reader::from_bytes(&bytes).unwrap();
    assert_eq!(r.metadata(), meta);
}

#[test]
fn leitor_em_fluxo_sobre_arquivo_de_verdade() {
    let mut rng = Rng::new(19);
    let dados = texto(7 * 4096 + 99, &mut rng);
    let bytes = escrever(&dados, 12, &plano_padrao, None, &Metadata::default());
    let caminho = std::env::temp_dir().join(format!("syntra-v4-test-{}.synt", std::process::id()));
    std::fs::write(&caminho, &bytes).unwrap();
    {
        let arquivo = std::fs::File::open(&caminho).unwrap();
        let mut r = Reader::from_reader(arquivo).unwrap();
        assert_eq!(r.block_count(), 8);
        r.verify_all().unwrap();
        assert!(r.restore_all(&NoDictionaries).unwrap() == dados);
        let b = r.read_block(7, &NoDictionaries).unwrap();
        assert!(b == dados[7 * 4096..]);
    }
    let _ = std::fs::remove_file(&caminho);
}

#[test]
fn escrita_em_arquivo_produz_os_mesmos_bytes_que_em_memoria() {
    let mut rng = Rng::new(20);
    let dados = texto(3 * 4096, &mut rng);
    let em_memoria = escrever(&dados, 12, &plano_padrao, None, &Metadata::default());
    let caminho = std::env::temp_dir().join(format!("syntra-v4-escrita-{}.synt", std::process::id()));
    {
        let arquivo = std::io::BufWriter::new(std::fs::File::create(&caminho).unwrap());
        let mut w = Writer::new(arquivo, 12).unwrap();
        for pedaco in dados.chunks(4096) {
            let c = compress_block(pedaco, &plano_padrao(0, pedaco), None).unwrap();
            w.push_compressed(pedaco, c).unwrap();
        }
        let (mut arquivo, _) = w.finish(&Metadata::default()).unwrap();
        arquivo.flush().unwrap();
    }
    let lido = std::fs::read(&caminho).unwrap();
    let _ = std::fs::remove_file(&caminho);
    assert_eq!(lido, em_memoria);
}

#[test]
fn block_info_descreve_um_layout_contiguo() {
    let (bytes, _) = arquivo_misto(20, 100);
    let r = Reader::from_bytes(&bytes).unwrap();
    let mut esperado = HEADER_LEN as u64;
    for i in 0..r.block_count() {
        let info = r.block_info(i).unwrap();
        assert_eq!(info.offset, esperado);
        esperado += info.stored_len as u64;
    }
    assert_eq!(esperado + r.footer_len() as u64 + TRAILER_LEN as u64, bytes.len() as u64);
}

#[test]
fn escritor_rejeita_uso_invalido() {
    assert_eq!(erro(Writer::new(Vec::new(), 11)).code(), "block_size_invalid");
    assert_eq!(erro(Writer::new(Vec::new(), 27)).code(), "block_size_invalid");
    assert!(Writer::new(Vec::new(), MIN_BLOCK_LOG2).is_ok());
    assert!(Writer::new(Vec::new(), MAX_BLOCK_LOG2).is_ok());

    let mut w = Writer::new(Vec::new(), 12).unwrap();
    assert_eq!(erro(w.push_block(&[1], 0, &[1])).code(), "invalid_input");
    let stored = w.add_plan(BlockPlan::stored()).unwrap();
    assert_eq!(w.add_plan(BlockPlan::stored()).unwrap(), stored);
    assert_eq!(erro(w.push_block(&[], stored, &[])).code(), "invalid_input");
    assert_eq!(
        erro(w.push_block(&vec![0u8; 4097], stored, &vec![0u8; 4097])).code(),
        "invalid_input"
    );
    assert_eq!(
        erro(w.push_block(&[1, 2, 3], stored, &vec![0u8; 5000])).code(),
        "invalid_input"
    );
    assert_eq!(erro(w.push_block(&[1], 7, &[1])).code(), "invalid_input");

    w.push_block(&[1, 2, 3], stored, &[1, 2, 3]).unwrap();
    assert_eq!(
        erro(w.push_block(&[4], stored, &[4])).code(),
        "invalid_input",
        "bloco curto só pode ser o último"
    );

    let mut w = Writer::new(Vec::new(), 12).unwrap();
    let mut ruim = BlockPlan::new(Codec::Zstd, 3);
    ruim.level = 99;
    assert_eq!(erro(w.add_plan(ruim)).code(), "plan_invalid");
    let ruim = BlockPlan::new(Codec::Zstd, 3).with_steps(&[(Transform::Delta, 3)]);
    assert_eq!(erro(w.add_plan(ruim)).code(), "plan_invalid");
    let ruim = BlockPlan {
        transforms: Vec::new(),
        codec: Codec::Lz4,
        level: 0,
        dictionary: Some(DictionaryRef {
            id: "x.0".to_string(),
            digest: vec![0; 32],
            size: 1,
        }),
    };
    assert_eq!(erro(w.add_plan(ruim)).code(), "plan_invalid");
    let ruim = BlockPlan {
        transforms: Vec::new(),
        codec: Codec::Unspecified,
        level: 0,
        dictionary: None,
    };
    assert_eq!(erro(w.add_plan(ruim)).code(), "plan_invalid");

    let w = Writer::new(Vec::new(), 12).unwrap();
    let meta = Metadata {
        original_names: vec![String::new(); MAX_NAMES + 1],
        ..Default::default()
    };
    assert_eq!(erro(w.finish(&meta)).code(), "invalid_input");
}

#[test]
fn leitura_fora_da_faixa_e_erro_tipado() {
    let (bytes, _) = arquivo_misto(3, 0);
    let mut r = Reader::from_bytes(&bytes).unwrap();
    let e = erro(r.read_block(3, &NoDictionaries));
    assert_eq!(e.code(), "block_out_of_range");
    assert_eq!(e.block_index(), Some(3));
}

const CODIGOS: [&str; 17] = [
    "bad_magic",
    "unsupported_version",
    "unsupported_flags",
    "block_size_invalid",
    "truncated",
    "footer_too_large",
    "footer_corrupted",
    "layout_invalid",
    "plan_invalid",
    "block_out_of_range",
    "block_corrupted",
    "dictionary_unavailable",
    "dictionary_mismatch",
    "block_decode_failed",
    "fidelity_mismatch",
    "io_error",
    "invalid_input",
];

fn confere_codigo(e: &BlockError) {
    assert!(CODIGOS.contains(&e.code()), "código desconhecido: {}", e.code());
    assert!(!e.to_string().is_empty());
}

#[derive(Debug, PartialEq, Eq)]
enum Resultado {
    Erro,
    Identico,
}

fn avalia(bytes: &[u8], original: &[u8], dicts: &dyn DictionaryResolver) -> Resultado {
    match Reader::from_bytes(bytes) {
        Err(e) => {
            confere_codigo(&e);
            Resultado::Erro
        }
        Ok(mut r) => {
            assert!(r.footer_len() <= MAX_FOOTER_LEN);
            assert!(r.block_count() <= MAX_BLOCKS);
            assert!(r.block_count() * ENTRY_BYTES <= r.footer_len());
            match r.restore_all(dicts) {
                Err(e) => {
                    confere_codigo(&e);
                    Resultado::Erro
                }
                Ok(d) => {
                    assert!(d == original, "restauro divergente sem erro");
                    Resultado::Identico
                }
            }
        }
    }
}

fn bases_do_fuzz() -> Vec<(Vec<u8>, Vec<u8>, Dicts)> {
    let mut rng = Rng::new(21);
    let mut bases = Vec::new();
    let sem = || Dicts(Vec::new());

    bases.push((
        escrever(&[], 12, &plano_padrao, None, &Metadata::default()),
        Vec::new(),
        sem(),
    ));
    bases.push((
        escrever(&[7], 12, &plano_padrao, None, &Metadata::default()),
        vec![7],
        sem(),
    ));
    let (b, d) = arquivo_misto(4, 700);
    bases.push((b, d, sem()));

    let dict = treinar_dicionario();
    let dados = ndjson(3 * 4096 + 50, &mut rng);
    let escolher = |_: usize, _: &[u8]| Plan {
        transforms: Vec::new(),
        codec: Codec::Zstd,
        level: 3,
        use_dictionary: true,
    };
    let bytes = escrever(&dados, 12, &escolher, Some(&dict), &Metadata::default());
    bases.push((bytes, dados, Dicts(vec![dict])));
    bases
}

fn rodape_de(bytes: &[u8]) -> (FooterV4, usize) {
    let len = bytes.len();
    let footer_len =
        u32::from_le_bytes([bytes[len - 24], bytes[len - 23], bytes[len - 22], bytes[len - 21]])
            as usize;
    let ini = len - TRAILER_LEN - footer_len;
    (FooterV4::decode(&bytes[ini..ini + footer_len]).unwrap(), footer_len)
}

fn forjar_bruto(log2: u8, payload: &[u8], footer: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&MAGIC);
    v.extend_from_slice(&[FORMAT_VERSION, 0, log2]);
    v.extend_from_slice(payload);
    v.extend_from_slice(footer);
    v.extend_from_slice(&(footer.len() as u32).to_le_bytes());
    v.extend_from_slice(&short_hash(footer));
    v.extend_from_slice(&MAGIC);
    v
}

fn forjar(log2: u8, payload: &[u8], footer: &FooterV4) -> Vec<u8> {
    forjar_bruto(log2, payload, &footer.encode_to_vec())
}

fn regiao_de_payload(bytes: &[u8]) -> Vec<u8> {
    let (_, footer_len) = rodape_de(bytes);
    bytes[HEADER_LEN..bytes.len() - TRAILER_LEN - footer_len].to_vec()
}

fn fuzz_bytes_aleatorios() -> usize {
    let mut rng = Rng::new(22);
    let mut casos = 0usize;
    for i in 0..8000usize {
        let n = match i % 4 {
            0 => rng.abaixo(64) as usize,
            1 | 2 => rng.abaixo(4096) as usize,
            _ => rng.abaixo(70_000) as usize,
        };
        let mut b = rng.bytes(n);
        if i % 3 != 2 && n >= HEADER_LEN {
            b[..4].copy_from_slice(&MAGIC);
            b[4] = FORMAT_VERSION;
            b[5] = 0;
            b[6] = 12 + (rng.abaixo(15) as u8);
        }
        if i % 2 == 0 && n >= TRAILER_LEN + HEADER_LEN {
            b[n - 4..].copy_from_slice(&MAGIC);
            let tamanho = rng.abaixo(n as u64 + 100) as u32;
            b[n - 24..n - 20].copy_from_slice(&tamanho.to_le_bytes());
        }
        match Reader::from_bytes(&b) {
            Ok(_) => panic!("bytes aleatórios foram aceitos (caso {i})"),
            Err(e) => confere_codigo(&e),
        }
        casos += 1;
    }
    casos
}

fn rodape_hostil(rng: &mut Rng) -> (u8, Vec<u8>, FooterV4) {
    let log2 = 12 + rng.abaixo(3) as u8;
    let tamanho_bloco = 1u64 << log2;
    let n = rng.abaixo(4) as usize;
    let estruturado = rng.abaixo(2) == 0;

    let mut payload = Vec::new();
    let mut stored = Vec::new();
    let mut originais = Vec::new();
    let mut indices = Vec::new();
    let mut hashes = Vec::new();
    let mut total = 0u64;
    for i in 0..n {
        let original_len = if i + 1 < n {
            tamanho_bloco
        } else {
            1 + rng.abaixo(tamanho_bloco)
        };
        let bytes_do_payload = if rng.abaixo(2) == 0 {
            original_len as usize
        } else {
            rng.abaixo(original_len + 64) as usize
        };
        let p = rng.bytes(bytes_do_payload);
        stored.extend_from_slice(&(p.len() as u32).to_le_bytes());
        originais.extend_from_slice(&(original_len as u32).to_le_bytes());
        indices.extend_from_slice(&(rng.abaixo(3) as u16).to_le_bytes());
        hashes.extend_from_slice(&short_hash(&p));
        payload.extend_from_slice(&p);
        total += original_len;
    }

    let codecs = [
        Codec::Stored,
        Codec::Lz4,
        Codec::Zstd,
        Codec::Brotli,
        Codec::Xz,
        Codec::Bzip2,
        Codec::Deflate,
    ];
    let transforms = [Transform::Delta, Transform::ByteSplit, Transform::Rle];
    let planos: Vec<BlockPlanV4> = (0..3)
        .map(|_| {
            let c = codecs[rng.abaixo(codecs.len() as u64) as usize];
            let passos = rng.abaixo(3) as usize;
            BlockPlanV4 {
                transforms: (0..passos)
                    .map(|_| TransformStep {
                        transform: transforms[rng.abaixo(3) as usize] as i32,
                        width: WIDTHS[rng.abaixo(4) as usize],
                        input_len: 0,
                    })
                    .collect(),
                codec: c as i32,
                codec_level: codec::default_level(c),
                dictionary: None,
            }
        })
        .collect();

    let mut footer = FooterV4 {
        header_flags: 0,
        block_size_log2: log2 as u32,
        original_size: total,
        file_hash: if n == 0 {
            hash_de(&[]).to_vec()
        } else {
            rng.bytes(32)
        },
        plans: planos,
        block_count: n as u32,
        stored_lens: stored,
        original_lens: originais,
        plan_indices: indices,
        payload_hashes: hashes,
        ..Default::default()
    };

    if !estruturado {
        match rng.abaixo(12) {
            0 => footer.original_size = rng.proximo(),
            1 => footer.block_count = rng.proximo() as u32,
            2 => footer.file_hash = rng.bytes_ate(64),
            3 => footer.header_flags = rng.proximo() as u32,
            4 => footer.block_size_log2 = rng.abaixo(40) as u32,
            5 => footer.stored_lens = rng.bytes_ate(40),
            6 => footer.original_lens = rng.bytes_ate(40),
            7 => footer.plan_indices = rng.bytes_ate(40),
            8 => footer.payload_hashes = rng.bytes_ate(80),
            9 => {
                if let Some(p) = footer.plans.first_mut() {
                    p.codec = rng.proximo() as i32;
                    p.codec_level = rng.proximo() as i32;
                }
            }
            10 => {
                if let Some(p) = footer.plans.first_mut() {
                    p.transforms = (0..rng.abaixo(12))
                        .map(|_| TransformStep {
                            transform: rng.abaixo(8) as i32,
                            width: rng.abaixo(12) as u32,
                            input_len: rng.proximo() >> rng.abaixo(64),
                        })
                        .collect();
                }
            }
            _ => footer.plans.clear(),
        }
    }
    (log2, payload, footer)
}

fn fuzz_rodapes_forjados() -> usize {
    let mut rng = Rng::new(23);
    let mut aceitos = 0usize;
    for i in 0..5000usize {
        let (log2, payload, footer) = rodape_hostil(&mut rng);
        let arquivo = forjar(log2, &payload, &footer);
        match Reader::from_bytes(&arquivo) {
            Err(e) => confere_codigo(&e),
            Ok(mut r) => {
                aceitos += 1;
                assert!(r.block_count() <= MAX_BLOCKS);
                match r.restore_all(&NoDictionaries) {
                    Ok(_) => {}
                    Err(e) => confere_codigo(&e),
                }
                let _ = r.verify_all();
                let _ = r.corrupted_blocks();
            }
        }
        if i % 997 == 0 {
            let bruto = rng.bytes_ate(300);
            let arquivo = forjar_bruto(12, &payload, &bruto);
            if let Err(e) = Reader::from_bytes(&arquivo) {
                confere_codigo(&e);
            }
        }
    }
    assert!(aceitos > 0, "o gerador deveria produzir alguns arquivos válidos");
    5000
}

fn fuzz_um_byte_alterado() -> usize {
    let mascaras = [0x01u8, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0xFF];
    let mut casos = 0usize;
    for (bytes, original, dicts) in bases_do_fuzz() {
        let (_, footer_len) = rodape_de(&bytes);
        let fim_payload = bytes.len() - TRAILER_LEN - footer_len;
        let regioes = [(0, HEADER_LEN), (fim_payload, bytes.len())];
        let mut copia = bytes.clone();
        for (ini, fim) in regioes {
            for pos in ini..fim {
                for m in mascaras {
                    copia[pos] ^= m;
                    let r = avalia(&copia, &original, &dicts);
                    assert_eq!(r, Resultado::Erro, "alteração em {pos} (máscara {m:#04x}) passou");
                    copia[pos] ^= m;
                    casos += 1;
                }
            }
        }
        assert_eq!(copia, bytes);
    }
    casos
}

fn fuzz_truncamentos() -> usize {
    let mut casos = 0usize;
    for (bytes, original, dicts) in bases_do_fuzz() {
        for corte in 0..bytes.len() {
            let r = avalia(&bytes[..corte], &original, &dicts);
            assert_eq!(r, Resultado::Erro, "truncamento em {corte}/{} aceito", bytes.len());
            casos += 1;
        }
        assert_eq!(avalia(&bytes, &original, &dicts), Resultado::Identico);

        let mut rng = Rng::new(24);
        for extra in 1..=48usize {
            let mut maior = bytes.clone();
            maior.extend_from_slice(&rng.bytes(extra));
            assert_eq!(avalia(&maior, &original, &dicts), Resultado::Erro);
            casos += 1;
        }
    }
    casos
}

#[test]
fn fuzz_do_parser_com_mais_de_vinte_mil_entradas() {
    let aleatorios = fuzz_bytes_aleatorios();
    let forjados = fuzz_rodapes_forjados();
    let alterados = fuzz_um_byte_alterado();
    let truncados = fuzz_truncamentos();
    let total = aleatorios + forjados + alterados + truncados;
    println!(
        "fuzz do parser v4: {aleatorios} aleatórios, {forjados} rodapés forjados, {alterados} bytes alterados, {truncados} truncamentos/extensões, total {total}"
    );
    assert!(alterados > 3000, "alterações: {alterados}");
    assert!(truncados > 4000, "truncamentos: {truncados}");
    assert!(total >= 20_000, "total de entradas: {total}");
}

fn base_pequena() -> Vec<u8> {
    let mut rng = Rng::new(25);
    let dados = texto(3 * 4096, &mut rng);
    escrever(&dados, 12, &plano_padrao, None, &Metadata::default())
}

#[test]
fn rodapes_hostis_sao_recusados_com_codigo_estavel() {
    let base = base_pequena();
    let payload = regiao_de_payload(&base);
    let (rodape, _) = rodape_de(&base);

    type Ajuste = Box<dyn Fn(&mut FooterV4)>;
    let casos: Vec<(&str, &str, Ajuste)> = vec![
        ("block_count máximo", "footer_corrupted", Box::new(|f| f.block_count = u32::MAX)),
        ("block_count a mais", "footer_corrupted", Box::new(|f| f.block_count += 1)),
        ("original_size máximo", "layout_invalid", Box::new(|f| f.original_size = u64::MAX)),
        ("original_size a mais", "layout_invalid", Box::new(|f| f.original_size += 1)),
        ("file_hash curto", "footer_corrupted", Box::new(|f| f.file_hash.truncate(31))),
        ("flags divergentes", "footer_corrupted", Box::new(|f| f.header_flags = 1)),
        ("log2 divergente", "footer_corrupted", Box::new(|f| f.block_size_log2 = 13)),
        (
            "plano fora da tabela",
            "layout_invalid",
            Box::new(|f| f.plan_indices[..2].copy_from_slice(&[0xFF, 0xFF])),
        ),
        (
            "stored_len gigante",
            "layout_invalid",
            Box::new(|f| f.stored_lens[..4].copy_from_slice(&u32::MAX.to_le_bytes())),
        ),
        (
            "stored_len diminuído",
            "layout_invalid",
            Box::new(|f| {
                let v = u32::from_le_bytes([
                    f.stored_lens[0],
                    f.stored_lens[1],
                    f.stored_lens[2],
                    f.stored_lens[3],
                ]);
                f.stored_lens[..4].copy_from_slice(&(v - 1).to_le_bytes());
            }),
        ),
        (
            "original_len curto fora do último",
            "layout_invalid",
            Box::new(|f| f.original_lens[..4].copy_from_slice(&1u32.to_le_bytes())),
        ),
        (
            "original_len do último zerado",
            "layout_invalid",
            Box::new(|f| {
                let n = f.original_lens.len();
                f.original_lens[n - 4..].copy_from_slice(&0u32.to_le_bytes());
            }),
        ),
        ("hashes truncados", "footer_corrupted", Box::new(|f| f.payload_hashes.truncate(20))),
        ("tabela de tamanhos truncada", "footer_corrupted", Box::new(|f| f.stored_lens.truncate(5))),
        ("codec desconhecido", "plan_invalid", Box::new(|f| f.plans[0].codec = 99)),
        ("codec não especificado", "plan_invalid", Box::new(|f| f.plans[0].codec = 0)),
        ("nível fora da faixa", "plan_invalid", Box::new(|f| f.plans[0].codec_level = 1000)),
        (
            "input_len no plano",
            "plan_invalid",
            Box::new(|f| {
                f.plans[0].transforms = vec![TransformStep {
                    transform: Transform::Rle as i32,
                    width: 1,
                    input_len: 5,
                }]
            }),
        ),
        (
            "transform desconhecido",
            "plan_invalid",
            Box::new(|f| {
                f.plans[0].transforms = vec![TransformStep {
                    transform: 77,
                    width: 1,
                    input_len: 0,
                }]
            }),
        ),
        (
            "largura inválida",
            "plan_invalid",
            Box::new(|f| {
                f.plans[0].transforms = vec![TransformStep {
                    transform: Transform::Delta as i32,
                    width: 3,
                    input_len: 0,
                }]
            }),
        ),
        (
            "cadeia longa demais",
            "footer_corrupted",
            Box::new(|f| {
                f.plans[0].transforms = vec![
                    TransformStep {
                        transform: Transform::Rle as i32,
                        width: 1,
                        input_len: 0,
                    };
                    MAX_CHAIN + 1
                ]
            }),
        ),
        (
            "digest de dicionário curto",
            "plan_invalid",
            Box::new(|f| {
                f.plans[0].dictionary = Some(DictionaryRef {
                    id: "a.1".to_string(),
                    digest: vec![1; 5],
                    size: 0,
                })
            }),
        ),
        (
            "dicionário sem id",
            "plan_invalid",
            Box::new(|f| {
                f.plans[0].dictionary = Some(DictionaryRef {
                    id: String::new(),
                    digest: vec![1; 32],
                    size: 0,
                })
            }),
        ),
        (
            "planos demais",
            "footer_corrupted",
            Box::new(|f| f.plans = vec![BlockPlanV4::default(); MAX_PLANS + 1]),
        ),
        (
            "nomes demais",
            "footer_corrupted",
            Box::new(|f| f.original_names = vec![String::new(); MAX_NAMES + 1]),
        ),
    ];

    for (nome, esperado, ajuste) in &casos {
        let mut f = rodape.clone();
        ajuste(&mut f);
        let arquivo = forjar(12, &payload, &f);
        let e = erro(Reader::from_bytes(&arquivo));
        assert_eq!(e.code(), *esperado, "caso '{nome}': {e}");
    }

    let sem_ajuste = forjar(12, &payload, &rodape);
    assert!(Reader::from_bytes(&sem_ajuste).is_ok());
}

#[test]
fn campos_do_cabecalho_e_do_final_hostis() {
    let base = base_pequena();
    let (_, footer_len) = rodape_de(&base);

    let mut b = base.clone();
    b[0] = b'X';
    assert_eq!(erro(Reader::from_bytes(&b)).code(), "bad_magic");

    let mut b = base.clone();
    b[4] = 5;
    assert_eq!(erro(Reader::from_bytes(&b)).code(), "unsupported_version");
    b[4] = 3;
    assert_eq!(erro(Reader::from_bytes(&b)).code(), "unsupported_version");

    let mut b = base.clone();
    b[5] = 0x80;
    assert_eq!(erro(Reader::from_bytes(&b)).code(), "unsupported_flags");

    for log2 in [0u8, 11, 27, 255] {
        let mut b = base.clone();
        b[6] = log2;
        assert_eq!(erro(Reader::from_bytes(&b)).code(), "block_size_invalid");
    }
    let mut b = base.clone();
    b[6] = 13;
    assert_eq!(erro(Reader::from_bytes(&b)).code(), "footer_corrupted");

    let n = base.len();
    for tamanho in [u32::MAX, MAX_FOOTER_LEN as u32 + 1, 0x7FFF_FFFF] {
        let mut b = base.clone();
        b[n - 24..n - 20].copy_from_slice(&tamanho.to_le_bytes());
        assert_eq!(erro(Reader::from_bytes(&b)).code(), "footer_too_large");
    }
    let mut b = base.clone();
    b[n - 24..n - 20].copy_from_slice(&(n as u32).to_le_bytes());
    assert_eq!(erro(Reader::from_bytes(&b)).code(), "footer_corrupted");
    let mut b = base.clone();
    b[n - 24..n - 20].copy_from_slice(&(footer_len as u32 - 1).to_le_bytes());
    assert_eq!(erro(Reader::from_bytes(&b)).code(), "footer_corrupted");
    let mut b = base.clone();
    b[n - 1] = 0;
    assert_eq!(erro(Reader::from_bytes(&b)).code(), "truncated");
}

fn arquivo_de_64_blocos() -> (Vec<u8>, Vec<u8>) {
    let (bytes, dados) = arquivo_misto(64, 0);
    (bytes, dados)
}

#[test]
fn dez_mil_bit_flips_no_payload_sao_detectados_antes_de_decodificar() {
    let (mut bytes, original) = arquivo_de_64_blocos();
    let (inicio, fim, infos) = {
        let r = Reader::from_bytes(&bytes).unwrap();
        assert_eq!(r.block_count(), 64);
        let infos: Vec<BlockInfo> = (0..64).map(|i| r.block_info(i).unwrap()).collect();
        (HEADER_LEN, infos[63].offset + infos[63].stored_len as u64, infos)
    };
    let fim = fim as usize;
    let mut rng = Rng::new(26);

    for it in 0..10_000usize {
        let pos = inicio + rng.abaixo((fim - inicio) as u64) as usize;
        let bit = rng.abaixo(8) as u8;
        let alvo = infos.partition_point(|b| b.offset <= pos as u64) - 1;
        assert!(pos < (infos[alvo].offset + infos[alvo].stored_len as u64) as usize);

        bytes[pos] ^= 1 << bit;
        {
            let mut r = Reader::from_bytes(&bytes).unwrap();
            let e = erro(r.read_block(alvo, &NoDictionaries));
            assert_eq!(e.code(), "block_corrupted", "iteração {it}: {e}");
            assert_eq!(e.block_index(), Some(alvo), "iteração {it}");
            assert!(e.to_string().contains(&format!("bloco {alvo} ")), "{e}");

            let outro = (alvo + 1 + rng.abaixo(63) as usize) % 64;
            let b = r.read_block(outro, &NoDictionaries).unwrap();
            assert!(b == original[outro * 4096..outro * 4096 + b.len()]);

            if it % 100 == 0 {
                let e = erro(r.verify_all());
                assert_eq!(e.block_index(), Some(alvo));
                assert_eq!(r.corrupted_blocks().unwrap(), vec![alvo]);
                let e = erro(r.restore_all(&NoDictionaries));
                assert_eq!(e.code(), "block_corrupted");
                assert_eq!(e.block_index(), Some(alvo));
            }
        }
        bytes[pos] ^= 1 << bit;
    }

    let mut r = Reader::from_bytes(&bytes).unwrap();
    assert!(r.restore_all(&NoDictionaries).unwrap() == original);
}

#[test]
fn um_bit_flip_nao_impede_ler_os_outros_blocos() {
    let (mut bytes, original) = arquivo_de_64_blocos();
    let alvo = 17usize;
    let info = Reader::from_bytes(&bytes).unwrap().block_info(alvo).unwrap();
    bytes[info.offset as usize + info.stored_len as usize / 2] ^= 0x04;

    let mut r = Reader::from_bytes(&bytes).unwrap();
    for i in 0..64usize {
        let res = r.read_block(i, &NoDictionaries);
        if i == alvo {
            let e = erro(res);
            assert_eq!(e.code(), "block_corrupted");
            assert_eq!(e.block_index(), Some(alvo));
        } else {
            let b = res.unwrap_or_else(|e| panic!("bloco {i} deveria estar íntegro: {e}"));
            assert!(b == original[i * 4096..(i + 1) * 4096]);
        }
    }
    assert_eq!(r.corrupted_blocks().unwrap(), vec![alvo]);

    let mut rs = Reader::from_reader(Cursor::new(&bytes)).unwrap();
    assert_eq!(rs.corrupted_blocks().unwrap(), vec![alvo]);
    assert!(rs.read_block(alvo + 1, &NoDictionaries).is_ok());
}

#[test]
fn blocos_trocados_de_lugar_sao_detectados() {
    let (bytes, _) = arquivo_de_64_blocos();
    let r = Reader::from_bytes(&bytes).unwrap();
    let candidatos: Vec<BlockInfo> = (0..64)
        .map(|i| r.block_info(i).unwrap())
        .filter(|b| b.stored_len == 4096)
        .collect();
    assert!(candidatos.len() >= 2);
    let (a, b) = (candidatos[0], candidatos[1]);
    let mut trocado = bytes.clone();
    let (oa, ob) = (a.offset as usize, b.offset as usize);
    let copia_a = bytes[oa..oa + 4096].to_vec();
    trocado[oa..oa + 4096].copy_from_slice(&bytes[ob..ob + 4096]);
    trocado[ob..ob + 4096].copy_from_slice(&copia_a);
    let mut r = Reader::from_bytes(&trocado).unwrap();
    assert_eq!(r.corrupted_blocks().unwrap(), vec![a.index, b.index]);
}

#[test]
fn truncar_o_rodape_e_o_final_e_sempre_erro_tipado() {
    let (bytes, _) = arquivo_de_64_blocos();
    let (_, footer_len) = rodape_de(&bytes);
    let fim_payload = bytes.len() - TRAILER_LEN - footer_len;

    let cortes = [
        fim_payload,
        fim_payload + 1,
        fim_payload + footer_len / 2,
        fim_payload + footer_len - 1,
        fim_payload + footer_len,
        bytes.len() - 23,
        bytes.len() - 20,
        bytes.len() - 4,
        bytes.len() - 1,
    ];
    for corte in cortes {
        let e = erro(Reader::from_bytes(&bytes[..corte]));
        confere_codigo(&e);
        assert!(
            ["truncated", "footer_corrupted", "layout_invalid", "footer_too_large"].contains(&e.code()),
            "corte em {corte}: {}",
            e.code()
        );
    }

    let mut buraco = bytes[..HEADER_LEN + 5000].to_vec();
    buraco.extend_from_slice(&bytes[HEADER_LEN + 9000..]);
    let e = erro(Reader::from_bytes(&buraco));
    assert!(e.code() == "layout_invalid" || e.code() == "footer_corrupted", "{e}");
}

#[test]
fn payload_lixo_com_hash_valido_vira_erro_de_decode_em_todos_os_codecs() {
    let mut rng = Rng::new(27);
    for info in CATALOG {
        if info.codec == Codec::Stored {
            continue;
        }
        let mut w = Writer::new(Vec::new(), 12).unwrap();
        let idx = w.add_plan(BlockPlan::new(info.codec, info.default_level)).unwrap();
        let original = texto(4096, &mut rng);
        let lixo = rng.bytes(300);
        w.push_block(&original, idx, &lixo).unwrap();
        let (bytes, _) = w.finish(&Metadata::default()).unwrap();
        let mut r = Reader::from_bytes(&bytes).unwrap();
        r.verify_all().unwrap();
        let e = erro(r.read_block(0, &NoDictionaries));
        assert_eq!(e.code(), "block_decode_failed", "{}: {e}", info.name);
        assert_eq!(e.block_index(), Some(0));
    }
}

fn arquivo_com_payload(codec_id: Codec, payload: &[u8]) -> Vec<u8> {
    let mut w = Writer::new(Vec::new(), 12).unwrap();
    let idx = w.add_plan(BlockPlan::new(codec_id, codec::default_level(codec_id))).unwrap();
    w.push_block(&[0u8; 4096], idx, payload).unwrap();
    w.finish(&Metadata::default()).unwrap().0
}

#[test]
fn tamanho_declarado_acima_do_bloco_e_recusado_em_zstd_e_lz4() {
    let enorme = vec![0u8; 1 << 20];

    let z = codec::encode(&enorme, Codec::Zstd, 3, None).unwrap();
    assert!(z.len() < 200);
    let z_bytes = arquivo_com_payload(Codec::Zstd, &z);
    let mut r = Reader::from_bytes(&z_bytes).unwrap();
    let e = erro(r.read_block(0, &NoDictionaries));
    assert_eq!(e.code(), "block_decode_failed");
    assert!(e.to_string().contains("excede o limite"), "{e}");

    let l = codec::encode(&enorme, Codec::Lz4, 0, None).unwrap();
    let l_bytes = arquivo_com_payload(Codec::Lz4, &l);
    let mut r = Reader::from_bytes(&l_bytes).unwrap();
    let e = erro(r.read_block(0, &NoDictionaries));
    assert_eq!(e.code(), "block_decode_failed");
    assert!(e.to_string().contains("excede o limite"), "{e}");

    let sem_tamanho = zstd::stream::encode_all(&[0u8; 4096][..], 3).unwrap();
    assert_eq!(zstd::zstd_safe::get_frame_content_size(&sem_tamanho).unwrap(), None);
    let s_bytes = arquivo_com_payload(Codec::Zstd, &sem_tamanho);
    let mut r = Reader::from_bytes(&s_bytes).unwrap();
    let e = erro(r.read_block(0, &NoDictionaries));
    assert_eq!(e.code(), "block_decode_failed");
    assert!(e.to_string().contains("sem tamanho de conteúdo"), "{e}");
}

#[test]
fn bloco_decodificado_com_tamanho_errado_e_erro() {
    let curto = codec::encode(&[1u8; 100], Codec::Zstd, 3, None).unwrap();
    let bytes = arquivo_com_payload(Codec::Zstd, &curto);
    let mut r = Reader::from_bytes(&bytes).unwrap();
    let e = erro(r.read_block(0, &NoDictionaries));
    assert_eq!(e.code(), "block_decode_failed");
    assert!(e.to_string().contains("esperado 4096"), "{e}");
}

#[test]
fn hash_final_do_arquivo_divergente_vira_erro_de_fidelidade() {
    let base = base_pequena();
    let payload = regiao_de_payload(&base);
    let (mut rodape, _) = rodape_de(&base);
    rodape.file_hash[0] ^= 1;
    let arquivo = forjar(12, &payload, &rodape);
    let mut r = Reader::from_bytes(&arquivo).unwrap();
    r.verify_all().unwrap();
    for i in 0..r.block_count() {
        assert!(r.read_block(i, &NoDictionaries).is_ok());
    }
    let e = erro(r.restore_all(&NoDictionaries));
    assert_eq!(e.code(), "fidelity_mismatch");
}

#[test]
fn propriedade_round_trip_aleatorio() {
    let mut rng = Rng::new(28);
    for _ in 0..300usize {
        let log2 = 12 + rng.abaixo(2) as u8;
        let bloco = 1usize << log2;
        let n = match rng.abaixo(5) {
            0 => 0,
            1 => 1 + rng.abaixo(64) as usize,
            2 => bloco * (1 + rng.abaixo(4) as usize),
            3 => bloco * (1 + rng.abaixo(4) as usize) + 1,
            _ => rng.abaixo(5 * bloco as u64) as usize,
        };
        let dados = match rng.abaixo(5) {
            0 => texto(n, &mut rng),
            1 => serie_i64(n, &mut rng),
            2 => floats(n, &mut rng),
            3 => corridas(n, &mut rng),
            _ => rng.bytes(n),
        };
        let semente = rng.proximo();
        let escolher = move |i: usize, _: &[u8]| {
            let mut r = Rng::new(semente ^ (i as u64 + 1));
            match r.abaixo(7) {
                0 => plano(Codec::Zstd, 1, &[]),
                1 => plano(Codec::Lz4, 0, &[]),
                2 => plano(Codec::Deflate, 6, &[]),
                3 => plano(Codec::Zstd, 3, &[(Transform::Delta, 8)]),
                4 => plano(Codec::Zstd, 3, &[(Transform::ByteSplit, 4)]),
                5 => plano(Codec::Brotli, 5, &[(Transform::Rle, 1)]),
                _ => plano(Codec::Stored, 0, &[]),
            }
        };
        let bytes = escrever(&dados, log2, &escolher, None, &Metadata::default());
        confere(&bytes, &dados, &NoDictionaries);
    }
}

#[test]
fn envelopes_v2_e_v3_do_corpus_dourado_nao_sao_v4() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/testdata/golden");
    let manifesto: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    let casos = manifesto["casos"].as_array().unwrap();
    let mut versoes = BTreeSet::new();
    for caso in casos {
        let envelope = std::fs::read(dir.join(caso["envelope"].as_str().unwrap())).unwrap();
        let versao = caso["versao"].as_i64().unwrap();
        versoes.insert(versao);
        assert!(!is_v4(&envelope), "{} começa com o magic do v4", caso["nome"]);
        assert!(container::parse(&envelope).is_ok());
        assert_eq!(erro(Reader::from_bytes(&envelope)).code(), "bad_magic");
    }
    assert!(versoes.contains(&2) && versoes.contains(&3));
}

#[test]
fn nenhum_processed_file_serializado_comeca_com_o_magic() {
    let mut amostras: Vec<ProcessedFile> = vec![ProcessedFile::default()];
    for versao in [0, 1, 2, 3, 4, 7] {
        amostras.push(ProcessedFile {
            version: versao,
            original_name: "SYNT".to_string(),
            data: b"SYNT".to_vec(),
            ..Default::default()
        });
        amostras.push(ProcessedFile {
            version: versao,
            ..Default::default()
        });
    }
    amostras.push(ProcessedFile {
        pointer_to: "SYNT".to_string(),
        original_mime: "SYNT".to_string(),
        ..Default::default()
    });
    for a in amostras {
        let bytes = a.encode_to_vec();
        assert!(!is_v4(&bytes));
        assert!(bytes.first() != Some(&b'S'));
    }
}

#[test]
fn container_parse_recusa_v4_com_erro_claro_e_sem_panico() {
    let (misto, _) = arquivo_misto(5, 300);
    let vazio = escrever(&[], 12, &plano_padrao, None, &Metadata::default());
    let um = escrever(&[1], 12, &plano_padrao, None, &Metadata::default());
    for bytes in [misto.clone(), vazio, um] {
        match container::parse(&bytes) {
            Err(ContainerError::Decode(m)) => assert!(!m.is_empty()),
            outro => panic!("esperava ContainerError::Decode, veio {:?}", outro.map(|e| e.version)),
        }
        assert_eq!(
            container::parse(&bytes).err().map(|e| e.code()),
            Some("envelope_invalid")
        );
    }
    for corte in (0..misto.len()).step_by(97) {
        let _ = container::parse(&misto[..corte]);
    }
}

const _: () = assert!(MAX_BLOCKS * ENTRY_BYTES + FOOTER_RESERVE <= MAX_FOOTER_LEN);
const _: () = assert!(MAX_BLOCKS > 2_000_000);

#[test]
fn limites_documentados_sao_coerentes() {
    assert_eq!(ENTRY_BYTES, 26);
    assert_eq!(MAX_PLANS, u16::MAX as usize + 1);
    assert!(slack_len(1 << MAX_BLOCK_LOG2) < u32::MAX as usize);
    assert_eq!(DEFAULT_BLOCK_LOG2, 20);
    assert_eq!(HEADER_LEN, MAGIC.len() + 3);
    assert_eq!(TRAILER_LEN, 4 + HASH_LEN + MAGIC.len());
}
