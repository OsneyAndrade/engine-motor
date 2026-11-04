use std::io::{self, Read};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use prost::Message;

use crate::blockfile::{BlockPlan, Metadata, Reader, Writer};
use crate::codec::{self, Codec};
use crate::container::{self, ContainerError, NoDictionaries};
use crate::memtrack;
use crate::proto::{ProcessedFile, TransformStep};
use crate::transform::Transform;

const GIB: u64 = 1 << 30;
const TETO: u64 = 64 << 20;

struct Zeros(u64);

impl Read for Zeros {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = (buf.len() as u64).min(self.0) as usize;
        buf[..n].fill(0);
        self.0 -= n as u64;
        Ok(n)
    }
}

fn comprimir_zeros(codec_id: Codec, bytes: u64, com_tamanho: bool) -> Vec<u8> {
    let mut out = Vec::new();
    match codec_id {
        Codec::Zstd => {
            if com_tamanho {
                use std::io::Write;
                let mut enc = zstd::Encoder::new(Vec::new(), 1).unwrap();
                enc.include_contentsize(true).unwrap();
                enc.set_pledged_src_size(Some(bytes)).unwrap();
                let bloco = vec![0u8; 1 << 20];
                for _ in 0..(bytes >> 20) {
                    enc.write_all(&bloco).unwrap();
                }
                return enc.finish().unwrap();
            }
            let mut enc = zstd::stream::read::Encoder::new(Zeros(bytes), 1).unwrap();
            enc.read_to_end(&mut out).unwrap();
        }
        Codec::Brotli => {
            let params = brotli::enc::BrotliEncoderParams {
                quality: 1,
                lgwin: 22,
                ..Default::default()
            };
            brotli::BrotliCompress(&mut Zeros(bytes), &mut out, &params).unwrap();
        }
        Codec::Xz => {
            let mut enc = xz2::read::XzEncoder::new(Zeros(bytes), 0);
            enc.read_to_end(&mut out).unwrap();
        }
        Codec::Bzip2 => {
            let mut enc = bzip2::read::BzEncoder::new(Zeros(bytes), bzip2::Compression::new(1));
            enc.read_to_end(&mut out).unwrap();
        }
        Codec::Deflate => {
            let mut enc = flate2::read::DeflateEncoder::new(Zeros(bytes), flate2::Compression::new(1));
            enc.read_to_end(&mut out).unwrap();
        }
        outro => panic!("codec sem gerador de bomba: {outro:?}"),
    }
    out
}

fn bomba_lz4(declarado: u32, real: u64) -> Vec<u8> {
    let soma = real - 25;
    let k = (soma / 255) as usize;
    let resto = (soma % 255) as u8;
    let mut out = Vec::with_capacity(k + 16);
    out.extend_from_slice(&declarado.to_le_bytes());
    out.extend_from_slice(&[0x1F, 0x00, 0x01, 0x00]);
    out.extend(std::iter::repeat_n(0xFFu8, k));
    out.push(resto);
    out.extend_from_slice(&[0x50, 0, 0, 0, 0, 0]);
    out
}

pub(crate) struct Bombas {
    pub(crate) por_codec: Vec<(&'static str, Codec, Vec<u8>)>,
    zstd_sem_tamanho: Vec<u8>,
    tempo_de_geracao: Duration,
}

pub(crate) fn bombas() -> &'static Bombas {
    static BOMBAS: OnceLock<Bombas> = OnceLock::new();
    BOMBAS.get_or_init(|| {
        let inicio = Instant::now();
        let alvos = [
            ("zstd", Codec::Zstd, true),
            ("brotli", Codec::Brotli, false),
            ("xz", Codec::Xz, false),
            ("bzip2", Codec::Bzip2, false),
            ("deflate", Codec::Deflate, false),
        ];
        let mut por_codec: Vec<(&'static str, Codec, Vec<u8>)> = std::thread::scope(|escopo| {
            let tarefas: Vec<_> = alvos
                .iter()
                .map(|(nome, c, com_tamanho)| {
                    escopo.spawn(move || {
                        let t = Instant::now();
                        let payload = comprimir_zeros(*c, GIB, *com_tamanho);
                        eprintln!("BOMBAS gerar {nome}: {} ms", t.elapsed().as_millis());
                        (*nome, *c, payload)
                    })
                })
                .collect();
            tarefas.into_iter().map(|t| t.join().unwrap()).collect()
        });
        por_codec.push(("lz4", Codec::Lz4, bomba_lz4(GIB as u32, GIB)));
        let zstd_sem_tamanho = comprimir_zeros(Codec::Zstd, GIB, false);
        Bombas {
            por_codec,
            zstd_sem_tamanho,
            tempo_de_geracao: inicio.elapsed(),
        }
    })
}

