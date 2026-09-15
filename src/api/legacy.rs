use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderName};
use axum::response::{IntoResponse, Response};
use axum::middleware;
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use serde::Serialize;

use crate::service::{self, EngineState};
use crate::tenancy::{Scope, TenantContext};
use crate::usage::Operation;
use crate::vault::VaultManager;

use super::content_disposition;
use super::dto::{round2, CompressParams, ListParams};
use super::error::{ApiError, ApiResult};

pub fn router() -> Router<Arc<EngineState>> {
    let write = Router::new()
        .route("/process", post(process))
        .route("/compress", post(process))
        .route("/api/sim/process", post(sim_process))
        .layer(middleware::from_fn_with_state(
            Scope::Compress,
            super::require_scope,
        ));

    let read = Router::new()
        .route("/reconstruct", post(reconstruct))
        .route("/decompress", post(reconstruct))
        .route("/stats", get(stats))
        .route("/api/files", get(list_files))
        .route("/api/files/:id", get(download_file))
        .route("/api/sim/download/:id", get(download_file))
        .layer(middleware::from_fn_with_state(
            Scope::Read,
            super::require_scope,
        ));

    let remove = Router::new()
        .route("/api/files/:id", delete(delete_file))
        .layer(middleware::from_fn_with_state(
            Scope::Delete,
            super::require_scope,
        ));

    Router::new().merge(write).merge(read).merge(remove)
}

async fn process(
    State(state): State<Arc<EngineState>>,
    Extension(ctx): Extension<TenantContext>,
    Query(params): Query<CompressParams>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let mut req = params.resolve(&headers, &state.config)?;
    req.idempotency_key = super::dto::idempotency_key(&headers);
    req.request_id = super::dto::request_id(&headers);
    let result = service::compress(&state, &ctx, body.into(), req).await?;

    let mut out = HeaderMap::new();
    out.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/x-protobuf"),
    );
    for (k, v) in [
        ("x-syn-filename", format!("{}.syntra", result.id)),
        ("x-syntra-id", result.id.clone()),
        ("x-syntra-savings-pct", format!("{:.2}", result.savings_pct())),
    ] {
        if let (Ok(name), Ok(value)) = (
            HeaderName::try_from(k),
            header::HeaderValue::from_str(&v),
        ) {
            out.insert(name, value);
        }
    }

    Ok((out, result.bytes).into_response())
}

async fn reconstruct(
    State(state): State<Arc<EngineState>>,
    Extension(ctx): Extension<TenantContext>,
    Query(params): Query<CompressParams>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    super::compress::decompress(State(state), Extension(ctx), Query(params), headers, body).await
}

async fn stats(State(state): State<Arc<EngineState>>) -> Json<serde_json::Value> {
    let db_stats = state
        .db
        .get_stats()
        .await
        .unwrap_or_else(|_| serde_json::json!({}));

    let bytes_in = db_stats["bytes_in"].as_i64().unwrap_or(0).max(0);
    let bytes_out = db_stats["bytes_out"].as_i64().unwrap_or(0).max(0);
    let files = db_stats["files_processed"].as_i64().unwrap_or(0);
    let savings = if bytes_in > 0 {
        (1.0 - bytes_out as f64 / bytes_in as f64) * 100.0
    } else {
        0.0
    };

    let usage: std::collections::HashMap<&str, u64> =
        state.metrics.codec_usage().into_iter().collect();
    let get = |k: &str| *usage.get(k).unwrap_or(&0);

    let process = state.metrics.to_json(
        state.dictionaries.active_count(),
        state.dictionaries.total_count(),
        state.dictionaries.hits(),
    );

    Json(serde_json::json!({
        "engine": "Syntra Engine",
        "uptime_seconds": process["uptime_seconds"],
        "roi": {
            "total_bytes_in": bytes_in,
            "essence_bytes_out": bytes_out,
            "efficiency_pct": format!("{:.1}%", savings),
            "items_processed": files,
            "errors": process["throughput"]["errors"],
        },
        "deduplication": process["deduplication"],
        "context_models": {
            "active_trained_count": state.dictionaries.active_count(),
            "hits": state.dictionaries.hits(),
        },
        "processing_strategies": {
            "ultra_fast": get("lz4"),
            "fast": get("zstd"),
            "balanced": get("deflate"),
            "max_density": get("xz") + get("brotli") + get("bzip2"),
            "passthrough": get("stored"),
            "by_codec": process["codec_usage"],
        },
        "system": process["system"],
        "latency": {
            "sum_ms": db_stats["total_duration_ms"].as_f64().unwrap_or(0.0),
            "count": files,
        },
    }))
}

