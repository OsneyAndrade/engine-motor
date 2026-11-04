use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use prost::Message;

use crate::codec::{self, Codec, CATALOG};
use crate::container::{self, DictionaryResolver, NoDictionaries};
use crate::dict_store::Dictionary;
use crate::memtrack;
use crate::proto::{DictionaryRef, ProcessedFile, TransformStep};
use crate::transform::{self, Transform, WIDTHS};

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
        self.proximo() % n.max(1)
    }

    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| (self.proximo() >> 24) as u8).collect()
    }

    fn bytes_ate(&mut self, max: u64) -> Vec<u8> {
        let n = self.abaixo(max) as usize;
        self.bytes(n)
    }

    fn escolher<T: Copy>(&mut self, itens: &[T]) -> T {
        itens[self.abaixo(itens.len() as u64) as usize]
    }
}

const HOSTIS: [u64; 40] = [
    0,
    1,
    2,
    3,
    4,
    5,
    7,
    8,
    15,
    16,
    31,
    32,
    63,
    64,
    127,
    128,
    255,
    256,
    16_383,
    16_384,
    65_535,
    65_536,
    1 << 20,
    (1 << 31) - 1,
    1 << 31,
    (1 << 32) - 1,
    1 << 32,
    1 << 40,
    1 << 48,
    1 << 56,
    1 << 62,
    (1 << 63) - 1,
    1 << 63,
    u64::MAX - 1,
    u64::MAX,
    usize::MAX as u64,
    usize::MAX as u64 - 1,
    (usize::MAX / 2) as u64,
    (usize::MAX / 2 + 1) as u64,
    (isize::MAX) as u64,
];

fn hostil(rng: &mut Rng) -> u64 {
    match rng.abaixo(10) {
        0..=5 => rng.escolher(&HOSTIS),
        6 => u64::MAX - rng.abaixo(300),
        7 => (usize::MAX as u64).wrapping_sub(rng.abaixo(300)),
        8 => rng.proximo() >> rng.abaixo(64),
        _ => rng.proximo(),
    }
}

fn varint(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn varint_sujo(rng: &mut Rng, out: &mut Vec<u8>) {
    match rng.abaixo(6) {
        0 => out.extend(std::iter::repeat_n(0x80u8, 1 + rng.abaixo(14) as usize)),
        1 => {
            out.extend(std::iter::repeat_n(0xFFu8, 9));
            out.push(rng.escolher(&[0x00, 0x01, 0x7F, 0x80]));
        }
        _ => varint(hostil(rng), out),
    }
}

struct Vigia {
    vivo: Arc<AtomicBool>,
    batida: Arc<AtomicU64>,
    inicio: Instant,
}

impl Vigia {
    fn novo(nome: &'static str) -> Self {
        let vivo = Arc::new(AtomicBool::new(true));
        let batida = Arc::new(AtomicU64::new(0));
        let inicio = Instant::now();
        let (v, b) = (vivo.clone(), batida.clone());
        std::thread::spawn(move || {
            while v.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(500));
                let parado = inicio.elapsed().as_millis() as u64 - b.load(Ordering::Relaxed);
                if parado > 60_000 && v.load(Ordering::Relaxed) {
                    eprintln!("VIGIA {nome}: um caso travou há mais de 60 s; abortando o processo");
                    std::process::abort();
                }
            }
        });
        Vigia { vivo, batida, inicio }
    }

    fn bater(&self) {
        self.batida
            .store(self.inicio.elapsed().as_millis() as u64, Ordering::Relaxed);
    }
}