#[test]
fn gerador_de_bomba_lz4_produz_zeros_validos() {
    let n = 5 << 20;
    let payload = bomba_lz4(n as u32, n);
    let out = codec::decode_limited(&payload, Codec::Lz4, None, None, 16 << 20).unwrap();
    assert_eq!(out.len() as u64, n);
    assert!(out.iter().all(|b| *b == 0));
}

#[test]
fn bombas_de_1_gib_sao_pequenas_e_de_verdade() {
    let b = bombas();
    eprintln!("BOMBAS geradas em {} ms", b.tempo_de_geracao.as_millis());
    for (nome, _, payload) in &b.por_codec {
        eprintln!("BOMBAS {nome}: payload de {} bytes para 1 GiB", payload.len());
        let limite = 8 << 20;
        assert!(
            payload.len() < limite,
            "{nome}: payload grande demais ({})",
            payload.len()
        );
    }
}

#[test]
fn bomba_de_1_gib_em_cada_codec_e_recusada_no_teto_de_64_mib() {
    let b = bombas();
    for (nome, codec_id, payload) in &b.por_codec {
        let inicio = Instant::now();
        let (resultado, pico) = memtrack::medir(|| {
            codec::decode_limited(payload, *codec_id, None, Some(GIB), TETO)
        });
        let tempo = inicio.elapsed();
        let erro = resultado.expect_err(nome);
        assert_eq!(
            codec::too_large_limit(&erro),
            Some(TETO),
            "{nome}: erro inesperado: {erro}"
        );
        eprintln!(
            "BOMBA {nome}: recusada em {} ms, pico de alocação do Rust {} bytes",
            tempo.as_millis(),
            pico
        );
        assert!(
            tempo < Duration::from_secs(10),
            "{nome}: demorou {tempo:?} para recusar"
        );
        assert!(
            pico as u64 <= TETO + (24 << 20),
            "{nome}: alocou {pico} bytes com teto de {TETO}"
        );
    }
}

#[test]
fn zstd_sem_tamanho_declarado_tambem_respeita_o_teto() {
    let b = bombas();
    assert_eq!(
        zstd::zstd_safe::get_frame_content_size(&b.zstd_sem_tamanho).unwrap(),
        None
    );
    let (resultado, pico) = memtrack::medir(|| {
        codec::decode_limited(&b.zstd_sem_tamanho, Codec::Zstd, None, Some(GIB), TETO)
    });
    let erro = resultado.unwrap_err();
    assert_eq!(codec::too_large_limit(&erro), Some(TETO), "{erro}");
    assert!(pico as u64 <= TETO + (24 << 20), "pico {pico}");
}

#[test]
fn lz4_valida_o_prefixo_antes_de_alocar() {
    let mut hostil = vec![0xFF, 0xFF, 0xFF, 0xFF];
    hostil.extend_from_slice(&[0x11; 64]);
    let (resultado, pico) = memtrack::medir(|| {
        codec::decode_limited(&hostil, Codec::Lz4, None, None, TETO)
    });
    let erro = resultado.unwrap_err();
    assert_eq!(codec::too_large_limit(&erro), Some(TETO), "{erro}");
    assert!(pico < (1 << 20), "o prefixo de 4 GiB provocou alocação de {pico} bytes");
}

#[test]
fn lz4_com_prefixo_inflado_e_corpo_minusculo_nao_aloca_o_declarado() {
    let mut hostil = Vec::new();
    hostil.extend_from_slice(&3_500_000_000u32.to_le_bytes());
    hostil.extend_from_slice(&[0x11; 64]);
    let (resultado, pico) = memtrack::medir(|| {
        codec::decode_limited(&hostil, Codec::Lz4, None, None, 4 * GIB)
    });
    let erro = resultado.unwrap_err();
    assert_eq!(erro.kind(), io::ErrorKind::InvalidData, "{erro}");
    assert!(codec::too_large_limit(&erro).is_none(), "{erro}");
    assert!(
        pico < (1 << 20),
        "o prefixo de 3,5 GB com corpo de 64 bytes provocou alocação de {pico} bytes"
    );
}