#[derive(Debug, Serialize)]
struct EssenceFile {
    name: String,
    original_name: String,
    original_size: u64,
    essence_size: u64,
    savings_pct: f32,
    algorithm: String,
    processing_time_ms: f64,
    created_at: String,
}

async fn list_files(
    State(state): State<Arc<EngineState>>,
    Extension(ctx): Extension<TenantContext>,
    Query(params): Query<ListParams>,
) -> ApiResult<Json<Vec<EssenceFile>>> {
    let limit = params.limit.unwrap_or(100).clamp(1, 1000);
    let grants = crate::tenancy::TenancyStore::list_grants(
        &state.db,
        ctx.tenant_id(),
        limit,
        params.search.as_deref(),
    )
    .await
    .map_err(|e| ApiError::internal(format!("falha ao listar: {e}")))?;

    Ok(Json(
        grants
            .into_iter()
            .map(|g| {
                let savings = if g.original_size > 0 {
                    (1.0 - g.essence_size as f64 / g.original_size as f64) * 100.0
                } else {
                    0.0
                };
                EssenceFile {
                    name: g.object_hash,
                    original_name: g.name,
                    original_size: g.original_size.max(0) as u64,
                    essence_size: g.essence_size.max(0) as u64,
                    savings_pct: round2(savings) as f32,
                    algorithm: String::new(),
                    processing_time_ms: 0.0,
                    created_at: (g.created_at_ms / 1000).to_string(),
                }
            })
            .collect(),
    ))
}

async fn download_file(
    State(state): State<Arc<EngineState>>,
    Extension(ctx): Extension<TenantContext>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let hash = VaultManager::parse_id(&id)
        .map_err(|e| ApiError::bad_request("invalid_object_id", e.to_string()))?;

    let envelope = service::load_object(&state, ctx.tenant_id(), &hash).await?;
    let name = envelope.original_name.clone();
    let mime = envelope.original_mime.clone();
    let restored = service::reconstruct(&state, envelope).await?;
    service::record_read(
        &state,
        &ctx,
        &hex::encode(hash),
        restored.data.len() as u64,
        Operation::Read,
        None,
    )
    .await;

    let mut headers = HeaderMap::new();
    if let Ok(v) = header::HeaderValue::from_str(&mime) {
        headers.insert(header::CONTENT_TYPE, v);
    }
    if let Ok(v) = header::HeaderValue::from_str(&content_disposition(&name)) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    for (k, v) in [
        ("x-filename", name),
        ("x-file-id", hex::encode(hash)),
        (
            "x-reconstruct-time-ms",
            format!("{:.2}", restored.duration_ms),
        ),
    ] {
        if let (Ok(n), Ok(value)) = (
            HeaderName::try_from(k),
            header::HeaderValue::from_str(&v),
        ) {
            headers.insert(n, value);
        }
    }

    Ok((headers, restored.data).into_response())
}

async fn delete_file(
    State(state): State<Arc<EngineState>>,
    Extension(ctx): Extension<TenantContext>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let hash = VaultManager::parse_id(&id)
        .map_err(|e| ApiError::bad_request("invalid_object_id", e.to_string()))?;

    let outcome = service::delete_object(&state, &ctx, hash, true).await?;
    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Arquivo deletado com sucesso",
        "id": outcome.id,
    })))
}

#[derive(Debug, Serialize)]
struct SimResult {
    original_name: String,
    original_size: u64,
    reduced_size: u64,
    savings_pct: f64,
    algorithm: String,
    processing_time_ms: f64,
    download_id: String,
    success: bool,
    deduplicated: bool,
    verified: bool,
    content_class: String,
}

async fn sim_process(
    State(state): State<Arc<EngineState>>,
    Extension(ctx): Extension<TenantContext>,
    Query(params): Query<CompressParams>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<SimResult>> {
    let mut req = params.resolve(&headers, &state.config)?;
    req.idempotency_key = super::dto::idempotency_key(&headers);
    let name = req.filename.clone();
    let result = service::compress(&state, &ctx, body.into(), req).await?;

    Ok(Json(SimResult {
        original_name: name,
        original_size: result.envelope.original_size,
        reduced_size: result.envelope.processed_size,
        savings_pct: round2(result.savings_pct()),
        algorithm: service::codec_label(&result.envelope),
        processing_time_ms: round2(result.compress_ms),
        download_id: result.id.clone(),
        success: true,
        deduplicated: result.deduplicated,
        verified: result.envelope.verified,
        content_class: result.envelope.content_class.clone(),
    }))
}
