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

pub fn invert(data: &[u8], s: &TransformStep) -> io::Result<Vec<u8>> {
    let t = Transform::try_from(s.transform)
        .map_err(|_| invalid(format!("transform desconhecido no envelope: {}", s.transform)))?;
    let out = match t {
        Transform::Unspecified => data.to_vec(),
        Transform::Delta => delta_invert(data, width_of(s)?),
        Transform::ByteSplit => byte_split_invert(data, width_of(s)?),
        Transform::Rle => rle_invert(data)?,
        Transform::CsvColumnar => csv_invert(data)?,
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
    let mut current = data.to_vec();
    for s in steps.iter().rev() {
        current = invert(&current, s)?;
    }
    Ok(current)
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

fn rle_invert(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len() * 2);
    let mut i = 0usize;

    while i < data.len() {
        let ctrl = data[i];
        i += 1;

        if ctrl < 128 {
            let n = ctrl as usize + 1;
            if i + n > data.len() {
                return Err(invalid("RLE truncado em bloco literal".to_string()));
            }
            out.extend_from_slice(&data[i..i + n]);
            i += n;
        } else if ctrl > 128 {
            let n = 257 - ctrl as usize;
            if i >= data.len() {
                return Err(invalid("RLE truncado em bloco de repetição".to_string()));
            }
            let b = data[i];
            i += 1;
            out.extend(std::iter::repeat(b).take(n));
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

fn csv_invert(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut p = 0usize;
    if data.len() < 5 || data[0] != CSV_MAGIC {
        return Err(invalid("cabeçalho colunar inválido".to_string()));
    }
    p += 1;
    let delim = data[p];
    p += 1;
    let trailing = data[p] != 0;
    p += 1;

    let rows = get_varint(data, &mut p)? as usize;
    let cols = get_varint(data, &mut p)? as usize;
    let total = rows
        .checked_mul(cols)
        .ok_or_else(|| invalid("dimensões colunares absurdas".to_string()))?;
    if total > data.len().saturating_mul(4) + 1024 {
        return Err(invalid("dimensões colunares inconsistentes".to_string()));
    }

    let mut lens = Vec::with_capacity(total);
    for _ in 0..total {
        lens.push(get_varint(data, &mut p)? as usize);
    }

    let mut fields: Vec<&[u8]> = vec![&[][..]; total];
    for c in 0..cols {
        for r in 0..rows {
            let len = lens[c * rows + r];
            if p + len > data.len() {
                return Err(invalid("payload colunar truncado".to_string()));
            }
            fields[r * cols + c] = &data[p..p + len];
            p += len;
        }
    }

    let mut out = Vec::with_capacity(p);
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
}
