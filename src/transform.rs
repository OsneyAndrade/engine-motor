use std::io;

pub use crate::proto::{Transform, TransformStep};

pub const WIDTHS: [u32; 4] = [1, 2, 4, 8];

pub fn transform_name(t: Transform) -> &'static str {
    match t {
        Transform::Unspecified => "none",
        Transform::Delta => "delta",
        Transform::ByteSplit => "byte_split",
        Transform::Rle => "rle",
        Transform::CsvColumnar => "csv_columnar",
    }
}

pub fn chain_label(steps: &[TransformStep]) -> String {
    if steps.is_empty() {
        return "none".to_string();
    }
    steps
        .iter()
        .map(|s| {
            let t = Transform::try_from(s.transform).unwrap_or(Transform::Unspecified);
            match t {
                Transform::Delta | Transform::ByteSplit => {
                    format!("{}:{}", transform_name(t), s.width)
                }
                _ => transform_name(t).to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(">")
}

pub fn step(transform: Transform, width: u32, input_len: u64) -> TransformStep {
    TransformStep {
        transform: transform as i32,
        width,
        input_len,
    }
}

pub fn apply(data: &[u8], s: &TransformStep) -> io::Result<Vec<u8>> {
    let t = Transform::try_from(s.transform)
        .map_err(|_| invalid(format!("transform desconhecido: {}", s.transform)))?;
    match t {
        Transform::Unspecified => Ok(data.to_vec()),
        Transform::Delta => Ok(delta_apply(data, width_of(s)?)),
        Transform::ByteSplit => Ok(byte_split_apply(data, width_of(s)?)),
        Transform::Rle => Ok(rle_apply(data)),
        Transform::CsvColumnar => csv_apply(data)
            .ok_or_else(|| invalid("entrada não é tabular uniforme".to_string())),
    }
}

pub const MAX_CHAIN: usize = 8;

fn default_cap() -> usize {
    usize::try_from(crate::codec::MAX_DECOMPRESSED).unwrap_or(usize::MAX)
}

fn check_len(len: usize, limit: usize) -> io::Result<()> {
    if len > limit {
        Err(crate::codec::too_large(limit as u64))
    } else {
        Ok(())
    }
}

pub fn invert(data: &[u8], s: &TransformStep) -> io::Result<Vec<u8>> {
    invert_limited(data, s, default_cap())
}

pub fn invert_limited(data: &[u8], s: &TransformStep, max_out: usize) -> io::Result<Vec<u8>> {
    let t = Transform::try_from(s.transform)
        .map_err(|_| invalid(format!("transform desconhecido no envelope: {}", s.transform)))?;

    let declared = usize::try_from(s.input_len).unwrap_or(usize::MAX);
    let limit = if s.input_len != 0 { max_out.min(declared) } else { max_out };

    let produced = match t {
        Transform::Unspecified => check_len(data.len(), limit).map(|_| data.to_vec()),
        Transform::Delta => width_of(s)
            .and_then(|w| check_len(data.len(), limit).map(|_| delta_invert(data, w))),
        Transform::ByteSplit => width_of(s)
            .and_then(|w| check_len(data.len(), limit).map(|_| byte_split_invert(data, w))),
        Transform::Rle => rle_invert_limited(data, limit),
        Transform::CsvColumnar => csv_invert(data, limit),
    };

    let out = match produced {
        Ok(out) => out,
        Err(e) if s.input_len != 0 && declared < max_out && crate::codec::too_large_limit(&e).is_some() => {
            return Err(invalid(format!(
                "inversão de {} excede o tamanho declarado ({})",
                transform_name(t),
                s.input_len
            )));
        }
        Err(e) => return Err(e),
    };

    if s.input_len != 0 && out.len() as u64 != s.input_len {
        return Err(invalid(format!(
            "inversão de {} devolveu {} bytes, esperado {}",
            transform_name(t),
            out.len(),
            s.input_len
        )));
    }
    Ok(out)
}

pub fn apply_chain(data: &[u8], steps: &mut [TransformStep]) -> io::Result<Vec<u8>> {
    let mut current = data.to_vec();
    for s in steps.iter_mut() {
        s.input_len = current.len() as u64;
        current = apply(&current, s)?;
    }
    Ok(current)
}

pub fn invert_chain(data: &[u8], steps: &[TransformStep]) -> io::Result<Vec<u8>> {
    invert_chain_limited(data, steps, default_cap(), default_cap())
}

pub fn invert_chain_limited(
    data: &[u8],
    steps: &[TransformStep],
    stage_cap: usize,
    final_cap: usize,
) -> io::Result<Vec<u8>> {
    if steps.len() > MAX_CHAIN {
        return Err(invalid(format!(
            "cadeia de {} transforms excede o máximo de {MAX_CHAIN}",
            steps.len()
        )));
    }
    if steps.is_empty() {
        check_len(data.len(), final_cap)?;
        return Ok(data.to_vec());
    }

    let mut current: Option<Vec<u8>> = None;
    for (i, s) in steps.iter().enumerate().rev() {
        let cap = if i == 0 { final_cap } else { stage_cap };
        let input: &[u8] = current.as_deref().unwrap_or(data);
        let out = invert_limited(input, s, cap)?;
        current = Some(out);
    }
    Ok(current.unwrap_or_default())
}

fn width_of(s: &TransformStep) -> io::Result<usize> {
    if WIDTHS.contains(&s.width) {
        Ok(s.width as usize)
    } else {
        Err(invalid(format!("largura de elemento inválida: {}", s.width)))
    }
}

fn invalid(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

fn delta_apply(data: &[u8], width: usize) -> Vec<u8> {
    let n = data.len() / width;
    let mut out = Vec::with_capacity(data.len());
    let mut prev = [0u8; 8];

    for i in 0..n {
        let chunk = &data[i * width..(i + 1) * width];
        let cur = load_le(chunk);
        let prv = load_le(&prev[..width]);
        let diff = cur.wrapping_sub(prv);
        out.extend_from_slice(&diff.to_le_bytes()[..width]);
        prev[..width].copy_from_slice(chunk);
    }

    out.extend_from_slice(&data[n * width..]);
    out
}

fn delta_invert(data: &[u8], width: usize) -> Vec<u8> {
    let n = data.len() / width;
    let mut out = Vec::with_capacity(data.len());
    let mut acc: u64 = 0;

    for i in 0..n {
        let chunk = &data[i * width..(i + 1) * width];
        acc = acc.wrapping_add(load_le(chunk));
        let bytes = acc.to_le_bytes();
        out.extend_from_slice(&bytes[..width]);
        acc = load_le(&bytes[..width]);
    }

    out.extend_from_slice(&data[n * width..]);
    out
}

#[inline]
fn load_le(bytes: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    buf[..bytes.len()].copy_from_slice(bytes);
    u64::from_le_bytes(buf)
}

fn byte_split_apply(data: &[u8], width: usize) -> Vec<u8> {
    let n = data.len() / width;
    let mut out = vec![0u8; data.len()];

    for plane in 0..width {
        let base = plane * n;
        for i in 0..n {
            out[base + i] = data[i * width + plane];
        }
    }

    let tail = n * width;
    out[tail..].copy_from_slice(&data[tail..]);
    out
}

fn byte_split_invert(data: &[u8], width: usize) -> Vec<u8> {
    let n = data.len() / width;
    let mut out = vec![0u8; data.len()];

    for plane in 0..width {
        let base = plane * n;
        for i in 0..n {
            out[i * width + plane] = data[base + i];
        }
    }

    let tail = n * width;
    out[tail..].copy_from_slice(&data[tail..]);
    out
}

fn rle_apply(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 100 + 8);
    let mut i = 0usize;

    while i < data.len() {
        let b = data[i];
        let mut run = 1usize;
        while i + run < data.len() && data[i + run] == b && run < 128 {
            run += 1;
        }

        if run >= 2 {
            out.push((257 - run) as u8);
            out.push(b);
            i += run;
            continue;
        }

        let start = i;
        i += 1;
        while i < data.len() && i - start < 128 {
            if data.len() - i >= 2 && data[i] == data[i + 1] {
                break;
            }
            i += 1;
        }
        let lit = &data[start..i];
        debug_assert!(!lit.is_empty());
        out.push((lit.len() - 1) as u8);
        out.extend_from_slice(lit);
    }

    out
}

#[cfg(test)]
fn rle_invert(data: &[u8]) -> io::Result<Vec<u8>> {
    rle_invert_limited(data, default_cap())
}

fn rle_invert_limited(data: &[u8], limit: usize) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len().saturating_mul(2).min(limit));
    let mut i = 0usize;

    while i < data.len() {
        let ctrl = data[i];
        i += 1;

        if ctrl < 128 {
            let n = ctrl as usize + 1;
            if i + n > data.len() {
                return Err(invalid("RLE truncado em bloco literal".to_string()));
            }
            check_len(out.len().saturating_add(n), limit)?;
            out.extend_from_slice(&data[i..i + n]);
            i += n;
        } else if ctrl > 128 {
            let n = 257 - ctrl as usize;
            if i >= data.len() {
                return Err(invalid("RLE truncado em bloco de repetição".to_string()));
            }
            let b = data[i];
            i += 1;
            check_len(out.len().saturating_add(n), limit)?;
            out.resize(out.len() + n, b);
        } else {
            return Err(invalid("byte de controle RLE reservado (128)".to_string()));
        }
    }

    Ok(out)
}

const CSV_MAGIC: u8 = 0x43;
const CSV_DELIMS: [u8; 3] = [b',', b'\t', b';'];
const MAX_FIELDS: usize = 32 * 1024 * 1024;

pub fn csv_detect(data: &[u8]) -> Option<(u8, usize)> {
    if data.len() < 64 {
        return None;
    }

    let probe_end = data.len().min(256 * 1024);
    let probe = &data[..probe_end];

    let mut best: Option<(u8, usize)> = None;
    for delim in CSV_DELIMS {
        let mut counts = probe
            .split(|&b| b == b'\n')
            .take(32)
            .filter(|l| !l.is_empty())
            .map(|l| l.iter().filter(|&&b| b == delim).count());

        let first = counts.next()?;
        if first == 0 {
            continue;
        }
        if counts.all(|c| c == first) {
            let cols = first + 1;
            if best.map(|(_, c)| cols > c).unwrap_or(true) {
                best = Some((delim, cols));
            }
        }
    }

    let (delim, cols) = best?;
    if cols < 2 {
        return None;
    }

    let trailing = data.last() == Some(&b'\n');
    let body = if trailing { &data[..data.len() - 1] } else { data };

    let mut rows = 0usize;
    for line in body.split(|&b| b == b'\n') {
        if line.iter().filter(|&&b| b == delim).count() + 1 != cols {
            return None;
        }
        rows += 1;
        if rows > MAX_FIELDS / cols {
            return None;
        }
    }
    if rows < 4 {
        return None;
    }

    Some((delim, cols))
}

fn split_lines(data: &[u8]) -> (Vec<&[u8]>, bool) {
    let trailing = data.last() == Some(&b'\n');
    let body = if trailing { &data[..data.len() - 1] } else { data };
    (body.split(|&b| b == b'\n').collect(), trailing)
}

fn csv_apply(data: &[u8]) -> Option<Vec<u8>> {
    let (delim, cols) = csv_detect(data)?;
    let (lines, trailing) = split_lines(data);
    let rows = lines.len();
    if rows.checked_mul(cols)? > MAX_FIELDS {
        return None;
    }

    let mut fields: Vec<&[u8]> = Vec::with_capacity(rows * cols);
    for line in &lines {
        let mut n = 0;
        for f in line.split(|&b| b == delim) {
            fields.push(f);
            n += 1;
        }
        if n != cols {
            return None;
        }
    }

    let mut out = Vec::with_capacity(data.len() + 16 + rows * cols);
    out.push(CSV_MAGIC);
    out.push(delim);
    out.push(trailing as u8);
    put_varint(&mut out, rows as u64);
    put_varint(&mut out, cols as u64);

    for c in 0..cols {
        for r in 0..rows {
            put_varint(&mut out, fields[r * cols + c].len() as u64);
        }
    }
    for c in 0..cols {
        for r in 0..rows {
            out.extend_from_slice(fields[r * cols + c]);
        }
    }

    Some(out)
}

fn csv_invert(data: &[u8], limit: usize) -> io::Result<Vec<u8>> {
    if data.len() < 5 || data[0] != CSV_MAGIC {
        return Err(invalid("cabeçalho colunar inválido".to_string()));
    }
    let delim = data[1];
    let trailing = data[2] != 0;
    let mut p = 3usize;

    let rows = usize::try_from(get_varint(data, &mut p)?)
        .map_err(|_| invalid("dimensões colunares absurdas".to_string()))?;
    let cols = usize::try_from(get_varint(data, &mut p)?)
        .map_err(|_| invalid("dimensões colunares absurdas".to_string()))?;
    if rows == 0 || cols == 0 {
        return Err(invalid("dimensões colunares inválidas".to_string()));
    }
    let total = rows
        .checked_mul(cols)
        .ok_or_else(|| invalid("dimensões colunares absurdas".to_string()))?;
    if total > MAX_FIELDS || total > data.len() - p {
        return Err(invalid("dimensões colunares inconsistentes".to_string()));
    }

    let mut lens = Vec::with_capacity(total);
    let mut payload = 0usize;
    for _ in 0..total {
        let len = usize::try_from(get_varint(data, &mut p)?)
            .map_err(|_| invalid("campo colunar absurdo".to_string()))?;
        payload = payload
            .checked_add(len)
            .ok_or_else(|| invalid("payload colunar absurdo".to_string()))?;
        lens.push(len);
    }
    let end = p
        .checked_add(payload)
        .filter(|end| *end <= data.len())
        .ok_or_else(|| invalid("payload colunar truncado".to_string()))?;

    let out_len = payload
        .checked_add(total - 1)
        .and_then(|n| n.checked_add(trailing as usize))
        .ok_or_else(|| invalid("saída colunar absurda".to_string()))?;
    check_len(out_len, limit)?;

    let mut fields: Vec<&[u8]> = vec![&[][..]; total];
    let mut q = p;
    for c in 0..cols {
        for r in 0..rows {
            let len = lens[c * rows + r];
            fields[r * cols + c] = data
                .get(q..q + len)
                .ok_or_else(|| invalid("payload colunar truncado".to_string()))?;
            q += len;
        }
    }
    debug_assert_eq!(q, end);

    let mut out = Vec::with_capacity(out_len);
    for r in 0..rows {
        if r > 0 {
            out.push(b'\n');
        }
        for c in 0..cols {
            if c > 0 {
                out.push(delim);
            }
            out.extend_from_slice(fields[r * cols + c]);
        }
    }
    if trailing {
        out.push(b'\n');
    }

    Ok(out)
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
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

fn get_varint(data: &[u8], p: &mut usize) -> io::Result<u64> {
    let mut v = 0u64;
    let mut shift = 0u32;
    loop {
        if *p >= data.len() {
            return Err(invalid("varint truncado".to_string()));
        }
        if shift > 63 {
            return Err(invalid("varint fora de faixa".to_string()));
        }
        let byte = data[*p];
        *p += 1;
        v |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok(v);
        }
        shift += 7;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(data: &[u8], t: Transform, width: u32) {
        let mut s = step(t, width, 0);
        s.input_len = data.len() as u64;
        let enc = apply(data, &s).expect("apply falhou");
        let dec = invert(&enc, &s).expect("invert falhou");
        assert_eq!(dec, data, "{} width={} não é reversível", transform_name(t), width);
    }

    #[test]
    fn delta_reversivel_em_todas_as_larguras() {
        let mut data: Vec<u8> = Vec::new();
        for i in 0..5000u64 {
            data.extend_from_slice(&(1_700_000_000_000u64 + i * 37).to_le_bytes());
        }
        data.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
        for w in WIDTHS {
            rt(&data, Transform::Delta, w);
        }
    }

    #[test]
    fn byte_split_reversivel_em_todas_as_larguras() {
        let mut data = Vec::new();
        for i in 0..3000u32 {
            data.extend_from_slice(&(i as f32 * 0.125).to_le_bytes());
        }
        data.extend_from_slice(&[7, 7, 7, 7, 7]);
        for w in WIDTHS {
            rt(&data, Transform::ByteSplit, w);
        }
    }

    #[test]
    fn rle_reversivel_inclusive_em_ruido() {
        let repetitivo: Vec<u8> = std::iter::repeat(0u8).take(10_000).collect();
        rt(&repetitivo, Transform::Rle, 1);

        let mut x = 0x12345678u32;
        let ruido: Vec<u8> = (0..10_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x & 0xff) as u8
            })
            .collect();
        rt(&ruido, Transform::Rle, 1);

        let mut s = step(Transform::Rle, 1, ruido.len() as u64);
        s.input_len = ruido.len() as u64;
        let enc = apply(&ruido, &s).unwrap();
        assert!(
            enc.len() < ruido.len() + ruido.len() / 100 + 16,
            "RLE expandiu demais: {} -> {}",
            ruido.len(),
            enc.len()
        );

        rt(&[], Transform::Rle, 1);
        rt(&[1], Transform::Rle, 1);
        rt(&[1, 1], Transform::Rle, 1);
        rt(&[1, 2], Transform::Rle, 1);
    }

    fn csv_sample(trailing: bool) -> Vec<u8> {
        let mut s = String::from("id,data,regiao,valor\n");
        for i in 0..500 {
            s.push_str(&format!(
                "{},2026-03-{:02},sudeste,{}.{:02}\n",
                i,
                (i % 28) + 1,
                i * 7,
                i % 100
            ));
        }
        if !trailing {
            s.pop();
        }
        s.into_bytes()
    }

    #[test]
    fn csv_colunar_reversivel() {
        for trailing in [true, false] {
            let data = csv_sample(trailing);
            assert!(csv_detect(&data).is_some(), "detector falhou");
            rt(&data, Transform::CsvColumnar, 1);
        }
    }

    #[test]
    fn csv_detector_rejeita_nao_tabular() {
        assert!(csv_detect(b"texto qualquer sem estrutura nenhuma aqui dentro").is_none());
        let irregular = b"a,b,c\n1,2\n3,4,5\n6,7,8\n9,10,11\n".to_vec();
        assert!(csv_detect(&irregular).is_none());
        let json = br#"{"a":1,"b":[1,2,3],"c":"x"}"#.to_vec();
        assert!(csv_detect(&json).is_none());
    }

    #[test]
    fn pilha_completa_reversivel() {
        let data = csv_sample(true);
        let mut steps = vec![
            step(Transform::CsvColumnar, 1, 0),
            step(Transform::Delta, 1, 0),
        ];
        let enc = apply_chain(&data, &mut steps).unwrap();
        let dec = invert_chain(&enc, &steps).unwrap();
        assert_eq!(dec, data);
        assert_eq!(chain_label(&steps), "csv_columnar>delta:1");
    }

    #[test]
    fn invert_detecta_tamanho_divergente() {
        let data = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut s = step(Transform::Delta, 2, 0);
        s.input_len = data.len() as u64;
        let enc = apply(&data, &s).unwrap();
        let mut bad = s;
        bad.input_len = 999;
        assert!(invert(&enc, &bad).is_err());
    }

    #[test]
    fn rle_rejeita_stream_corrompido() {
        assert!(rle_invert(&[128]).is_err());
        assert!(rle_invert(&[5, 1, 2]).is_err());
        assert!(rle_invert(&[200]).is_err());
    }

    fn csv_forjado(rows: u64, cols: u64, lens: &[u64], cauda: &[u8]) -> Vec<u8> {
        let mut out = vec![CSV_MAGIC, b',', 1];
        put_varint(&mut out, rows);
        put_varint(&mut out, cols);
        for l in lens {
            put_varint(&mut out, *l);
        }
        out.extend_from_slice(cauda);
        out
    }

    #[test]
    fn csv_com_varint_de_comprimento_gigante_devolve_erro_em_vez_de_panico() {
        let passo_csv = step(Transform::CsvColumnar, 1, 0);
        for len in [
            u64::MAX,
            u64::MAX - 1,
            usize::MAX as u64,
            (usize::MAX - 1) as u64,
            (usize::MAX / 2 + 1) as u64,
            1 << 63,
            (1 << 63) - 1,
            1 << 62,
            1 << 40,
        ] {
            let dados = csv_forjado(1, 2, &[len, 0], b"abc");
            let r = std::panic::catch_unwind(|| invert(&dados, &passo_csv));
            let r = r.unwrap_or_else(|_| panic!("pânico com len {len}"));
            assert!(r.is_err(), "len {len} deveria ser recusado");
        }
    }

    #[test]
    fn csv_com_soma_de_comprimentos_que_estoura_devolve_erro() {
        let passo_csv = step(Transform::CsvColumnar, 1, 0);
        let dados = csv_forjado(1, 3, &[usize::MAX as u64, usize::MAX as u64, 5], b"abcdef");
        let r = std::panic::catch_unwind(|| invert(&dados, &passo_csv));
        assert!(r.expect("pânico na soma").is_err());
    }

    #[test]
    fn csv_com_dimensoes_hostis_devolve_erro_sem_travar_nem_alocar() {
        let passo_csv = step(Transform::CsvColumnar, 1, 0);
        for (rows, cols) in [
            (0, u64::MAX),
            (u64::MAX, 0),
            (1 << 40, 1 << 40),
            (u64::MAX, u64::MAX),
            (1, 1 << 40),
            (1 << 40, 1),
        ] {
            let dados = csv_forjado(rows, cols, &[], b"");
            assert!(invert(&dados, &passo_csv).is_err(), "{rows}x{cols}");
        }
    }

    #[test]
    fn invert_limited_barra_a_saida_acima_do_teto_em_todos_os_transforms() {
        let rle: Vec<u8> = (0..1000).flat_map(|_| [129u8, 7]).collect();
        let s_rle = step(Transform::Rle, 1, 0);
        assert_eq!(invert(&rle, &s_rle).unwrap().len(), 128_000);
        let e = invert_limited(&rle, &s_rle, 127_999).unwrap_err();
        assert_eq!(crate::codec::too_large_limit(&e), Some(127_999));
        assert_eq!(invert_limited(&rle, &s_rle, 128_000).unwrap().len(), 128_000);

        let dados = vec![9u8; 4096];
        for t in [Transform::Delta, Transform::ByteSplit] {
            let s = step(t, 4, 0);
            assert!(invert_limited(&dados, &s, 4096).is_ok());
            let e = invert_limited(&dados, &s, 4095).unwrap_err();
            assert_eq!(crate::codec::too_large_limit(&e), Some(4095));
        }

        let csv = csv_sample(true);
        let s_csv = step(Transform::CsvColumnar, 1, 0);
        let colunar = apply(&csv, &s_csv).unwrap();
        assert_eq!(invert_limited(&colunar, &s_csv, csv.len()).unwrap(), csv);
        let e = invert_limited(&colunar, &s_csv, csv.len() - 1).unwrap_err();
        assert_eq!(crate::codec::too_large_limit(&e), Some(csv.len() as u64 - 1));
    }

    #[test]
    fn input_len_declarado_menor_que_a_saida_e_erro_comum_e_nao_de_teto() {
        let rle: Vec<u8> = vec![129u8, 7];
        let s = step(Transform::Rle, 1, 100);
        let e = invert_limited(&rle, &s, 1 << 20).unwrap_err();
        assert_eq!(crate::codec::too_large_limit(&e), None, "{e}");
    }

    #[test]
    fn cadeia_acima_do_maximo_e_recusada_e_a_do_maximo_passa() {
        let dados = vec![1u8, 2, 3, 4];
        let longa: Vec<TransformStep> = (0..MAX_CHAIN + 1).map(|_| step(Transform::Delta, 1, 0)).collect();
        assert!(invert_chain(&dados, &longa).is_err());
        let justa: Vec<TransformStep> = (0..MAX_CHAIN).map(|_| step(Transform::Delta, 1, 0)).collect();
        assert!(invert_chain(&dados, &justa).is_ok());
    }

    #[test]
    fn cadeia_limitada_aplica_o_teto_de_estagio_e_o_final() {
        let mut steps = vec![step(Transform::Rle, 1, 0), step(Transform::Delta, 1, 0)];
        let original = vec![5u8; 600];
        let enc = apply_chain(&original, &mut steps).unwrap();
        assert_eq!(invert_chain_limited(&enc, &steps, 700, 600).unwrap(), original);
        assert!(invert_chain_limited(&enc, &steps, 700, 599).is_err());
        assert!(invert_chain_limited(&enc, &steps, 9, 600).is_err());
    }
}
