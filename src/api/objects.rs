use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderName};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::Serialize;

use crate::service::{self, EngineState};
use crate::tenancy::TenantContext;
use crate::usage::Operation;
use crate::vault::VaultManager;

use super::content_disposition;
use super::dto::{round2, DeleteParams, DeleteResponse, ListParams, ObjectLinks, ObjectResponse};
use super::error::{ApiError, ApiResult};

fn parse_id(id: &str) -> ApiResult<[u8; 32]> {
    VaultManager::parse_id(id).map_err(|e| {
        ApiError::bad_request("invalid_object_id", e.to_string()).with_hint(
            "O identificador é o hash BLAKE3 do original, 64 dígitos hexadecimais.",
        )
    })
}

pub async fn get_object(
    State(state): State<Arc<EngineState>>,
    Extension(ctx): Extension<TenantContext>,
    Path(id): Path<String>,
) -> ApiResult<Json<ObjectResponse>> {
    let hash = parse_id(&id)?;
    let hash_hex = hex::encode(hash);
    let envelope = service::load_object(&state, ctx.tenant_id(), &hash).await?;
    let grant = crate::tenancy::TenancyStore::find_grant(&state.db, ctx.tenant_id(), &hash_hex)
        .await
        .ok()
        .flatten();
    Ok(Json(ObjectResponse::build(&hash_hex, &envelope, grant.as_ref())))
}

pub async fn get_content(
    State(state): State<Arc<EngineState>>,
    Extension(ctx): Extension<TenantContext>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let hash = parse_id(&id)?;
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
    insert(&mut headers, header::CONTENT_TYPE, &mime);
    insert(
        &mut headers,
        header::CONTENT_DISPOSITION,
        &content_disposition(&name),
    );
    for (k, v) in [
        ("x-syntra-id", hex::encode(hash)),
        ("x-syntra-filename", name),
        (
            "x-syntra-fidelity-checked",
            restored.fidelity_checked.to_string(),
        ),
        (
            "x-syntra-reconstruct-ms",
            format!("{:.2}", restored.duration_ms),
        ),
    ] {
        if let Ok(n) = HeaderName::try_from(k) {
            insert(&mut headers, n, &v);
        }
    }

    Ok((headers, restored.data).into_response())
}

pub async fn get_essence(
    State(state): State<Arc<EngineState>>,
    Extension(ctx): Extension<TenantContext>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let hash = parse_id(&id)?;
    let bytes = service::load_container(&state, ctx.tenant_id(), &hash).await?;
    let hex_id = hex::encode(hash);
    service::record_read(&state, &ctx, &hex_id, bytes.len() as u64, Operation::Read, None).await;

    let mut headers = HeaderMap::new();
    insert(
        &mut headers,
        header::CONTENT_TYPE,
        "application/x-syntra-essence",
    );
    insert(
        &mut headers,
        header::CONTENT_DISPOSITION,
        &content_disposition(&format!("{hex_id}.syntra")),
    );
    Ok((headers, bytes).into_response())
}

pub async fn delete_object(
    State(state): State<Arc<EngineState>>,
    Extension(ctx): Extension<TenantContext>,
    Path(id): Path<String>,
    Query(params): Query<DeleteParams>,
) -> ApiResult<Json<DeleteResponse>> {
    let hash = parse_id(&id)?;
    let outcome =
        service::delete_object(&state, &ctx, hash, params.purge.unwrap_or(false)).await?;
    Ok(Json(DeleteResponse {
        id: outcome.id,
        removed: outcome.blob_removed,
        remaining_references: outcome.remaining_refs,
    }))
}

#[derive(Debug, Serialize)]
pub struct ObjectListItem {
    pub id: String,
    pub original_name: String,
    pub original_size: u64,
    pub essence_size: u64,
    pub savings_pct: f64,
    pub references: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub links: ObjectLinks,
}

#[derive(Debug, Serialize)]
pub struct ObjectListResponse {
    pub count: usize,
    pub limit: i64,
    pub items: Vec<ObjectListItem>,
}

pub async fn list_objects(
    State(state): State<Arc<EngineState>>,
    Extension(ctx): Extension<TenantContext>,
    Query(params): Query<ListParams>,
) -> ApiResult<Json<ObjectListResponse>> {
    let limit = params.limit.unwrap_or(100).clamp(1, 1000);
    let grants = crate::tenancy::TenancyStore::list_grants(
        &state.db,
        ctx.tenant_id(),
        limit,
        params.search.as_deref(),
    )
    .await
    .map_err(|e| ApiError::internal(format!("falha ao listar: {e}")))?;

    let items: Vec<ObjectListItem> = grants
        .into_iter()
        .map(|g| {
            let savings = if g.original_size > 0 {
                (1.0 - g.essence_size as f64 / g.original_size as f64) * 100.0
            } else {
                0.0
            };
            ObjectListItem {
                links: ObjectLinks::for_id(&g.object_hash),
                id: g.object_hash,
                original_name: g.name,
                original_size: g.original_size.max(0) as u64,
                essence_size: g.essence_size.max(0) as u64,
                savings_pct: round2(savings),
                references: g.refs,
                created_at_ms: g.created_at_ms,
                updated_at_ms: g.updated_at_ms,
            }
        })
        .collect();

    Ok(Json(ObjectListResponse {
        count: items.len(),
        limit,
        items,
    }))
}

fn insert(headers: &mut HeaderMap, name: HeaderName, value: &str) {
    if let Ok(v) = header::HeaderValue::from_str(value) {
        headers.insert(name, v);
    }
}