#[test]
fn envelope_lz4_forjado_com_tamanho_inflado_nao_aloca_o_declarado() {
    let mut payload = Vec::new();
    payload.extend_from_slice(&3_500_000_000u32.to_le_bytes());
    payload.extend_from_slice(&[0x11; 64]);
    let bytes = envelope_forjado(Codec::Lz4, payload, 3_500_000_000);
    let env = container::parse(&bytes).unwrap();
    let (resultado, pico) = memtrack::medir(|| {
        container::restore_with_limit(&env, &NoDictionaries, 4 * GIB)
    });
    assert!(resultado.is_err());
    assert!(pico < (1 << 20), "alocou {pico} bytes para um corpo de 68 bytes");
}

#[test]
fn lz4_legitimo_de_razao_extrema_continua_restaurando() {
    let dados = vec![0u8; 8 << 20];
    let enc = codec::encode(&dados, Codec::Lz4, 0, None).unwrap();
    assert!(enc.len() * 255 + (1 << 20) >= dados.len());
    let out = codec::decode_limited(&enc, Codec::Lz4, None, Some(dados.len() as u64), 16 << 20).unwrap();
    assert_eq!(out, dados);
}

#[test]
fn stored_acima_do_teto_e_recusado() {
    let dados = vec![1u8; 4096];
    let erro = codec::decode_limited(&dados, Codec::Stored, None, None, 1000).unwrap_err();
    assert_eq!(codec::too_large_limit(&erro), Some(1000));
    assert!(codec::decode_limited(&dados, Codec::Stored, None, None, 4096).is_ok());
}

#[test]
fn saida_no_limite_exato_passa_e_um_byte_acima_falha_em_todos_os_codecs() {
    let dados: Vec<u8> = (0..50_000u32).map(|i| (i % 251) as u8).collect();
    for info in codec::CATALOG {
        let enc = codec::encode(&dados, info.codec, info.default_level, None).unwrap();
        let n = dados.len() as u64;
        let ok = codec::decode_limited(&enc, info.codec, None, Some(n), n).unwrap();
        assert_eq!(ok, dados, "{}", info.name);
        let erro = codec::decode_limited(&enc, info.codec, None, Some(n), n - 1).unwrap_err();
        assert_eq!(codec::too_large_limit(&erro), Some(n - 1), "{}: {erro}", info.name);
    }
}

#[test]
fn capacidade_do_buffer_de_saida_nao_passa_do_teto() {
    let dados = vec![0u8; 3 << 20];
    for info in codec::CATALOG {
        let enc = codec::encode(&dados, info.codec, info.default_level, None).unwrap();
        let teto = dados.len() as u64;
        let (resultado, pico) = memtrack::medir(|| {
            codec::decode_limited(&enc, info.codec, None, None, teto)
        });
        let out = resultado.unwrap();
        assert_eq!(out.len(), dados.len());
        assert!(
            out.capacity() as u64 <= teto,
            "{}: capacidade {} acima do teto {teto}",
            info.name,
            out.capacity()
        );
        assert!(
            (pico as u64) < teto + (6 << 20),
            "{}: pico {pico} para saída de {teto}",
            info.name
        );
    }
}

fn erro_de<T>(r: Result<T, ContainerError>) -> ContainerError {
    match r {
        Ok(_) => panic!("esperava erro, veio Ok"),
        Err(e) => e,
    }
}

fn envelope_forjado(codec_id: Codec, payload: Vec<u8>, original_size: u64) -> Vec<u8> {
    let env = ProcessedFile {
        version: 3,
        original_name: "forjado.bin".to_string(),
        original_size,
        data: payload,
        codec: codec_id as i32,
        ..Default::default()
    };
    container::seal(env).unwrap().bytes
}

