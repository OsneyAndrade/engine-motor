use std::io;

use rayon::prelude::*;

use crate::codec::{self, Codec};
use crate::content::{ContentClass, ContentProfile};
use crate::transform::{self, Transform, TransformStep};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    Fast,
    Balanced,
    Max,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Fast => "fast",
            Effort::Balanced => "balanced",
            Effort::Max => "max",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fast" | "speed" | "low" => Some(Effort::Fast),
            "balanced" | "default" | "medium" => Some(Effort::Balanced),
            "max" | "density" | "high" | "best" => Some(Effort::Max),
            _ => None,
        }
    }

    fn candidate_budget(self) -> usize {
        match self {
            Effort::Fast => 1,
            Effort::Balanced => 4,
            Effort::Max => 16,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub transforms: Vec<TransformStep>,
    pub codec: Codec,
    pub level: i32,
    pub use_dictionary: bool,
}

impl Plan {
    fn new(codec: Codec, level: i32) -> Self {
        Plan {
            transforms: Vec::new(),
            codec,
            level: codec::clamp_level(codec, level),
            use_dictionary: false,
        }
    }

    fn with(mut self, steps: &[(Transform, u32)]) -> Self {
        self.transforms = steps
            .iter()
            .map(|(t, w)| transform::step(*t, *w, 0))
            .collect();
        self
    }

    fn dict(mut self) -> Self {
        self.use_dictionary = codec::supports_dictionary(self.codec);
        self
    }

    pub fn label(&self) -> String {
        let mut s = String::new();
        if !self.transforms.is_empty() {
            s.push_str(&transform::chain_label(&self.transforms));
            s.push('+');
        }
        s.push_str(codec::name(self.codec));
        if codec::info(self.codec).map(|c| c.max_level > 0).unwrap_or(false) {
            s.push_str(&format!(":{}", self.level));
        }
        if self.use_dictionary {
            s.push_str("+dict");
        }
        s
    }

    pub fn is_stored(&self) -> bool {
        self.codec == Codec::Stored && self.transforms.is_empty()
    }

    fn stored() -> Self {
        Plan::new(Codec::Stored, 0)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ForcedPlan {
    pub codec: Option<Codec>,
    pub level: Option<i32>,
    pub use_dictionary: Option<bool>,
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub probe_threshold: usize,
    pub probe_sample: usize,
    pub keep_output_max: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            probe_threshold: 48 * 1024 * 1024,
            probe_sample: 8 * 1024 * 1024,
            keep_output_max: 16 * 1024 * 1024,
        }
    }
}

pub struct Planned {
    pub plan: Plan,
    pub payload: Vec<u8>,
    pub candidates_tried: u32,
    pub sampled_decision: bool,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct Measurement {
    pub label: String,
    pub codec: Codec,
    pub level: i32,
    pub transforms: String,
    pub used_dictionary: bool,
    pub output_size: usize,
    pub savings_pct: f64,
    pub encode_ms: f64,
    pub error: Option<String>,
}

pub fn candidates(profile: &ContentProfile, effort: Effort, dict_available: bool) -> Vec<Plan> {
    let mut plans: Vec<Plan> = Vec::new();
    let stride = profile.stride.unwrap_or(4);
    let very_low_entropy = profile.entropy < 2.0;

    match profile.class {
        ContentClass::Incompressible => {
            plans.push(Plan::stored());
        }

        ContentClass::Media | ContentClass::PreCompressed => {
            plans.push(Plan::stored());
            if effort == Effort::Max {
                plans.push(Plan::new(Codec::Zstd, 19));
                plans.push(Plan::new(Codec::Xz, 6));
            }
        }

        ContentClass::Tabular => {
            let cols: &[(Transform, u32)] = &[(Transform::CsvColumnar, 1)];
            let cols_delta: &[(Transform, u32)] =
                &[(Transform::CsvColumnar, 1), (Transform::Delta, 1)];
            match effort {
                Effort::Fast => {
                    plans.push(Plan::new(Codec::Zstd, 3));
                }
                Effort::Balanced => {
                    plans.push(Plan::new(Codec::Zstd, 12));
                    plans.push(Plan::new(Codec::Zstd, 12).with(cols));
                    plans.push(Plan::new(Codec::Brotli, 9).with(cols));
                    if dict_available {
                        plans.push(Plan::new(Codec::Zstd, 12).dict());
                    }
                }
                Effort::Max => {
                    plans.push(Plan::new(Codec::Zstd, 19));
                    plans.push(Plan::new(Codec::Zstd, 19).with(cols));
                    plans.push(Plan::new(Codec::Zstd, 22).with(cols));
                    plans.push(Plan::new(Codec::Brotli, 11).with(cols));
                    plans.push(Plan::new(Codec::Xz, 6).with(cols));
                    plans.push(Plan::new(Codec::Zstd, 19).with(cols_delta));
                    plans.push(Plan::new(Codec::Bzip2, 9).with(cols));
                    if dict_available {
                        plans.push(Plan::new(Codec::Zstd, 19).dict());
                        plans.push(Plan::new(Codec::Zstd, 19).with(cols).dict());
                    }
                }
            }
        }

        ContentClass::StructuredText | ContentClass::PlainText => match effort {
            Effort::Fast => {
                plans.push(Plan::new(Codec::Zstd, 3));
            }
            Effort::Balanced => {
                plans.push(Plan::new(Codec::Zstd, 12));
                plans.push(Plan::new(Codec::Brotli, 9));
                if dict_available {
                    plans.push(Plan::new(Codec::Zstd, 12).dict());
                }
            }
            Effort::Max => {
                plans.push(Plan::new(Codec::Zstd, 19));
                plans.push(Plan::new(Codec::Zstd, 22));
                plans.push(Plan::new(Codec::Brotli, 11));
                plans.push(Plan::new(Codec::Xz, 6));
                plans.push(Plan::new(Codec::Bzip2, 9));
                if dict_available {
                    plans.push(Plan::new(Codec::Zstd, 19).dict());
                }
                if very_low_entropy {
                    plans.push(Plan::new(Codec::Zstd, 19).with(&[(Transform::Rle, 1)]));
                }
            }
        },

        ContentClass::NumericBinary => {
            let split: &[(Transform, u32)] = &[(Transform::ByteSplit, stride)];
            let delta: &[(Transform, u32)] = &[(Transform::Delta, stride)];
            let delta_split: &[(Transform, u32)] =
                &[(Transform::Delta, stride), (Transform::ByteSplit, stride)];
            match effort {
                Effort::Fast => {
                    plans.push(Plan::new(Codec::Zstd, 3).with(split));
                }
                Effort::Balanced => {
                    plans.push(Plan::new(Codec::Zstd, 12));
                    plans.push(Plan::new(Codec::Zstd, 12).with(split));
                    plans.push(Plan::new(Codec::Zstd, 12).with(delta));
                    plans.push(Plan::new(Codec::Zstd, 12).with(delta_split));
                }
                Effort::Max => {
                    plans.push(Plan::new(Codec::Zstd, 19));
                    plans.push(Plan::new(Codec::Zstd, 19).with(split));
                    plans.push(Plan::new(Codec::Zstd, 19).with(delta));
                    plans.push(Plan::new(Codec::Zstd, 19).with(delta_split));
                    plans.push(Plan::new(Codec::Xz, 6).with(split));
                    plans.push(Plan::new(Codec::Xz, 6).with(delta_split));
                    plans.push(Plan::new(Codec::Brotli, 11).with(split));
                    for w in [2u32, 4, 8] {
                        if w != stride {
                            plans.push(
                                Plan::new(Codec::Zstd, 19).with(&[(Transform::ByteSplit, w)]),
                            );
                        }
                    }
                }
            }
        }

        ContentClass::Binary => match effort {
            Effort::Fast => {
                plans.push(Plan::new(Codec::Lz4, 0));
            }
            Effort::Balanced => {
                plans.push(Plan::new(Codec::Zstd, 12));
                plans.push(Plan::new(Codec::Zstd, 12).with(&[(Transform::ByteSplit, 4)]));
                if very_low_entropy {
                    plans.push(Plan::new(Codec::Zstd, 12).with(&[(Transform::Rle, 1)]));
                }
            }
            Effort::Max => {
                plans.push(Plan::new(Codec::Zstd, 19));
                plans.push(Plan::new(Codec::Xz, 6));
                plans.push(Plan::new(Codec::Brotli, 11));
                plans.push(Plan::new(Codec::Bzip2, 9));
                plans.push(Plan::new(Codec::Zstd, 19).with(&[(Transform::ByteSplit, 4)]));
                plans.push(Plan::new(Codec::Zstd, 19).with(&[(Transform::Delta, 4)]));
                if very_low_entropy {
                    plans.push(Plan::new(Codec::Zstd, 19).with(&[(Transform::Rle, 1)]));
                    plans.push(Plan::new(Codec::Xz, 6).with(&[(Transform::Rle, 1)]));
                }
                if dict_available {
                    plans.push(Plan::new(Codec::Zstd, 19).dict());
                }
            }
        },
    }

    plans.truncate(effort.candidate_budget());

    if !plans.iter().any(|p| p.is_stored()) {
        plans.push(Plan::stored());
    }

    plans
}

pub fn execute(
    data: &[u8],
    plan: &Plan,
    dict: Option<&[u8]>,
) -> io::Result<(Vec<u8>, Vec<TransformStep>)> {
    let dict = if plan.use_dictionary { dict } else { None };

    if plan.transforms.is_empty() {
        let out = codec::encode(data, plan.codec, plan.level, dict)?;
        return Ok((out, Vec::new()));
    }

    let mut steps = plan.transforms.clone();
    let staged = transform::apply_chain(data, &mut steps)?;
    let out = codec::encode(&staged, plan.codec, plan.level, dict)?;
    Ok((out, steps))
}

pub fn restore(
    payload: &[u8],
    codec_id: Codec,
    steps: &[TransformStep],
    dict: Option<&[u8]>,
    expected_len: Option<u64>,
) -> io::Result<Vec<u8>> {
    let staged_len = steps.last().map(|s| s.input_len).or(expected_len);
    let staged = codec::decode(payload, codec_id, dict, staged_len)?;
    transform::invert_chain(&staged, steps)
}

pub fn compress_best(
    data: &[u8],
    profile: &ContentProfile,
    effort: Effort,
    dict: Option<&[u8]>,
    forced: &ForcedPlan,
    limits: &Limits,
) -> io::Result<Planned> {
    if let Some(codec_id) = forced.codec {
        let level = forced
            .level
            .unwrap_or_else(|| codec::default_level(codec_id));
        let mut plan = Plan::new(codec_id, level);
        if forced.use_dictionary.unwrap_or(false) {
            plan = plan.dict();
        }
        let (payload, steps) = execute(data, &plan, dict)?;
        plan.transforms = steps;

        if payload.len() >= data.len() && !plan.is_stored() {
            let stored = Plan::stored();
            return Ok(Planned {
                plan: stored,
                payload: data.to_vec(),
                candidates_tried: 1,
                sampled_decision: false,
                reason: format!(
                    "codec {} imposto pelo cliente não reduziu ({} -> {} bytes); gravado sem compressão",
                    codec::name(codec_id),
                    data.len(),
                    payload.len()
                ),
            });
        }

        let label = plan.label();
        return Ok(Planned {
            plan,
            payload,
            candidates_tried: 1,
            sampled_decision: false,
            reason: format!("plano imposto pelo cliente: {}", label),
        });
    }

    if data.is_empty() {
        return Ok(Planned {
            plan: Plan::stored(),
            payload: Vec::new(),
            candidates_tried: 0,
            sampled_decision: false,
            reason: "entrada vazia".to_string(),
        });
    }

    let dict_available = dict.map(|d| !d.is_empty()).unwrap_or(false)
        && forced.use_dictionary != Some(false);
    let plans = candidates(profile, effort, dict_available);

    let sampled = data.len() > limits.probe_threshold && plans.len() > 1;
    let probe: &[u8] = if sampled {
        probe_slice(data, limits.probe_sample)
    } else {
        data
    };
    let keep_outputs = !sampled && probe.len() <= limits.keep_output_max;

    let measured: Vec<(usize, Option<Vec<u8>>, Vec<TransformStep>)> = plans
        .par_iter()
        .map(|plan| match execute(probe, plan, dict) {
            Ok((out, steps)) => {
                let len = out.len();
                (len, if keep_outputs { Some(out) } else { None }, steps)
            }
            Err(_) => (usize::MAX, None, Vec::new()),
        })
        .collect();

    let (best_idx, best_len) = measured
        .iter()
        .enumerate()
        .map(|(i, (len, _, _))| (i, *len))
        .min_by_key(|(i, len)| {
            (*len, *i)
        })
        .unwrap_or((0, usize::MAX));

    let tried = plans.len() as u32;

    if best_len == usize::MAX {
        return Ok(Planned {
            plan: Plan::stored(),
            payload: data.to_vec(),
            candidates_tried: tried,
            sampled_decision: sampled,
            reason: "nenhum candidato aplicável; gravado sem compressão".to_string(),
        });
    }

    let mut best_plan = plans[best_idx].clone();
    let probe_len = probe.len();

    let (payload, steps) = match measured.into_iter().nth(best_idx) {
        Some((_, Some(out), steps)) => (out, steps),
        _ => execute(data, &best_plan, dict)?,
    };
    best_plan.transforms = steps;

    if payload.len() >= data.len() {
        return Ok(Planned {
            plan: Plan::stored(),
            payload: data.to_vec(),
            candidates_tried: tried,
            sampled_decision: sampled,
            reason: format!(
                "melhor de {} candidatos não reduziu ({} bytes); gravado sem compressão",
                tried,
                data.len()
            ),
        });
    }

    let reason = if sampled {
        format!(
            "{} candidatos medidos em amostra de {} bytes ({:.1}% do arquivo); vencedor {}",
            tried,
            probe_len,
            probe_len as f64 * 100.0 / data.len() as f64,
            best_plan.label()
        )
    } else {
        format!(
            "{} candidatos medidos no conteúdo integral; vencedor {} sobre classe {}",
            tried,
            best_plan.label(),
            profile.class.as_str()
        )
    };

    Ok(Planned {
        plan: best_plan,
        payload,
        candidates_tried: tried,
        sampled_decision: sampled,
        reason,
    })
}

fn probe_slice(data: &[u8], budget: usize) -> &[u8] {
    if data.len() <= budget {
        return data;
    }
    let start = ((data.len() - budget) / 2) & !7usize;
    &data[start..start + budget]
}

pub fn measure_all(
    data: &[u8],
    profile: &ContentProfile,
    effort: Effort,
    dict: Option<&[u8]>,
    limits: &Limits,
) -> Vec<Measurement> {
    let dict_available = dict.map(|d| !d.is_empty()).unwrap_or(false);
    let plans = candidates(profile, effort, dict_available);
    let probe = probe_slice(data, limits.probe_sample.min(limits.probe_threshold));

    let mut out: Vec<Measurement> = plans
        .par_iter()
        .map(|plan| {
            let t0 = std::time::Instant::now();
            let result = execute(probe, plan, dict);
            let encode_ms = t0.elapsed().as_secs_f64() * 1000.0;
            let transforms = transform::chain_label(&plan.transforms);
            match result {
                Ok((bytes, _)) => Measurement {
                    label: plan.label(),
                    codec: plan.codec,
                    level: plan.level,
                    transforms,
                    used_dictionary: plan.use_dictionary,
                    output_size: bytes.len(),
                    savings_pct: if probe.is_empty() {
                        0.0
                    } else {
                        (1.0 - bytes.len() as f64 / probe.len() as f64) * 100.0
                    },
                    encode_ms,
                    error: None,
                },
                Err(e) => Measurement {
                    label: plan.label(),
                    codec: plan.codec,
                    level: plan.level,
                    transforms,
                    used_dictionary: plan.use_dictionary,
                    output_size: 0,
                    savings_pct: 0.0,
                    encode_ms,
                    error: Some(e.to_string()),
                },
            }
        })
        .collect();

    out.sort_by(|a, b| match (a.error.is_some(), b.error.is_some()) {
        (false, true) => std::cmp::Ordering::Less,
        (true, false) => std::cmp::Ordering::Greater,
        _ => a.output_size.cmp(&b.output_size),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content;

    fn csv(rows: usize) -> Vec<u8> {
        let mut s = String::from("id,data,regiao,produto,valor\n");
        for i in 0..rows {
            s.push_str(&format!(
                "{},2026-0{}-{:02},sudeste,produto-{},{}.{:02}\n",
                i,
                (i % 9) + 1,
                (i % 28) + 1,
                i % 50,
                i * 13,
                i % 100
            ));
        }
        s.into_bytes()
    }

    fn floats(n: usize) -> Vec<u8> {
        let mut v = Vec::new();
        for i in 0..n {
            v.extend_from_slice(&(20.0f32 + (i as f32) * 0.001).to_le_bytes());
        }
        v
    }

    fn random(n: usize) -> Vec<u8> {
        let mut x = 0x243F6A88u32;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x >> 7) as u8
            })
            .collect()
    }

    fn roundtrip(data: &[u8], effort: Effort) -> Planned {
        let profile = content::classify(data, "amostra.dat", None);
        let planned = compress_best(
            data,
            &profile,
            effort,
            None,
            &ForcedPlan::default(),
            &Limits::default(),
        )
        .expect("compressão falhou");

        let restored = restore(
            &planned.payload,
            planned.plan.codec,
            &planned.plan.transforms,
            None,
            Some(data.len() as u64),
        )
        .expect("reconstrução falhou");

        assert_eq!(
            restored, data,
            "reconstrução divergente no plano {}",
            planned.plan.label()
        );
        planned
    }

    #[test]
    fn roundtrip_exato_em_todas_as_classes_e_esforcos() {
        let corpus: Vec<(&str, Vec<u8>)> = vec![
            ("csv", csv(2000)),
            ("floats", floats(20_000)),
            ("random", random(200_000)),
            ("json", {
                let mut s = String::from("[");
                for i in 0..2000 {
                    s.push_str(&format!(r#"{{"id":{i},"ok":true,"nome":"reg-{i}"}},"#));
                }
                s.push(']');
                s.into_bytes()
            }),
            ("zeros", vec![0u8; 100_000]),
            ("vazio", Vec::new()),
            ("um_byte", vec![42u8]),
        ];

        for (nome, data) in corpus {
            for effort in [Effort::Fast, Effort::Balanced, Effort::Max] {
                let p = roundtrip(&data, effort);
                assert!(
                    p.payload.len() <= data.len().max(1),
                    "{nome}/{}: essência ({}) maior que original ({})",
                    effort.as_str(),
                    p.payload.len(),
                    data.len()
                );
            }
        }
    }

    #[test]
    fn max_comprime_melhor_que_fast() {
        let data = csv(4000);
        let fast = roundtrip(&data, Effort::Fast);
        let max = roundtrip(&data, Effort::Max);
        assert!(
            max.payload.len() < fast.payload.len(),
            "max ({}) deveria bater fast ({})",
            max.payload.len(),
            fast.payload.len()
        );
    }

    #[test]
    fn layout_colunar_vence_em_csv_no_modo_max() {
        let data = csv(4000);
        let profile = content::classify(&data, "t.csv", None);
        assert_eq!(profile.class, ContentClass::Tabular);

        let plano_direto = Plan::new(Codec::Zstd, 19);
        let plano_colunar =
            Plan::new(Codec::Zstd, 19).with(&[(Transform::CsvColumnar, 1)]);
        let direto = execute(&data, &plano_direto, None).unwrap().0.len();
        let colunar = execute(&data, &plano_colunar, None).unwrap().0.len();
        assert!(
            colunar < direto,
            "colunar ({colunar}) deveria bater linha-a-linha ({direto})"
        );
    }

    #[test]
    fn byte_split_vence_em_array_de_float() {
        let data = floats(50_000);
        let direto = execute(&data, &Plan::new(Codec::Zstd, 19), None).unwrap().0.len();
        let split = execute(
            &data,
            &Plan::new(Codec::Zstd, 19).with(&[(Transform::ByteSplit, 4)]),
            None,
        )
        .unwrap()
        .0
        .len();
        assert!(split < direto, "byte split ({split}) deveria bater direto ({direto})");
    }

    #[test]
    fn dado_aleatorio_cai_no_piso_stored() {
        let data = random(300_000);
        let planned = roundtrip(&data, Effort::Max);
        assert!(planned.plan.is_stored(), "plano foi {}", planned.plan.label());
        assert_eq!(planned.payload.len(), data.len());
    }

    #[test]
    fn plano_imposto_e_respeitado() {
        let data = csv(1000);
        let profile = content::classify(&data, "t.csv", None);
        let forced = ForcedPlan {
            codec: Some(Codec::Bzip2),
            level: Some(9),
            use_dictionary: None,
        };
        let p = compress_best(
            &data,
            &profile,
            Effort::Max,
            None,
            &forced,
            &Limits::default(),
        )
        .unwrap();
        assert_eq!(p.plan.codec, Codec::Bzip2);
        assert_eq!(p.candidates_tried, 1);
        let back = restore(&p.payload, p.plan.codec, &p.plan.transforms, None, None).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn plano_imposto_inutil_cai_para_stored() {
        let data = random(100_000);
        let profile = content::classify(&data, "r.bin", None);
        let forced = ForcedPlan {
            codec: Some(Codec::Zstd),
            level: Some(1),
            use_dictionary: None,
        };
        let p = compress_best(
            &data,
            &profile,
            Effort::Fast,
            None,
            &forced,
            &Limits::default(),
        )
        .unwrap();
        assert!(p.plan.is_stored());
        assert_eq!(p.payload.len(), data.len());
    }

    #[test]
    fn decisao_por_amostra_ainda_reconstroi() {
        let data = csv(60_000);
        let profile = content::classify(&data, "grande.csv", None);
        let limits = Limits {
            probe_threshold: 64 * 1024,
            probe_sample: 32 * 1024,
            keep_output_max: 1024,
        };
        let p = compress_best(
            &data,
            &profile,
            Effort::Max,
            None,
            &ForcedPlan::default(),
            &limits,
        )
        .unwrap();
        assert!(p.sampled_decision, "deveria ter decidido por amostra");
        let back = restore(
            &p.payload,
            p.plan.codec,
            &p.plan.transforms,
            None,
            Some(data.len() as u64),
        )
        .unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn analise_ordena_por_densidade() {
        let data = csv(2000);
        let profile = content::classify(&data, "t.csv", None);
        let m = measure_all(&data, &profile, Effort::Max, None, &Limits::default());
        assert!(m.len() >= 2);
        for w in m.windows(2) {
            if w[0].error.is_none() && w[1].error.is_none() {
                assert!(w[0].output_size <= w[1].output_size);
            }
        }
    }

    #[test]
    fn esforco_respeita_orcamento_de_candidatos() {
        let data = csv(500);
        let profile = content::classify(&data, "t.csv", None);
        assert!(candidates(&profile, Effort::Fast, false).len() <= 2);
        assert!(candidates(&profile, Effort::Balanced, false).len() <= 5);
        assert!(candidates(&profile, Effort::Max, false).len() <= 17);
    }
}
