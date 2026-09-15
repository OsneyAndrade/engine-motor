use std::collections::BTreeSet;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};

use crate::container;
use crate::service::{self, EngineState};
use crate::tenancy::{
    ApiKeyRecord, PlanKind, PriceConfig, Scope, Tenant, TenancyStore, TenantContext,
};
use crate::usage;

use super::dto::round2;
use super::error::{ApiError, ApiResult};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Serialize)]
pub struct TenantView {
    pub id: String,
    pub name: String,
    pub plan: String,
    pub price: PriceConfig,
    pub active: bool,
    pub created_at_ms: i64,
}

impl From<Tenant> for TenantView {
    fn from(t: Tenant) -> Self {
        TenantView {
            id: t.id,
            name: t.name,
            plan: t.plan.as_str().to_string(),
            price: t.price,
            active: t.active,
            created_at_ms: t.created_at_ms,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct KeyView {
    pub key_id: String,
    pub tenant_id: String,
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_at_ms: Option<i64>,
    pub revoked_at_ms: Option<i64>,
    pub last_used_at_ms: Option<i64>,
    pub created_at_ms: i64,
}

impl From<ApiKeyRecord> for KeyView {
    fn from(r: ApiKeyRecord) -> Self {
        KeyView {
            key_id: r.key_id,
            tenant_id: r.tenant_id,
            name: r.name,
            scopes: r.scopes.iter().map(|s| s.as_str().to_string()).collect(),
            expires_at_ms: r.expires_at_ms,
            revoked_at_ms: r.revoked_at_ms,
            last_used_at_ms: r.last_used_at_ms,
            created_at_ms: r.created_at_ms,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct IssuedKeyView {
    pub key: String,
    pub warning: &'static str,
    #[serde(flatten)]
    pub record: KeyView,
}

#[derive(Debug, Deserialize)]
pub struct CreateTenantBody {
    pub id: Option<String>,
    pub name: String,
    pub plan: Option<String>,
    pub price: Option<PriceConfig>,
    pub active: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct CreateKeyBody {
    pub name: Option<String>,
    pub scopes: Option<Vec<String>>,
    pub expires_in_days: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
pub struct UsageQuery {
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub days: Option<i64>,
    pub tenant_id: Option<String>,
    pub events: Option<i64>,
}

impl UsageQuery {
    fn window(&self) -> (i64, i64) {
        let now = container::now_millis();
        let to = self.to_ms.unwrap_or(now + 1);
        let from = self
            .from_ms
            .unwrap_or_else(|| to - self.days.unwrap_or(30).clamp(1, 366) * DAY_MS);
        (from.min(to), to)
    }
}

fn parse_id(raw: &str) -> ApiResult<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(64)
        .collect();
    if cleaned.is_empty() || cleaned != raw {
        return Err(ApiError::bad_request(
            "invalid_tenant_id",
            "o identificador aceita apenas letras, dígitos, '-', '_' e '.'",
        ));
    }
    Ok(cleaned)
}

fn parse_scopes(raw: Option<Vec<String>>) -> ApiResult<BTreeSet<Scope>> {
    match raw {
        None => Ok([Scope::Compress, Scope::Read].into_iter().collect()),
        Some(list) => {
            let mut out = BTreeSet::new();
            for item in &list {
                let scope = Scope::parse(item).ok_or_else(|| {
                    ApiError::bad_request(
                        "invalid_scope",
                        format!("escopo '{item}' inválido; use compress, read, delete ou admin"),
                    )
                })?;
                out.insert(scope);
            }
            if out.is_empty() {
                return Err(ApiError::bad_request(
                    "invalid_scope",
                    "informe ao menos um escopo",
                ));
            }
            Ok(out)
        }
    }
}

pub async fn list_tenants(
    State(state): State<Arc<EngineState>>,
) -> ApiResult<Json<Vec<TenantView>>> {
    let tenants = TenancyStore::list_tenants(&state.db)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(tenants.into_iter().map(TenantView::from).collect()))
}

pub async fn create_tenant(
    State(state): State<Arc<EngineState>>,
    Json(body): Json<CreateTenantBody>,
) -> ApiResult<Json<TenantView>> {
    let id = match body.id {
        Some(raw) => parse_id(&raw)?,
        None => uuid::Uuid::new_v4().to_string(),
    };

    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("invalid_name", "informe o nome"));
    }

    let plan = match body.plan {
        Some(ref p) => PlanKind::parse(p).ok_or_else(|| {
            ApiError::bad_request(
                "invalid_plan",
                format!("plano '{p}' inválido; use savings_share ou ingested_gb"),
            )
        })?,
        None => PlanKind::SavingsShare,
    };

    let tenant = Tenant {
        id,
        name: body.name.trim().to_string(),
        plan,
        price: body.price.unwrap_or_default(),
        active: body.active.unwrap_or(true),
        created_at_ms: container::now_millis(),
    };

    TenancyStore::upsert_tenant(&state.db, &tenant)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(TenantView::from(tenant)))
}

pub async fn get_tenant(
    State(state): State<Arc<EngineState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<TenantView>> {
    let id = parse_id(&id)?;
    TenancyStore::find_tenant(&state.db, &id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map(|t| Json(TenantView::from(t)))
        .ok_or_else(|| ApiError::not_found(format!("tenant {id} não encontrado")))
}

pub async fn list_keys(
    State(state): State<Arc<EngineState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<KeyView>>> {
    let id = parse_id(&id)?;
    let keys = TenancyStore::list_keys(&state.db, &id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(keys.into_iter().map(KeyView::from).collect()))
}

pub async fn create_key(
    State(state): State<Arc<EngineState>>,
    Path(id): Path<String>,
    Json(body): Json<CreateKeyBody>,
) -> ApiResult<Json<IssuedKeyView>> {
    let id = parse_id(&id)?;
    if TenancyStore::find_tenant(&state.db, &id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .is_none()
    {
        return Err(ApiError::not_found(format!("tenant {id} não encontrado")));
    }

    let scopes = parse_scopes(body.scopes)?;
    let now = container::now_millis();
    let mut issued = crate::tenancy::generate_key(
        &id,
        body.name.as_deref().unwrap_or("sem-nome"),
        scopes,
        now,
    );

    if let Some(days) = body.expires_in_days {
        if days <= 0 {
            return Err(ApiError::bad_request(
                "invalid_expiry",
                "expires_in_days deve ser positivo",
            ));
        }
        issued.record.expires_at_ms = Some(now + days.clamp(1, 3650) * DAY_MS);
    }

    TenancyStore::store_key(&state.db, &issued)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    state.mark_auth_required();

    Ok(Json(IssuedKeyView {
        key: issued.secret,
        warning: "A chave é exibida apenas nesta resposta. Guarde-a agora: o servidor \
                  armazena somente o hash e não há como recuperá-la.",
        record: KeyView::from(issued.record),
    }))
}

pub async fn revoke_key(
    State(state): State<Arc<EngineState>>,
    Path(key_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let key_id = parse_id(&key_id)?;
    let revoked = TenancyStore::revoke_key(&state.db, &key_id, container::now_millis())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if !revoked {
        return Err(ApiError::not_found(format!(
            "chave {key_id} não encontrada ou já revogada"
        )));
    }
    Ok(Json(serde_json::json!({ "key_id": key_id, "revoked": true })))
}

#[derive(Debug, Serialize)]
pub struct UsageResponse {
    pub tenant_id: String,
    pub from_ms: i64,
    pub to_ms: i64,
    pub totals: UsageTotalsView,
    pub quote: usage::Quote,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub recent: Vec<UsageEventView>,
}

#[derive(Debug, Serialize)]
pub struct UsageTotalsView {
    pub events: i64,
    pub bytes_in: i64,
    pub bytes_out: i64,
    pub bytes_saved: i64,
    pub savings_pct: f64,
    pub cpu_ms: f64,
    pub dedup_same_tenant: i64,
    pub dedup_cross_tenant: i64,
    pub by_operation: Vec<usage::OperationBucket>,
    pub by_effort: Vec<usage::EffortBucket>,
}

#[derive(Debug, Serialize)]
pub struct UsageEventView {
    pub id: String,
    pub operation: String,
    pub object_id: Option<String>,
    pub bytes_in: i64,
    pub bytes_out: i64,
    pub bytes_saved: i64,
    pub effort: String,
    pub plan: String,
    pub dedup_scope: String,
    pub occurred_at_ms: i64,
}

async fn build_usage(
    state: &Arc<EngineState>,
    ctx: &TenantContext,
    query: &UsageQuery,
) -> ApiResult<UsageResponse> {
    let (from, to) = query.window();
    let (totals, quote) = service::quote_usage(state, ctx, from, to).await?;

    let recent = match query.events {
        Some(n) if n > 0 => usage::recent(&state.db, ctx.tenant_id(), n)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .into_iter()
            .map(|e| UsageEventView {
                id: e.id,
                operation: e.operation,
                object_id: e.object_hash,
                bytes_in: e.bytes_in,
                bytes_out: e.bytes_out,
                bytes_saved: e.bytes_saved,
                effort: e.effort,
                plan: e.codec,
                dedup_scope: e.dedup_scope,
                occurred_at_ms: e.occurred_at_ms,
            })
            .collect(),
        _ => Vec::new(),
    };

    let savings_pct = if totals.bytes_in > 0 {
        totals.bytes_saved as f64 * 100.0 / totals.bytes_in as f64
    } else {
        0.0
    };

    Ok(UsageResponse {
        tenant_id: ctx.tenant_id().to_string(),
        from_ms: from,
        to_ms: to,
        totals: UsageTotalsView {
            events: totals.events,
            bytes_in: totals.bytes_in,
            bytes_out: totals.bytes_out,
            bytes_saved: totals.bytes_saved,
            savings_pct: round2(savings_pct),
            cpu_ms: round2(totals.cpu_ms),
            dedup_same_tenant: totals.dedup_tenant,
            dedup_cross_tenant: totals.dedup_global,
            by_operation: totals.by_operation.clone(),
            by_effort: totals.by_effort.clone(),
        },
        quote,
        recent,
    })
}

pub async fn my_usage(
    State(state): State<Arc<EngineState>>,
    Extension(ctx): Extension<TenantContext>,
    Query(query): Query<UsageQuery>,
) -> ApiResult<Json<UsageResponse>> {
    Ok(Json(build_usage(&state, &ctx, &query).await?))
}

pub async fn tenant_usage(
    State(state): State<Arc<EngineState>>,
    Query(query): Query<UsageQuery>,
) -> ApiResult<Json<UsageResponse>> {
    let tenant_id = query
        .tenant_id
        .clone()
        .ok_or_else(|| ApiError::bad_request("missing_tenant", "informe tenant_id"))?;
    let tenant_id = parse_id(&tenant_id)?;

    let tenant = TenancyStore::find_tenant(&state.db, &tenant_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("tenant {tenant_id} não encontrado")))?;

    let ctx = TenantContext {
        tenant,
        key_id: None,
        scopes: Scope::all(),
    };
    Ok(Json(build_usage(&state, &ctx, &query).await?))
}