#[test]
fn envelope_forjado_com_bomba_devolve_decompressed_too_large() {
    let b = bombas();
    for (nome, codec_id, payload) in &b.por_codec {
        let bytes = envelope_forjado(*codec_id, payload.clone(), 1 << 20);
        let env = container::parse(&bytes).unwrap();
        let (resultado, pico) = memtrack::medir(|| {
            container::restore_with_limit(&env, &NoDictionaries, TETO)
        });
        match resultado {
            Err(ContainerError::DecompressedTooLarge { limit }) => {
                assert_eq!(limit, 1 << 20, "{nome}: o teto deveria ser o tamanho declarado");
            }
            Err(outro) => panic!("{nome}: esperava DecompressedTooLarge, veio {outro}"),
            Ok(_) => panic!("{nome}: a bomba foi expandida"),
        }
        assert!(pico < (6 << 20), "{nome}: pico de {pico} bytes com declarado de 1 MiB");
    }
}

#[test]
fn teto_da_configuracao_vale_quando_o_envelope_declara_mais() {
    let b = bombas();
    let (nome, codec_id, payload) = &b.por_codec[1];
    let bytes = envelope_forjado(*codec_id, payload.clone(), u64::MAX);
    let env = container::parse(&bytes).unwrap();
    let erro = erro_de(container::restore_with_limit(&env, &NoDictionaries, TETO));
    assert_eq!(erro.code(), "decompressed_too_large", "{nome}");
    match erro {
        ContainerError::DecompressedTooLarge { limit } => assert_eq!(limit, TETO),
        outro => panic!("{outro}"),
    }
}

#[test]
fn original_size_forjado_nao_faz_o_motor_alocar_o_declarado() {
    let payload = codec::encode(b"dez bytes!", Codec::Zstd, 3, None).unwrap();
    for declarado in [60 * GIB, u64::MAX, 1 << 50] {
        let bytes = envelope_forjado(Codec::Zstd, payload.clone(), declarado);
        let env = container::parse(&bytes).unwrap();
        let (resultado, pico) = memtrack::medir(|| {
            container::restore_with_limit(&env, &NoDictionaries, TETO)
        });
        assert!(resultado.is_err(), "declarado {declarado}: deveria falhar");
        assert!(
            pico < (4 << 20),
            "declarado {declarado}: alocou {pico} bytes para 10 bytes reais"
        );
    }
}

#[test]
fn amplificacao_por_rle_encadeado_e_barrada_a_cada_estagio() {
    let mut staged = Vec::new();
    for _ in 0..8192 {
        staged.extend_from_slice(&[129u8, 0]);
    }
    let passos = vec![
        crate::transform::step(Transform::Rle, 1, 0),
        crate::transform::step(Transform::Rle, 1, 0),
    ];
    let env = ProcessedFile {
        version: 3,
        original_size: staged.len() as u64,
        data: staged,
        codec: Codec::Stored as i32,
        transforms: passos,
        ..Default::default()
    };
    let bytes = container::seal(env).unwrap().bytes;
    let env = container::parse(&bytes).unwrap();
    let (resultado, pico) = memtrack::medir(|| {
        container::restore_with_limit(&env, &NoDictionaries, TETO)
    });
    match resultado {
        Err(ContainerError::DecompressedTooLarge { .. }) => {}
        Err(outro) => panic!("esperava DecompressedTooLarge, veio {outro}"),
        Ok(_) => panic!("a amplificação passou"),
    }
    assert!(pico < (2 << 20), "alocou {pico} bytes para 16 KiB declarados");
}

#[test]
fn cadeia_de_transforms_acima_do_maximo_e_recusada() {
    let passos: Vec<TransformStep> = (0..9)
        .map(|_| crate::transform::step(Transform::Delta, 1, 0))
        .collect();
    let env = ProcessedFile {
        version: 3,
        original_size: 4,
        data: vec![1, 2, 3, 4],
        codec: Codec::Stored as i32,
        transforms: passos,
        ..Default::default()
    };
    let bytes = container::seal(env).unwrap().bytes;
    let env = container::parse(&bytes).unwrap();
    let erro = erro_de(container::restore(&env, &NoDictionaries));
    assert_eq!(erro.code(), "restore_failed");
    assert!(erro.to_string().contains("excede o máximo"), "{erro}");
}