impl Drop for Vigia {
    fn drop(&mut self) {
        self.vivo.store(false, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct Contagem {
    casos: u64,
    panicos: u64,
    ok: u64,
    erros: u64,
    estouros_de_memoria: u64,
    acima_do_teto: u64,
    pico_max: usize,
    tempo_max: Duration,
}

impl Contagem {
    fn executar<R>(&mut self, vigia: &Vigia, limite_pico: usize, f: impl FnOnce() -> R) -> Option<R> {
        vigia.bater();
        let inicio = Instant::now();
        let resultado = catch_unwind(AssertUnwindSafe(|| memtrack::medir(f)));
        self.casos += 1;
        self.tempo_max = self.tempo_max.max(inicio.elapsed());
        match resultado {
            Ok((valor, pico)) => {
                self.pico_max = self.pico_max.max(pico);
                if pico > limite_pico {
                    self.estouros_de_memoria += 1;
                }
                Some(valor)
            }
            Err(_) => {
                self.panicos += 1;
                None
            }
        }
    }

    fn registrar(&mut self, ok: bool) {
        if ok {
            self.ok += 1;
        } else {
            self.erros += 1;
        }
    }

    fn encerrar(&self, nome: &str, minimo: u64) {
        eprintln!(
            "FUZZ {nome}: casos={} panicos={} saidas_acima_do_teto={} estouros_de_memoria={} ok={} erros={} pico_max_bytes={} tempo_max_ms={}",
            self.casos,
            self.panicos,
            self.acima_do_teto,
            self.estouros_de_memoria,
            self.ok,
            self.erros,
            self.pico_max,
            self.tempo_max.as_millis()
        );
        assert!(self.casos >= minimo, "{nome}: só {} casos", self.casos);
        assert_eq!(self.panicos, 0, "{nome}: {} casos causaram pânico", self.panicos);
        assert_eq!(
            self.acima_do_teto, 0,
            "{nome}: {} casos devolveram saída acima do teto",
            self.acima_do_teto
        );
        assert_eq!(
            self.estouros_de_memoria, 0,
            "{nome}: {} casos alocaram além do limite (pico máximo {} bytes)",
            self.estouros_de_memoria, self.pico_max
        );
        assert!(
            self.tempo_max < Duration::from_secs(10),
            "{nome}: caso lento demais ({:?})",
            self.tempo_max
        );
    }
}

fn mutar(rng: &mut Rng, dados: &mut Vec<u8>) {
    match rng.abaixo(8) {
        0 | 1 | 2 => {
            if dados.is_empty() {
                return;
            }
            for _ in 0..1 + rng.abaixo(4) {
                let i = rng.abaixo(dados.len() as u64) as usize;
                dados[i] = match rng.abaixo(4) {
                    0 => rng.escolher(&[0x00u8, 0x7F, 0x80, 0xFF, 0x01, 0xFE]),
                    1 => dados[i] ^ (1 << rng.abaixo(8)),
                    _ => (rng.proximo() >> 24) as u8,
                };
            }
        }
        3 => {
            let n = rng.abaixo(dados.len() as u64 + 1) as usize;
            dados.truncate(n);
        }
        4 => {
            let n = 1 + rng.abaixo(64) as usize;
            let extra = rng.bytes(n);
            dados.extend_from_slice(&extra);
        }
        5 => {
            if dados.is_empty() {
                return;
            }
            let i = rng.abaixo(dados.len() as u64) as usize;
            let n = 1 + rng.abaixo(8) as usize;
            let extra = rng.bytes(n);
            for (k, b) in extra.into_iter().enumerate() {
                dados.insert(i + k, b);
            }
        }
        6 => {
            if dados.len() < 2 {
                return;
            }
            let i = rng.abaixo(dados.len() as u64) as usize;
            let n = 1 + rng.abaixo((dados.len() - i) as u64) as usize;
            dados.drain(i..i + n);
        }
        _ => {
            if dados.is_empty() {
                return;
            }
            let i = rng.abaixo(dados.len() as u64) as usize;
            let mut sujo = Vec::new();
            varint_sujo(rng, &mut sujo);
            for (k, b) in sujo.into_iter().enumerate() {
                if i + k < dados.len() {
                    dados[i + k] = b;
                }
            }
        }
    }
}

fn passo(rng: &mut Rng, t: Transform) -> TransformStep {
    let width = match rng.abaixo(10) {
        0..=6 => rng.escolher(&WIDTHS),
        7 => 0,
        8 => rng.escolher(&[3u32, 5, 6, 7, 9, 16, 1 << 20]),
        _ => rng.proximo() as u32,
    };
    let input_len = match rng.abaixo(10) {
        0..=3 => 0,
        4..=6 => rng.abaixo(4096),
        7 | 8 => rng.escolher(&HOSTIS),
        _ => rng.proximo(),
    };
    TransformStep {
        transform: t as i32,
        width,
        input_len,
    }
}

fn amostra_valida(rng: &mut Rng, step: &TransformStep) -> Vec<u8> {
    let n = rng.abaixo(600) as usize;
    let base: Vec<u8> = match rng.abaixo(3) {
        0 => rng.bytes(n),
        1 => (0..n).map(|i| (i / 7) as u8).collect(),
        _ => vec![rng.proximo() as u8; n],
    };
    let mut s = *step;
    s.input_len = base.len() as u64;
    if !WIDTHS.contains(&s.width) {
        s.width = 4;
    }
    transform::apply(&base, &s).unwrap_or(base)
}

fn csv_valido(rng: &mut Rng) -> Vec<u8> {
    let colunas = 2 + rng.abaixo(4) as usize;
    let mut s = String::new();
    let mut l = 0usize;
    while l < 4 || s.len() < 80 {
        for c in 0..colunas {
            if c > 0 {
                s.push(',');
            }
            s.push_str(&format!("{}", (l * 31 + c * 7) % 997));
        }
        s.push('\n');
        l += 1;
    }
    if rng.abaixo(2) == 0 {
        s.pop();
    }
    s.into_bytes()
}

fn fuzz_inversao(nome: &'static str, t: Transform, casos: u64, semente: u64, razao_max: usize) {
    let vigia = Vigia::novo(nome);
    let mut rng = Rng::new(semente);
    let mut c = Contagem::default();

    for _ in 0..casos {
        let step = passo(&mut rng, t);
        let mut dados = match rng.abaixo(4) {
            0 => rng.bytes_ate(400),
            1 => {
                let mut v = amostra_valida(&mut rng, &step);
                mutar(&mut rng, &mut v);
                v
            }
            2 => {
                let mut v = amostra_valida(&mut rng, &step);
                if rng.abaixo(2) == 0 {
                    mutar(&mut rng, &mut v);
                }
                v
            }
            _ => {
                let n = 2 * (1 + rng.abaixo(200) as usize);
                let mut v = Vec::with_capacity(n);
                for _ in 0..n / 2 {
                    v.push(129 + rng.abaixo(127) as u8);
                    v.push(rng.proximo() as u8);
                }
                v
            }
        };
        if dados.len() > 4096 {
            dados.truncate(4096);
        }
        let teto = match rng.abaixo(3) {
            0 => usize::MAX,
            1 => rng.abaixo(2048) as usize,
            _ => dados.len().saturating_mul(1 + rng.abaixo(80) as usize),
        };
        let limite_pico = dados.len().saturating_mul(razao_max) + (1 << 20);

        let resultado = c.executar(&vigia, limite_pico, || {
            let a = transform::invert(&dados, &step);
            let b = transform::invert_limited(&dados, &step, teto);
            (a, b)
        });
        if let Some((a, b)) = resultado {
            c.registrar(a.is_ok());
            if let Ok(out) = &a {
                if out.len() > dados.len().saturating_mul(razao_max) + 1024 {
                    c.acima_do_teto += 1;
                }
            }
            if let Ok(out) = &b {
                if out.len() > teto {
                    c.acima_do_teto += 1;
                }
            }
        }
    }
    c.encerrar(nome, casos);
}

#[test]
fn fuzz_delta_com_mais_de_vinte_mil_entradas() {
    fuzz_inversao("delta", Transform::Delta, 25_000, 101, 2);
}

#[test]
fn fuzz_byte_split_com_mais_de_vinte_mil_entradas() {
    fuzz_inversao("byte_split", Transform::ByteSplit, 25_000, 202, 2);
}

#[test]
fn fuzz_rle_com_mais_de_vinte_mil_entradas() {
    fuzz_inversao("rle", Transform::Rle, 25_000, 303, 130);
}

fn csv_cabecalho(rng: &mut Rng, rows: u64, cols: u64, out: &mut Vec<u8>) {
    out.push(0x43);
    out.push(rng.escolher(&[b',', b'\t', b';', 0u8, 0xFF]));
    out.push(rng.escolher(&[0u8, 1, 2, 0xFF]));
    varint(rows, out);
    varint(cols, out);
}

#[test]
fn fuzz_csv_colunar_comprimentos_hostis_nao_estouram_a_soma() {
    let nome = "csv_colunar/comprimentos";
    let vigia = Vigia::novo(nome);
    let mut rng = Rng::new(404);
    let mut c = Contagem::default();
    let casos = 25_000;

    for _ in 0..casos {
        let rows = 1 + rng.abaixo(6);
        let cols = 1 + rng.abaixo(5);
        let total = rows * cols;
        let mut dados = Vec::new();
        csv_cabecalho(&mut rng, rows, cols, &mut dados);

        let hostis = 1 + rng.abaixo(total.min(4));
        let posicoes: Vec<u64> = (0..hostis).map(|_| rng.abaixo(total)).collect();
        for i in 0..total {
            if posicoes.contains(&i) {
                varint_sujo(&mut rng, &mut dados);
            } else {
                varint(rng.abaixo(6), &mut dados);
            }
        }
        let cauda = rng.bytes_ate(80);
        dados.extend_from_slice(&cauda);

        let step = transform::step(Transform::CsvColumnar, 1, 0);
        let limite_pico = dados.len() * 64 + (1 << 20);
        let r = c.executar(&vigia, limite_pico, || transform::invert(&dados, &step));
        if let Some(r) = r {
            c.registrar(r.is_ok());
            if let Ok(out) = r {
                if out.len() > dados.len() * 2 + 2 {
                    c.acima_do_teto += 1;
                }
            }
        }
    }
    c.encerrar(nome, casos);
}

#[test]
fn fuzz_csv_colunar_dimensoes_hostis_nao_travam_nem_alocam() {
    let nome = "csv_colunar/dimensoes";
    let vigia = Vigia::novo(nome);
    let mut rng = Rng::new(505);
    let mut c = Contagem::default();
    let casos = 25_000;

    for _ in 0..casos {
        let (rows, cols) = match rng.abaixo(6) {
            0 => (0, hostil(&mut rng)),
            1 => (hostil(&mut rng), 0),
            2 => (hostil(&mut rng), hostil(&mut rng)),
            3 => (1 + rng.abaixo(8), hostil(&mut rng)),
            4 => (hostil(&mut rng), 1 + rng.abaixo(8)),
            _ => (rng.abaixo(5), rng.abaixo(5)),
        };
        let mut dados = Vec::new();
        csv_cabecalho(&mut rng, rows, cols, &mut dados);
        let cauda = rng.bytes_ate(120);
        dados.extend_from_slice(&cauda);

        let step = transform::step(Transform::CsvColumnar, 1, 0);
        let limite_pico = dados.len() * 64 + (1 << 20);
        let r = c.executar(&vigia, limite_pico, || transform::invert(&dados, &step));
        if let Some(r) = r {
            c.registrar(r.is_ok());
        }
    }
    c.encerrar(nome, casos);
}

#[test]
fn fuzz_csv_colunar_mutacoes_do_formato_valido() {
    let nome = "csv_colunar/mutacoes";
    let vigia = Vigia::novo(nome);
    let mut rng = Rng::new(606);
    let mut c = Contagem::default();
    let casos = 25_000;

    for _ in 0..casos {
        let original = csv_valido(&mut rng);
        let s0 = transform::step(Transform::CsvColumnar, 1, original.len() as u64);
        let mut dados = match transform::apply(&original, &s0) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for _ in 0..1 + rng.abaixo(4) {
            mutar(&mut rng, &mut dados);
        }
        let step = transform::step(
            Transform::CsvColumnar,
            1,
            if rng.abaixo(3) == 0 { original.len() as u64 } else { 0 },
        );
        let limite_pico = dados.len() * 64 + (1 << 20);
        let r = c.executar(&vigia, limite_pico, || transform::invert(&dados, &step));
        if let Some(r) = r {
            c.registrar(r.is_ok());
        }
    }
    c.encerrar(nome, casos);
}

#[test]
fn csv_colunar_valido_continua_reversivel_depois_do_endurecimento() {
    let mut rng = Rng::new(707);
    for _ in 0..500 {
        let original = csv_valido(&mut rng);
        let s = transform::step(Transform::CsvColumnar, 1, original.len() as u64);
        let enc = transform::apply(&original, &s).unwrap();
        assert_eq!(transform::invert(&enc, &s).unwrap(), original);
    }
}

fn amostras_para_codec(rng: &mut Rng) -> Vec<u8> {
    let n = rng.abaixo(3000) as usize;
    match rng.abaixo(3) {
        0 => rng.bytes(n),
        1 => (0..n).map(|i| b"abcdefgh"[i % 8]).collect(),
        _ => vec![0u8; n],
    }
}

#[test]
fn fuzz_dos_decoders_de_todos_os_codecs() {
    let nome = "codecs";
    let vigia = Vigia::novo(nome);
    let mut rng = Rng::new(808);
    let mut c = Contagem::default();
    let por_codec = 3_000u64;
    let teto = 1u64 << 20;

    for info in CATALOG {
        for _ in 0..por_codec {
            let original = amostras_para_codec(&mut rng);
            let mut payload = match rng.abaixo(4) {
                0 => rng.bytes_ate(200),
                _ => codec::encode(&original, info.codec, info.default_level, None).unwrap(),
            };
            if rng.abaixo(5) != 0 {
                for _ in 0..1 + rng.abaixo(3) {
                    mutar(&mut rng, &mut payload);
                }
            }
            let esperado = match rng.abaixo(3) {
                0 => None,
                1 => Some(original.len() as u64),
                _ => Some(hostil(&mut rng)),
            };
            let r = c.executar(&vigia, 48 << 20, || {
                codec::decode_limited(&payload, info.codec, None, esperado, teto)
            });
            if let Some(r) = r {
                c.registrar(r.is_ok());
                if let Ok(out) = r {
                    if out.len() as u64 > teto {
                        c.acima_do_teto += 1;
                    }
                }
            }
        }
    }
    c.encerrar(nome, por_codec * CATALOG.len() as u64);
}

struct SemDicionario;

impl DictionaryResolver for SemDicionario {
    fn resolve(&self, _id: &str) -> Option<Arc<Dictionary>> {
        None
    }
}

fn envelope_forjado(rng: &mut Rng, tamanho_hostil: bool) -> ProcessedFile {
    let original = amostras_para_codec(rng);
    let codec_id = match rng.abaixo(8) {
        0 => rng.escolher(&[-3i32, -1, 0, 8, 9, 100, i32::MAX, i32::MIN]),
        _ => rng.escolher(&[1i32, 2, 3, 4, 5, 6, 7]),
    };
    let payload = match rng.abaixo(4) {
        0 => rng.bytes_ate(300),
        _ => {
            let c = Codec::try_from(codec_id).unwrap_or(Codec::Stored);
            let mut p = codec::encode(&original, c, codec::default_level(c), None)
                .unwrap_or_else(|_| original.clone());
            if rng.abaixo(2) == 0 {
                mutar(rng, &mut p);
            }
            p
        }
    };
    let passos = rng.abaixo(5);
    let transforms: Vec<TransformStep> = (0..passos)
        .map(|_| {
            let t = rng.escolher(&[0i32, 1, 2, 3, 4, 5, 77, -1]);
            let mut s = passo(rng, Transform::try_from(t).unwrap_or(Transform::Rle));
            s.transform = t;
            s
        })
        .collect();
    let original_size = if tamanho_hostil {
        hostil(rng)
    } else {
        match rng.abaixo(4) {
            0 => 0,
            1 => original.len() as u64,
            2 => rng.abaixo(5000),
            _ => original.len() as u64 + rng.abaixo(10),
        }
    };
    let dictionary = match rng.abaixo(6) {
        0 => Some(DictionaryRef {
            id: rng.escolher(&[
                "../../../etc/passwd",
                "a/b",
                "..",
                "x.0123456789abcdef",
                "",
                "\u{0}",
            ])
            .to_string(),
            digest: rng.bytes(32),
            size: hostil(rng) as u32,
        }),
        _ => None,
    };
    let file_hash = match rng.abaixo(4) {
        0 => Vec::new(),
        1 => rng.bytes_ate(40),
        2 => blake3::hash(&original).as_bytes().to_vec(),
        _ => rng.bytes(32),
    };

    ProcessedFile {
        version: rng.escolher(&[0i32, 1, 2, 3, 3, 3, 4, 99, -1]),
        original_size,
        data: payload,
        codec: codec_id,
        algorithm: rng.escolher(&[0i32, 1, 2, 3, 4, 5, 6, 77]),
        transforms,
        dictionary,
        file_hash,
        ..Default::default()
    }
}

fn fuzz_envelopes_forjados(nome: &'static str, tamanho_hostil: bool, semente: u64) {
    let vigia = Vigia::novo(nome);
    let mut rng = Rng::new(semente);
    let mut c = Contagem::default();
    let casos = 20_000;
    let teto = 16u64 << 20;

    for _ in 0..casos {
        let env = envelope_forjado(&mut rng, tamanho_hostil);
        let bytes = match container::seal(env) {
            Ok(s) => s.bytes,
            Err(_) => continue,
        };
        let r = c.executar(&vigia, 96 << 20, || {
            let env = container::parse(&bytes)?;
            container::restore_with_limit(&env, &SemDicionario, teto).map(|r| r.data.len())
        });
        if let Some(r) = r {
            c.registrar(r.is_ok());
            if let Ok(n) = r {
                if n as u64 > teto {
                    c.acima_do_teto += 1;
                }
            }
        }
    }
    c.encerrar(nome, casos);
}

#[test]
fn fuzz_envelopes_forjados_com_checksum_valido() {
    fuzz_envelopes_forjados("envelopes_forjados", false, 909);
}

#[test]
fn fuzz_envelopes_forjados_com_tamanho_original_hostil() {
    fuzz_envelopes_forjados("envelopes_forjados/original_size", true, 1010);
}

fn dir_dourado() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/testdata/golden")
}

struct Dicionarios(Vec<Arc<Dictionary>>);

impl DictionaryResolver for Dicionarios {
    fn resolve(&self, id: &str) -> Option<Arc<Dictionary>> {
        self.0.iter().find(|d| d.id == id).cloned()
    }
}

fn carregar_corpus() -> (Vec<(String, Vec<u8>)>, Dicionarios) {
    let dir = dir_dourado();
    let manifesto: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();

    let mut dicts = Vec::new();
    for d in manifesto["dicionarios"].as_array().unwrap() {
        let bytes = std::fs::read(dir.join(d["arquivo"].as_str().unwrap())).unwrap();
        let digest: [u8; 32] = blake3::hash(&bytes).into();
        dicts.push(Arc::new(Dictionary {
            id: d["id"].as_str().unwrap().to_string(),
            digest,
            bytes,
        }));
    }

    let mut envelopes = Vec::new();
    for caso in manifesto["casos"].as_array().unwrap() {
        let caminho = dir.join(caso["envelope"].as_str().unwrap());
        envelopes.push((
            caso["nome"].as_str().unwrap().to_string(),
            std::fs::read(caminho).unwrap(),
        ));
    }
    (envelopes, Dicionarios(dicts))
}

fn mutar_envelope_serializado(rng: &mut Rng, bytes: &mut Vec<u8>) {
    let n = 1 + rng.abaixo(4);
    for _ in 0..n {
        if bytes.is_empty() {
            return;
        }
        let len = bytes.len() as u64;
        let i = match rng.abaixo(4) {
            0 => rng.abaixo(len.min(48)),
            1 => rng.abaixo(len),
            2 => len - 1 - rng.abaixo(len.min(48)),
            _ => len / 2 + rng.abaixo((len / 4).max(1)).min(len - len / 2 - 1),
        } as usize;
        bytes[i] = match rng.abaixo(5) {
            0 => rng.escolher(&[0x00u8, 0x7F, 0x80, 0xFF]),
            1 => bytes[i] ^ (1 << rng.abaixo(8)),
            2 => bytes[i].wrapping_add(1 + rng.abaixo(4) as u8),
            _ => (rng.proximo() >> 24) as u8,
        };
    }
}

fn mutar_camada_de_transform(rng: &mut Rng, env: &mut ProcessedFile) -> bool {
    if env.version < 3 || env.dictionary.is_some() || env.transforms.is_empty() {
        return false;
    }
    let Ok(codec_id) = Codec::try_from(env.codec) else {
        return false;
    };
    let Ok(mut staged) = codec::decode_limited(&env.data, codec_id, None, None, 64 << 20) else {
        return false;
    };
    for _ in 0..1 + rng.abaixo(3) {
        if rng.abaixo(2) == 0 && staged.len() > 16 {
            let i = rng.abaixo(staged.len().min(24) as u64) as usize;
            let mut sujo = Vec::new();
            varint_sujo(rng, &mut sujo);
            for (k, b) in sujo.into_iter().enumerate() {
                if i + k < staged.len() {
                    staged[i + k] = b;
                }
            }
        } else {
            mutar(rng, &mut staged);
        }
    }
    match codec::encode(&staged, codec_id, codec::clamp_level(codec_id, 1), None) {
        Ok(enc) => {
            env.data = enc;
            true
        }
        Err(_) => false,
    }
}

fn mutar_envelope_estruturado(rng: &mut Rng, env: &mut ProcessedFile) {
    for _ in 0..1 + rng.abaixo(4) {
        match rng.abaixo(9) {
            0 | 1 => mutar(rng, &mut env.data),
            2 => env.codec = rng.escolher(&[0i32, 1, 2, 3, 4, 5, 6, 7, 8, -1, 99]),
            3 => env.original_size = hostil(rng),
            4 => env.original_size = env.original_size.wrapping_add(rng.abaixo(3)).wrapping_sub(1),
            5 => {
                if !env.transforms.is_empty() {
                    let i = rng.abaixo(env.transforms.len() as u64) as usize;
                    env.transforms[i].width = rng.proximo() as u32;
                }
            }
            6 => {
                if !env.transforms.is_empty() {
                    let i = rng.abaixo(env.transforms.len() as u64) as usize;
                    env.transforms[i].input_len = hostil(rng);
                }
            }
            7 => {
                let t = rng.escolher(&[1i32, 2, 3, 4, 5]);
                env.transforms.push(passo(rng, Transform::try_from(t).unwrap_or(Transform::Rle)));
            }
            _ => {
                env.version = rng.escolher(&[0i32, 1, 2, 3, 4, 99]);
                let n = rng.escolher(&[0usize, 8, 32, 33]);
                env.file_hash = rng.bytes(n);
            }
        }
    }
}

#[test]
fn fuzz_de_mutacao_do_corpus_dourado_nao_causa_panico() {
    let nome = "corpus_dourado";
    let vigia = Vigia::novo(nome);
    let (envelopes, dicts) = carregar_corpus();
    assert!(envelopes.len() >= 40, "corpus pequeno: {}", envelopes.len());

    let mut rng = Rng::new(1111);
    let mut c = Contagem::default();
    let por_envelope = 400u64;
    let teto = 32u64 << 20;
    let mut aceitos_no_parse = 0u64;
    let mut restaurados = 0u64;
    let mut codigos: std::collections::BTreeMap<&'static str, u64> = Default::default();

    for (_, original) in &envelopes {
        for caso in 0..por_envelope {
            let bytes = if caso % 3 == 0 {
                let mut b = original.clone();
                mutar_envelope_serializado(&mut rng, &mut b);
                b
            } else {
                let mut env = match ProcessedFile::decode(original.as_slice()) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if caso % 3 == 1 || !mutar_camada_de_transform(&mut rng, &mut env) {
                    mutar_envelope_estruturado(&mut rng, &mut env);
                }
                match container::seal(env) {
                    Ok(s) => s.bytes,
                    Err(_) => continue,
                }
            };

            let r = c.executar(&vigia, 192 << 20, || {
                let env = container::parse(&bytes)?;
                container::restore_with_limit(&env, &dicts, teto).map(|r| r.data.len())
            });
            if let Some(r) = r {
                match &r {
                    Ok(_) => {
                        aceitos_no_parse += 1;
                        restaurados += 1;
                    }
                    Err(e) => {
                        if e.code() != "envelope_invalid" && e.code() != "envelope_corrupted" {
                            aceitos_no_parse += 1;
                        }
                        *codigos.entry(e.code()).or_default() += 1;
                    }
                }
                c.registrar(r.is_ok());
            }
        }
    }

    eprintln!(
        "FUZZ corpus_dourado/detalhe: aceitos_no_parse={aceitos_no_parse} restaurados={restaurados} codigos={codigos:?}"
    );
    assert!(aceitos_no_parse > 1000, "a mutação não chega ao restore: {aceitos_no_parse}");
    c.encerrar(nome, 20_000);
}

#[test]
fn fuzz_do_corpus_dourado_sem_mutacao_continua_restaurando() {
    let (envelopes, dicts) = carregar_corpus();
    for (nome, bytes) in &envelopes {
        let env = container::parse(bytes).unwrap_or_else(|e| panic!("{nome}: {e}"));
        container::restore_with_limit(&env, &dicts, 8 << 20)
            .unwrap_or_else(|e| panic!("{nome}: {e}"));
        container::restore_with_limit(&env, &NoDictionaries, 8 << 20).ok();
    }
}
