use std::cell::RefCell;

use zstd::bulk::Compressor;

pub const SAMPLE_BYTES: usize = 4096;
pub const PIECES: usize = 4;
pub const MIN_SAVING_PERCENT: usize = 2;
pub const PROBE_LEVEL: i32 = 1;

const PIECE_BYTES: usize = SAMPLE_BYTES / PIECES;
const OUT_BYTES: usize = SAMPLE_BYTES + SAMPLE_BYTES / 128 + 512;

thread_local! {
    static CONTEXT: RefCell<Option<Compressor<'static>>> =
        RefCell::new(Compressor::new(PROBE_LEVEL).ok());
}

fn compressed_len(data: &[u8]) -> Option<usize> {
    let mut out = [0u8; OUT_BYTES];
    CONTEXT.with(|cell| {
        let mut slot = cell.borrow_mut();
        let ctx = slot.as_mut()?;
        ctx.compress_to_buffer(data, &mut out).ok()
    })
}

pub fn piece_start(block_len: usize, piece: usize) -> usize {
    (2 * piece + 1) * block_len / (2 * PIECES) - PIECE_BYTES / 2
}

pub fn sample<'a>(block: &'a [u8], buf: &'a mut [u8; SAMPLE_BYTES]) -> &'a [u8] {
    if block.len() <= SAMPLE_BYTES {
        return block;
    }
    for i in 0..PIECES {
        let start = piece_start(block.len(), i);
        buf[i * PIECE_BYTES..(i + 1) * PIECE_BYTES]
            .copy_from_slice(&block[start..start + PIECE_BYTES]);
    }
    &buf[..]
}