#[test]
fn envelope_valido_continua_restaurando_com_teto_justo() {
    let dados: Vec<u8> = (0..200_000u32).map(|i| (i % 97) as u8).collect();
    let profile = crate::content::classify(&dados, "dados.bin", None);
    let planned = crate::planner::compress_best(
        &dados,
        &profile,
        crate::planner::Effort::Balanced,
        None,
        &crate::planner::ForcedPlan::default(),
        &crate::planner::Limits::default(),
    )
    .unwrap();
    let hash: [u8; 32] = blake3::hash(&dados).into();
    let env = container::build(planned, &profile, "dados.bin", &hash, None, 1.0, true, 0.0);
    let sealed = container::seal(env).unwrap();
    let env = ProcessedFile::decode(sealed.bytes.as_slice()).unwrap();
    let r = container::restore_with_limit(&env, &NoDictionaries, dados.len() as u64).unwrap();
    assert_eq!(r.data, dados);
    let erro = container::restore_with_limit(&env, &NoDictionaries, dados.len() as u64 - 1);
    assert!(erro.is_err());
}

fn arquivo_v4_com_bomba(codec_id: Codec, payload: &[u8]) -> Vec<u8> {
    let mut w = Writer::new(Vec::new(), 20).unwrap();
    let idx = w
        .add_plan(BlockPlan::new(codec_id, codec::default_level(codec_id)))
        .unwrap();
    w.push_block(&vec![0u8; 1 << 20], idx, payload).unwrap();
    w.finish(&Metadata::default()).unwrap().0
}

#[test]
fn bloco_v4_usa_o_original_len_como_teto_em_todos_os_codecs_sem_tamanho_declarado() {
    for (nome, codec_id) in [
        ("brotli", Codec::Brotli),
        ("xz", Codec::Xz),
        ("bzip2", Codec::Bzip2),
        ("deflate", Codec::Deflate),
    ] {
        let payload = comprimir_zeros(codec_id, 64 << 20, false);
        let bytes = arquivo_v4_com_bomba(codec_id, &payload);
        let mut leitor = Reader::from_bytes(&bytes).unwrap();
        leitor.verify_all().unwrap();
        let (resultado, pico) = memtrack::medir(|| leitor.read_block(0, &NoDictionaries));
        let erro = resultado.unwrap_err();
        assert_eq!(erro.code(), "block_decode_failed", "{nome}: {erro}");
        assert!(
            erro.to_string().contains("teto de 1048576 bytes"),
            "{nome}: o teto deveria ser o original_len do bloco: {erro}"
        );
        assert!(
            pico < (20 << 20),
            "{nome}: bloco de 1 MiB com bomba de 64 MiB alocou {pico} bytes"
        );
    }
}

#[test]
fn bloco_v4_recusa_bomba_real_de_1_gib_em_todos_os_codecs() {
    let b = bombas();
    let mut chegaram_ao_leitor = Vec::new();
    for (nome, codec_id, payload) in &b.por_codec {
        let mut w = Writer::new(Vec::new(), 20).unwrap();
        let idx = w
            .add_plan(BlockPlan::new(*codec_id, codec::default_level(*codec_id)))
            .unwrap();
        if w.push_block(&vec![0u8; 1 << 20], idx, payload).is_err() {
            assert!(
                payload.len() > crate::blockfile::slack_len(1 << 20),
                "{nome}: o escritor recusou um payload de {} bytes",
                payload.len()
            );
            continue;
        }
        let bytes = w.finish(&Metadata::default()).unwrap().0;
        let mut leitor = Reader::from_bytes(&bytes).unwrap();
        leitor.verify_all().unwrap();

        let inicio = Instant::now();
        let (resultado, pico) = memtrack::medir(|| leitor.read_block(0, &NoDictionaries));
        let tempo = inicio.elapsed();
        let erro = resultado.unwrap_err();
        assert_eq!(erro.code(), "block_decode_failed", "{nome}: {erro}");
        assert!(tempo < Duration::from_secs(10), "{nome}: demorou {tempo:?}");
        assert!(
            pico < (20 << 20),
            "{nome}: bloco de 1 MiB com bomba de 1 GiB alocou {pico} bytes"
        );
        eprintln!(
            "BOMBA v4 {nome}: recusada em {} ms, pico {pico} bytes, erro: {erro}",
            tempo.as_millis()
        );
        chegaram_ao_leitor.push(*nome);
    }
    for esperado in ["zstd", "xz", "bzip2"] {
        assert!(
            chegaram_ao_leitor.contains(&esperado),
            "{esperado} não chegou ao leitor: {chegaram_ao_leitor:?}"
        );
    }
}
