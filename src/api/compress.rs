use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderName, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};

use crate::container;
use crate::service::{self, EngineState};
use crate::tenancy::TenantContext;
use crate::usage::Operation;

use super::dto::{
    AnalyzeResponse, CompressParams, CompressResponse, DecompressMeta, ResponseFormat,
};
use super::error::{ApiError, ApiResult};
use super::content_disposition;

pub async fn compress(
    State(state): State<Arc<EngineState>>,
    Extension(ctx): Extension<TenantContext>,
    Query(params): Query<CompressParams>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let mut req = params.resolve(&headers, &state.config)?;
    req.idempotency_key = super::dto::idempotency_key(&headers);
    req.request_id = super::dto::request_id(&headers);
    let format = params.format(&headers, ResponseFormat::Binary);
    let persist = req.persist;

    let result = service::compress(&state, &ctx, body.into(), req).await?;
    let view = CompressResponse::from_result(&result, persist);

    match format {
        ResponseFormat::Json => Ok(Json(view).into_response()),
        ResponseFormat::Binary => {
            let mut response_headers = vec![
                (
                    header::CONTENT_TYPE,
                    "application/x-syntra-essence".to_string(),
                ),
                (
                    header::CONTENT_DISPOSITION,
                    content_disposition(&format!("{}.syntra", result.id)),
                ),
            ];
            for (name, value) in [
                ("x-syntra-id", view.id.clone()),
                ("x-syntra-original-size", view.original_size.to_string()),
                ("x-syntra-essence-size", view.essence_size.to_string()),
                ("x-syntra-savings-pct", format!("{:.2}", view.savings_pct)),
                ("x-syntra-plan", view.plan.label.clone()),
                ("x-syntra-content-class", view.content_class.clone()),
                ("x-syntra-verified", view.verified.to_string()),
                ("x-syntra-deduplicated", view.deduplicated.to_string()),
                ("x-syntra-tenant", ctx.tenant_id().to_string()),
            ] {
                if let Ok(n) = HeaderName::try_from(name) {
                    response_headers.push((n, value));
                }
            }
            Ok((header_map(response_headers), result.bytes).into_response())
        }
    }
}

pub async fn decompress(
    State(state): State<Arc<EngineState>>,
    Extension(ctx): Extension<TenantContext>,
    Query(params): Query<CompressParams>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    if body.is_empty() {
        return Err(ApiError::bad_request(
            "empty_body",
            "o corpo deve conter um container .syntra",
        ));
    }

    let (envelope, restored) = service::reconstruct_bytes(&state, body.into()).await?;
    let meta = DecompressMeta::build(&envelope, &restored);

    service::record_read(
        &state,
        &ctx,
        &hex::encode(&envelope.file_hash),
        restored.data.len() as u64,
        Operation::Decompress,
        super::dto::request_id(&headers),
    )
    .await;

    if params.format(&headers, ResponseFormat::Binary) == ResponseFormat::Json {
        return Ok(Json(meta).into_response());
    }

    let mut response_headers = vec![
        (header::CONTENT_TYPE, envelope.original_mime.clone()),
        (
            header::CONTENT_DISPOSITION,
            content_disposition(&envelope.original_name),
        ),
    ];
    for (name, value) in [
        ("x-syntra-filename", envelope.original_name.clone()),
        ("x-syntra-original-size", envelope.original_size.to_string()),
        ("x-syntra-plan", meta.plan.label.clone()),
        (
            "x-syntra-fidelity-checked",
            restored.fidelity_checked.to_string(),
        ),
        (
            "x-syntra-reconstruct-ms",
            format!("{:.2}", restored.duration_ms),
        ),
    ] {
        if let Ok(n) = HeaderName::try_from(name) {
            response_headers.push((n, value));
        }
    }

    Ok((header_map(response_headers), restored.data).into_response())
}

pub async fn analyze(
    State(state): State<Arc<EngineState>>,
    Extension(ctx): Extension<TenantContext>,
    Query(params): Query<CompressParams>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<AnalyzeResponse>> {
    if body.is_empty() {
        return Err(ApiError::bad_request(
            "empty_body",
            "o corpo deve conter o conteúdo a analisar",
        ));
    }

    let req = params.resolve(&headers, &state.config)?;
    let filename = req.filename.clone();
    let bytes_len = body.len() as u64;
    let report = service::analyze(
        &state,
        body.into(),
        req.filename,
        req.mime_hint,
        req.effort,
    )
    .await?;

    service::record_read(
        &state,
        &ctx,
        "",
        0,
        Operation::Analyze,
        super::dto::request_id(&headers),
    )
    .await;
    let _ = bytes_len;

    Ok(Json(AnalyzeResponse::build(report, filename)))
}

pub async fn inspect(
    State(_state): State<Arc<EngineState>>,
    body: Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    if body.is_empty() {
        return Err(ApiError::bad_request(
            "empty_body",
            "o corpo deve conter um container .syntra",
        ));
    }

    let env = container::parse(&body).map_err(crate::service::EngineError::from)?;
    let plan = super::dto::PlanView::from_envelope(&env);

    Ok(Json(serde_json::json!({
        "container_version": env.version,
        "original_name": env.original_name,
        "known_names": env.original_names,
        "mime": env.original_mime,
        "content_class": env.content_class,
        "original_size": env.original_size,
        "essence_size": env.processed_size,
        "savings_pct": super::dto::round2(env.savings_pct as f64),
        "file_hash": hex::encode(&env.file_hash),
        "plan": plan,
        "reason": env.plan_reason,
        "verified_on_write": env.verified,
        "checksum_ok": true,
        "created_at_ms": env.created_at,
        "updated_at_ms": env.updated_at,
        "metrics": env.file_metrics.as_ref().map(|m| serde_json::json!({
            "compression_ratio": super::dto::round4(m.compression_ratio),
            "processing_time_ms": super::dto::round2(m.processing_time_ms),
            "throughput_mbps": super::dto::round2(m.throughput_mbps),
            "candidates_tried": m.candidates_tried,
            "reconstruct_ms": super::dto::round2(m.reconstruct_ms),
            "strategy_reason": m.strategy_reason,
        })),
    })))
}

fn header_map(pairs: Vec<(HeaderName, String)>) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        match header::HeaderValue::from_str(&value) {
            Ok(v) => {
                map.insert(name, v);
            }
            Err(_) => {
                let safe: String = value.chars().filter(|c| c.is_ascii_graphic() || *c == ' ').collect();
                if let Ok(v) = header::HeaderValue::from_str(&safe) {
                    map.insert(name, v);
                }
            }
        }
    }
    map
}

pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "engine": "Syntra Engine",
        "version": env!("CARGO_PKG_VERSION"),
        "container_version": container::CONTAINER_VERSION,
    }))
}

pub async fn ready(State(state): State<Arc<EngineState>>) -> Response {
    let db_ok = state.db.ping().await.is_ok();
    let vault_ok = state.vault.base_dir().is_dir();

    let status = if db_ok && vault_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status,
        Json(serde_json::json!({
            "ready": db_ok && vault_ok,
            "checks": { "database": db_ok, "vault": vault_ok },
        })),
    )
        .into_response()
}