pub fn incompressible(block: &[u8]) -> bool {
    if block.is_empty() {
        return true;
    }
    let mut buf = [0u8; SAMPLE_BYTES];
    let data = sample(block, &mut buf);
    match compressed_len(data) {
        Some(len) => len * 100 >= data.len() * (100 - MIN_SAVING_PERCENT),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{self, Codec};

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

        fn bytes(&mut self, n: usize) -> Vec<u8> {
            (0..n).map(|_| (self.proximo() >> 24) as u8).collect()
        }
    }

    fn aleatorio(n: usize) -> Vec<u8> {
        Rng::new(0xA11CE).bytes(n)
    }

    fn texto(n: usize) -> Vec<u8> {
        const PALAVRAS: [&str; 24] = [
            "o", "motor", "comprime", "dados", "sem", "perdas", "e", "reconstroi", "cada", "byte",
            "do", "arquivo", "original", "com", "integridade", "verificada", "por", "hash",
            "no", "cofre", "de", "essencias", "bloco", "plano",
        ];
        let mut rng = Rng::new(0xBEEF);
        let mut out = Vec::with_capacity(n + 16);
        while out.len() < n {
            out.extend_from_slice(PALAVRAS[(rng.proximo() % 24) as usize].as_bytes());
            out.push(if rng.proximo() % 9 == 0 { b'\n' } else { b' ' });
        }
        out.truncate(n);
        out
    }

    fn csv(n: usize) -> Vec<u8> {
        let mut rng = Rng::new(0xC5F);
        let mut out = String::from("id,data,regiao,produto,valor\n");
        let mut i = 0u64;
        while out.len() < n {
            out.push_str(&format!(
                "{},2026-{:02}-{:02},sudeste,produto-{},{}.{:02}\n",
                i,
                1 + rng.proximo() % 12,
                1 + rng.proximo() % 28,
                rng.proximo() % 50,
                i * 13,
                i % 100
            ));
            i += 1;
        }
        let mut b = out.into_bytes();
        b.truncate(n);
        b
    }

    fn floats(n: usize) -> Vec<u8> {
        let mut rng = Rng::new(0xF10A7);
        let mut out = Vec::with_capacity(n + 4);
        let mut i = 0u32;
        while out.len() < n {
            let v = 20.0f32 + (i as f32 * 0.001).sin() * 5.0 + (rng.proximo() % 100) as f32 * 1e-4;
            out.extend_from_slice(&v.to_le_bytes());
            i += 1;
        }
        out.truncate(n);
        out
    }

    const MIB: usize = 1 << 20;

    #[test]
    fn aleatorio_e_incompressivel() {
        for n in [4096usize, 5000, 65_536, MIB] {
            assert!(incompressible(&aleatorio(n)), "n={n}");
        }
    }

    #[test]
    fn dado_ja_comprimido_e_incompressivel() {
        let base = texto(4 * MIB);
        for (nome, c, nivel) in [
            ("zstd", Codec::Zstd, 3),
            ("xz", Codec::Xz, 1),
            ("brotli", Codec::Brotli, 5),
            ("deflate", Codec::Deflate, 6),
            ("bzip2", Codec::Bzip2, 9),
        ] {
            let comprimido = codec::encode(&base, c, nivel, None).unwrap();
            assert!(comprimido.len() > 64 * 1024, "{nome}: amostra pequena demais");
            let bloco = &comprimido[..comprimido.len().min(MIB)];
            assert!(incompressible(bloco), "{nome} deveria parecer incompressível");
        }
    }

    #[test]
    fn texto_csv_e_zeros_sao_compressiveis() {
        assert!(!incompressible(&texto(MIB)));
        assert!(!incompressible(&texto(100_000)));
        assert!(!incompressible(&csv(MIB)));
        assert!(!incompressible(&csv(64 * 1024)));
        assert!(!incompressible(&vec![0u8; MIB]));
        assert!(!incompressible(&vec![0u8; 4096]));
        assert!(!incompressible(&vec![7u8; 1000]));
    }

    #[test]
    fn numerico_suave_nao_e_descartado() {
        assert!(!incompressible(&floats(MIB)));
    }

    #[test]
    fn blocos_menores_que_a_amostra_usam_o_bloco_inteiro() {
        assert!(incompressible(&aleatorio(1000)));
        assert!(!incompressible(&texto(1000)));
        assert!(!incompressible(&[0u8; 100]));
        assert!(incompressible(&[1u8]));
        assert!(incompressible(&[]));

        let mut buf = [0u8; SAMPLE_BYTES];
        let curto = texto(SAMPLE_BYTES);
        assert_eq!(sample(&curto, &mut buf), &curto[..]);
        let menor = texto(10);
        assert_eq!(sample(&menor, &mut buf), &menor[..]);
    }

    #[test]
    fn amostra_tem_quatro_pedacos_distribuidos() {
        let bloco: Vec<u8> = (0..MIB).map(|i| (i / 7 % 251) as u8).collect();
        let mut buf = [0u8; SAMPLE_BYTES];
        let s = sample(&bloco, &mut buf).to_vec();
        assert_eq!(s.len(), SAMPLE_BYTES);
        let mut anterior_fim = 0usize;
        for i in 0..PIECES {
            let start = piece_start(bloco.len(), i);
            assert_eq!(
                &s[i * PIECE_BYTES..(i + 1) * PIECE_BYTES],
                &bloco[start..start + PIECE_BYTES],
                "pedaço {i}"
            );
            assert!(start >= anterior_fim, "pedaços sobrepostos");
            anterior_fim = start + PIECE_BYTES;
            let quarto = bloco.len() / PIECES;
            assert!(start >= i * quarto && start + PIECE_BYTES <= (i + 1) * quarto);
        }
        assert!(piece_start(bloco.len(), 0) > 0, "não deve incluir o início do bloco");
        for n in [SAMPLE_BYTES + 1, SAMPLE_BYTES + 7, 5000, 8191, 1 << 26] {
            for i in 0..PIECES {
                assert!(piece_start(n, i) + PIECE_BYTES <= n, "n={n} peça {i}");
            }
        }
    }

    #[test]
    fn limiar_de_dois_por_cento() {
        let mut rng = Rng::new(5);
        let mut bloco = rng.bytes(MIB);
        assert!(incompressible(&bloco));
        for b in bloco.iter_mut() {
            if *b < 128 {
                *b = 0;
            }
        }
        assert!(!incompressible(&bloco));
    }

    #[test]
    fn decisao_e_deterministica_entre_chamadas_e_threads() {
        let a = aleatorio(MIB);
        let t = texto(MIB);
        let esperado = (incompressible(&a), incompressible(&t));
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let (a, t) = (a.clone(), t.clone());
                std::thread::spawn(move || (incompressible(&a), incompressible(&t)))
            })
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap(), esperado);
        }
        assert_eq!(esperado, (true, false));
    }

    fn medir<F: FnMut()>(iteracoes: usize, mut f: F) -> f64 {
        for _ in 0..(iteracoes / 10).max(1) {
            f();
        }
        let t0 = std::time::Instant::now();
        for _ in 0..iteracoes {
            f();
        }
        t0.elapsed().as_secs_f64() * 1e6 / iteracoes as f64
    }

    #[test]
    #[ignore]
    fn microbenchmark_do_probe_em_bloco_de_1_mib() {
        let aleatorio = aleatorio(MIB);
        let texto = texto(MIB);
        let csv = csv(MIB);

        let probe_aleatorio = medir(20_000, || {
            std::hint::black_box(incompressible(std::hint::black_box(&aleatorio)));
        });
        let probe_texto = medir(20_000, || {
            std::hint::black_box(incompressible(std::hint::black_box(&texto)));
        });
        let probe_csv = medir(20_000, || {
            std::hint::black_box(incompressible(std::hint::black_box(&csv)));
        });
        let zstd3_aleatorio = medir(200, || {
            std::hint::black_box(
                codec::encode(std::hint::black_box(&aleatorio), Codec::Zstd, 3, None).unwrap(),
            );
        });
        let zstd1_aleatorio = medir(200, || {
            std::hint::black_box(
                codec::encode(std::hint::black_box(&aleatorio), Codec::Zstd, 1, None).unwrap(),
            );
        });
        let zstd3_texto = medir(100, || {
            std::hint::black_box(
                codec::encode(std::hint::black_box(&texto), Codec::Zstd, 3, None).unwrap(),
            );
        });
        let mut buf = [0u8; SAMPLE_BYTES];
        let coleta = medir(100_000, || {
            std::hint::black_box(sample(std::hint::black_box(&aleatorio), &mut buf));
        });

        println!("--- probe vs zstd por bloco de 1 MiB (µs por bloco) ---");
        println!("probe(aleatório)      {probe_aleatorio:10.2} µs");
        println!("probe(texto)          {probe_texto:10.2} µs");
        println!("probe(csv)            {probe_csv:10.2} µs");
        println!("só a coleta (4x1 KiB) {coleta:10.2} µs");
        println!("zstd-1(aleatório)     {zstd1_aleatorio:10.2} µs");
        println!("zstd-3(aleatório)     {zstd3_aleatorio:10.2} µs");
        println!("zstd-3(texto)         {zstd3_texto:10.2} µs");
        println!(
            "probe / zstd-3(aleatório) = {:.2}%   probe / zstd-3(texto) = {:.2}%",
            probe_aleatorio / zstd3_aleatorio * 100.0,
            probe_aleatorio / zstd3_texto * 100.0
        );
        println!(
            "zstd-3 em dado incompressível: {:.2} GB/s por núcleo",
            MIB as f64 / (zstd3_aleatorio * 1e-6) / 1e9
        );
    }
}
