use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::codec::{self, Codec};
use crate::config::Config;
use crate::container::{self};
use crate::planner::{Effort, ForcedPlan};
use crate::proto::ProcessedFile;
use crate::service::CompressRequest;
use crate::tenancy::GrantRow;

use super::error::ApiError;

#[derive(Debug, Default, Deserialize)]
pub struct CompressParams {
    pub filename: Option<String>,
    pub mime: Option<String>,
    pub codec: Option<String>,
    pub level: Option<i32>,
    pub effort: Option<String>,
    pub verify: Option<bool>,
    pub persist: Option<bool>,
    pub dictionary: Option<bool>,
    pub dedup: Option<bool>,
    pub response: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseFormat {
    Binary,
    Json,
}

impl CompressParams {
    fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
        headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim).filter(|s| !s.is_empty())
    }

    fn header_bool(headers: &HeaderMap, name: &str) -> Option<bool> {
        Self::header(headers, name).and_then(|v| match v.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
    }

    pub fn resolve(&self, headers: &HeaderMap, config: &Config) -> Result<CompressRequest, ApiError> {
        let filename = self
            .filename
            .clone()
            .or_else(|| Self::header(headers, "x-filename").map(str::to_string))
            .unwrap_or_else(|| "sem-nome".to_string());

        let filename = sanitize_filename(&filename);

        let mime_hint = self
            .mime
            .clone()
            .or_else(|| Self::header(headers, "x-mime").map(str::to_string));

        let effort_raw = self
            .effort
            .clone()
            .or_else(|| Self::header(headers, "x-effort").map(str::to_string));
        let effort = match effort_raw {
            Some(v) => Effort::from_name(&v).ok_or_else(|| {
                ApiError::bad_request(
                    "invalid_effort",
                    format!("esforço '{v}' inválido; use fast, balanced ou max"),
                )
            })?,
            None => config.default_effort,
        };

        let codec_raw = self
            .codec
            .clone()
            .or_else(|| Self::header(headers, "x-codec").map(str::to_string));
        let codec: Option<Codec> = match codec_raw {
            Some(v) => Some(codec::from_name(&v).ok_or_else(|| {
                ApiError::bad_request(
                    "unsupported_codec",
                    format!("codec '{v}' não suportado"),
                )
            })?),
            None => None,
        };

        let level = self
            .level
            .or_else(|| Self::header(headers, "x-level").and_then(|v| v.parse().ok()));

        if let (Some(c), Some(l)) = (codec, level) {
            if let Some(info) = codec::info(c) {
                if l < info.min_level || l > info.max_level {
                    return Err(ApiError::bad_request(
                        "invalid_level",
                        format!(
                            "nível {l} fora da faixa do codec {} ({}..{})",
                            info.name, info.min_level, info.max_level
                        ),
                    ));
                }
            }
        }

        let use_dictionary = self
            .dictionary
            .or_else(|| Self::header_bool(headers, "x-dictionary"));

        let verify = self
            .verify
            .or_else(|| Self::header_bool(headers, "x-verify"))
            .unwrap_or(config.verify_on_write);

        Ok(CompressRequest {
            filename,
            mime_hint,
            effort,
            forced: ForcedPlan {
                codec,
                level,
                use_dictionary,
            },
            verify,
            persist: self.persist.unwrap_or(true),
            allow_dedup: self.dedup.unwrap_or(true),
            idempotency_key: None,
            request_id: None,
        })
    }

    pub fn format(&self, headers: &HeaderMap, default: ResponseFormat) -> ResponseFormat {
        let raw = self
            .response
            .clone()
            .or_else(|| Self::header(headers, "x-response-format").map(str::to_string));
        match raw.as_deref().map(str::to_ascii_lowercase).as_deref() {
            Some("json") => return ResponseFormat::Json,
            Some("binary") | Some("protobuf") => return ResponseFormat::Binary,
            _ => {}
        }
        if let Some(accept) = headers.get(axum::http::header::ACCEPT).and_then(|v| v.to_str().ok()) {
            if accept.contains("application/json") && !accept.contains("*/*") {
                return ResponseFormat::Json;
            }
        }
        default
    }
}

pub fn idempotency_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get(super::IDEMPOTENCY_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 200 && s.chars().all(|c| c.is_ascii_graphic()))
        .map(str::to_string)
}

pub fn request_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

pub fn sanitize_filename(name: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .trim();
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control())
        .take(255)
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "sem-nome".to_string()
    } else {
        cleaned
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct DeleteParams {
    pub purge: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ListParams {
    pub search: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct PlanView {
    pub label: String,
    pub codec: String,
    pub level: i32,
    pub transforms: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dictionary_id: Option<String>,
}

impl PlanView {
    pub fn from_envelope(env: &ProcessedFile) -> Self {
        let codec_name = Codec::try_from(env.codec)
            .map(|c| codec::name(c).to_string())
            .unwrap_or_else(|_| "desconhecido".to_string());
        PlanView {
            label: crate::service::codec_label(env),
            codec: codec_name,
            level: env.codec_level,
            transforms: crate::transform::chain_label(&env.transforms),
            dictionary_id: env.dictionary.as_ref().map(|d| d.id.clone()),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Timings {
    pub compress_ms: f64,
    pub verify_ms: f64,
    pub total_ms: f64,
}

#[derive(Debug, Serialize)]
pub struct ObjectLinks {
    pub metadata: String,
    pub content: String,
    pub essence: String,
}

impl ObjectLinks {
    pub fn for_id(id: &str) -> Self {
        ObjectLinks {
            metadata: format!("/api/v1/objects/{id}"),
            content: format!("/api/v1/objects/{id}/content"),
            essence: format!("/api/v1/objects/{id}/essence"),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CompressResponse {
    pub id: String,
    pub original_name: String,
    pub mime: String,
    pub content_class: String,
    pub original_size: u64,
    pub essence_size: u64,
    pub savings_pct: f64,
    pub ratio: f64,
    pub plan: PlanView,
    pub reason: String,
    pub candidates_tried: u32,
    pub verified: bool,
    pub deduplicated: bool,
    pub sampled_decision: bool,
    pub persisted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references: Option<i64>,
    /// Requisição idempotente atendida com o resultado anterior.
    pub replayed: bool,
    pub timings: Timings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<ObjectLinks>,
}

impl CompressResponse {
    pub fn from_result(r: &crate::service::CompressResult, persisted: bool) -> Self {
        let env = &r.envelope;
        let metrics = env.file_metrics.as_ref();
        CompressResponse {
            id: r.id.clone(),
            original_name: env.original_name.clone(),
            mime: env.original_mime.clone(),
            content_class: env.content_class.clone(),
            original_size: env.original_size,
            essence_size: env.processed_size,
            savings_pct: round2(r.savings_pct()),
            ratio: if env.original_size > 0 {
                round4(env.processed_size as f64 / env.original_size as f64)
            } else {
                1.0
            },
            plan: PlanView::from_envelope(env),
            reason: env.plan_reason.clone(),
            candidates_tried: metrics.map(|m| m.candidates_tried).unwrap_or(0),
            verified: env.verified,
            deduplicated: r.deduplicated,
            sampled_decision: r.sampled_decision,
            persisted,
            references: r.references,
            replayed: r.replayed,
            timings: Timings {
                compress_ms: round2(r.compress_ms),
                verify_ms: round2(r.verify_ms),
                total_ms: round2(r.total_ms),
            },
            links: persisted.then(|| ObjectLinks::for_id(&r.id)),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ObjectResponse {
    pub id: String,
    pub original_name: String,
    pub known_names: Vec<String>,
    pub mime: String,
    pub content_class: String,
    pub original_size: u64,
    pub essence_size: u64,
    pub savings_pct: f64,
    pub plan: PlanView,
    pub reason: String,
    pub verified: bool,
    pub references: i64,
    pub container_version: i32,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub links: ObjectLinks,
}

impl ObjectResponse {
    pub fn build(id: &str, env: &ProcessedFile, grant: Option<&GrantRow>) -> Self {
        ObjectResponse {
            id: id.to_string(),
            original_name: grant
                .map(|g| g.name.clone())
                .unwrap_or_else(|| env.original_name.clone()),
            known_names: env.original_names.clone(),
            mime: env.original_mime.clone(),
            content_class: env.content_class.clone(),
            original_size: env.original_size,
            essence_size: env.processed_size,
            savings_pct: round2(if env.original_size > 0 {
                (1.0 - env.processed_size as f64 / env.original_size as f64) * 100.0
            } else {
                0.0
            }),
            plan: PlanView::from_envelope(env),
            reason: env.plan_reason.clone(),
            verified: env.verified,
            references: grant.map(|g| g.refs).unwrap_or(1),
            container_version: env.version,
            created_at_ms: grant.map(|g| g.created_at_ms).unwrap_or(env.created_at),
            updated_at_ms: grant.map(|g| g.updated_at_ms).unwrap_or(env.updated_at),
            links: ObjectLinks::for_id(id),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct MeasurementView {
    pub plan: String,
    pub codec: String,
    pub level: i32,
    pub transforms: String,
    pub used_dictionary: bool,
    pub output_size: usize,
    pub savings_pct: f64,
    pub encode_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AnalyzeResponse {
    pub original_name: String,
    pub mime: String,
    pub content_class: String,
    pub original_size: u64,
    pub measured_bytes: usize,
    pub entropy_bits_per_byte: f64,
    pub printable_ratio: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_stride: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_columns: Option<usize>,
    pub effort: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dictionary_id: Option<String>,
    pub candidates: Vec<MeasurementView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended: Option<String>,
}

impl AnalyzeResponse {
    pub fn build(report: crate::service::AnalysisReport, original_name: String) -> Self {
        let p = &report.profile;
        let candidates: Vec<MeasurementView> = report
            .measurements
            .iter()
            .map(|m| MeasurementView {
                plan: m.label.clone(),
                codec: codec::name(m.codec).to_string(),
                level: m.level,
                transforms: m.transforms.clone(),
                used_dictionary: m.used_dictionary,
                output_size: m.output_size,
                savings_pct: round2(m.savings_pct),
                encode_ms: round2(m.encode_ms),
                error: m.error.clone(),
            })
            .collect();
        let recommended = candidates
            .iter()
            .find(|c| c.error.is_none())
            .map(|c| c.plan.clone());

        AnalyzeResponse {
            original_name,
            mime: p.mime.clone(),
            content_class: p.class.as_str().to_string(),
            original_size: p.size,
            measured_bytes: report.probe_size,
            entropy_bits_per_byte: round4(p.entropy),
            printable_ratio: round4(p.printable),
            detected_stride: p.stride,
            detected_columns: p.tabular.map(|(_, c)| c),
            effort: report.effort.as_str().to_string(),
            dictionary_id: report.dictionary_id,
            candidates,
            recommended,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DeleteResponse {
    pub id: String,
    pub removed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_references: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct DecompressMeta {
    pub original_name: String,
    pub mime: String,
    pub original_size: u64,
    pub essence_size: u64,
    pub plan: PlanView,
    pub fidelity_checked: bool,
    pub reconstruct_ms: f64,
    pub container_version: i32,
}

impl DecompressMeta {
    pub fn build(env: &ProcessedFile, restored: &container::Restored) -> Self {
        DecompressMeta {
            original_name: env.original_name.clone(),
            mime: env.original_mime.clone(),
            original_size: env.original_size,
            essence_size: env.processed_size,
            plan: PlanView::from_envelope(env),
            fidelity_checked: restored.fidelity_checked,
            reconstruct_ms: round2(restored.duration_ms),
            container_version: env.version,
        }
    }
}

pub fn round2(v: f64) -> f64 {
    if v.is_finite() {
        (v * 100.0).round() / 100.0
    } else {
        0.0
    }
}

pub fn round4(v: f64) -> f64 {
    if v.is_finite() {
        (v * 10_000.0).round() / 10_000.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_filename_remove_caminho_e_controles() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("C:\\temp\\a.txt"), "a.txt");
        assert_eq!(sanitize_filename("  relatório.csv  "), "relatório.csv");
        assert_eq!(sanitize_filename(""), "sem-nome");
        assert_eq!(sanitize_filename("."), "sem-nome");
        assert_eq!(sanitize_filename("..").as_str(), "sem-nome");
        assert_eq!(sanitize_filename("a\nb.txt"), "ab.txt");
        assert_eq!(sanitize_filename(&"x".repeat(400)).len(), 255);
    }

    fn headers(pares: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pares {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }

    fn config() -> Config {
        Config {
            vault_path: "vault".into(),
            sled_index_path: "idx".into(),
            dict_path: "dicts".into(),
            db_url: "sqlite::memory:".into(),
            watch_dir: "watch".into(),
            bind_addr: "127.0.0.1:0".into(),
            max_body_size_mb: 16,
            cors_origins: vec![],
            api_keys: vec![],
            compress_multiplier: 4,
            default_effort: Effort::Balanced,
            verify_on_write: true,
            dictionaries_enabled: true,
            redis_url: None,
            limits: Default::default(),
            watch_enabled: false,
            watch_delete_source: false,
        }
    }

    #[test]
    fn query_tem_precedencia_sobre_header() {
        let p = CompressParams {
            effort: Some("max".into()),
            filename: Some("da-query.csv".into()),
            ..Default::default()
        };
        let h = headers(&[("x-effort", "fast"), ("x-filename", "do-header.csv")]);
        let req = p.resolve(&h, &config()).unwrap();
        assert_eq!(req.effort, Effort::Max);
        assert_eq!(req.filename, "da-query.csv");
    }

    #[test]
    fn header_e_usado_quando_query_esta_ausente() {
        let p = CompressParams::default();
        let h = headers(&[("x-effort", "fast"), ("x-codec", "xz"), ("x-level", "9")]);
        let req = p.resolve(&h, &config()).unwrap();
        assert_eq!(req.effort, Effort::Fast);
        assert_eq!(req.forced.codec, Some(Codec::Xz));
        assert_eq!(req.forced.level, Some(9));
    }

    #[test]
    fn nivel_fora_da_faixa_e_erro_explicito() {
        let p = CompressParams {
            codec: Some("bzip2".into()),
            level: Some(50),
            ..Default::default()
        };
        let err = p.resolve(&HeaderMap::new(), &config()).unwrap_err();
        assert_eq!(err.code, "invalid_level");
    }

    #[test]
    fn codec_e_esforco_invalidos_sao_rejeitados() {
        let p = CompressParams {
            codec: Some("magia".into()),
            ..Default::default()
        };
        assert_eq!(
            p.resolve(&HeaderMap::new(), &config()).unwrap_err().code,
            "unsupported_codec"
        );

        let p = CompressParams {
            effort: Some("turbo".into()),
            ..Default::default()
        };
        assert_eq!(
            p.resolve(&HeaderMap::new(), &config()).unwrap_err().code,
            "invalid_effort"
        );
    }

    #[test]
    fn formato_de_resposta_respeita_accept() {
        let p = CompressParams::default();
        let h = headers(&[("accept", "application/json")]);
        assert_eq!(p.format(&h, ResponseFormat::Binary), ResponseFormat::Json);

        let h = headers(&[("accept", "*/*")]);
        assert_eq!(p.format(&h, ResponseFormat::Binary), ResponseFormat::Binary);

        let p = CompressParams {
            response: Some("json".into()),
            ..Default::default()
        };
        assert_eq!(
            p.format(&HeaderMap::new(), ResponseFormat::Binary),
            ResponseFormat::Json
        );
    }

    #[test]
    fn nome_de_arquivo_com_caminho_e_neutralizado_no_resolve() {
        let p = CompressParams {
            filename: Some("../../../etc/shadow".into()),
            ..Default::default()
        };
        let req = p.resolve(&HeaderMap::new(), &config()).unwrap();
        assert_eq!(req.filename, "shadow");
    }
}
