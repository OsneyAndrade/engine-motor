#[cfg(test)]
mod e2e;

pub mod accounts;
pub mod admin;
pub mod compress;
pub mod dto;
pub mod error;
pub mod legacy;
pub mod objects;
pub mod openapi;

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{header, HeaderValue, Method};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{delete, get, get_service, post};
use axum::Router;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

use crate::container;
use crate::service::EngineState;
use crate::tenancy::{Scope, TenancyStore, TenantContext};

use error::ApiError;

const REQUEST_ID_HEADER: &str = "x-request-id";
pub const IDEMPOTENCY_HEADER: &str = "idempotency-key";

pub fn router(state: Arc<EngineState>) -> Router {
    let write = Router::new()
        .route("/api/v1/compress", post(compress::compress))
        .layer(middleware::from_fn_with_state(Scope::Compress, require_scope));

    let read = Router::new()
        .route("/api/v1/decompress", post(compress::decompress))
        .route("/api/v1/analyze", post(compress::analyze))
        .route("/api/v1/inspect", post(compress::inspect))
        .route("/api/v1/objects", get(objects::list_objects))
        .route("/api/v1/objects/:id", get(objects::get_object))
        .route("/api/v1/objects/:id/content", get(objects::get_content))
        .route("/api/v1/objects/:id/essence", get(objects::get_essence))
        .route("/api/v1/usage", get(accounts::my_usage))
        .route("/api/v1/codecs", get(admin::codecs))
        .route("/api/v1/dictionaries", get(admin::dictionaries))
        .route("/api/v1/stats", get(admin::stats))
        .layer(middleware::from_fn_with_state(Scope::Read, require_scope));

    let remove = Router::new()
        .route("/api/v1/objects/:id", delete(objects::delete_object))
        .layer(middleware::from_fn_with_state(Scope::Delete, require_scope));

    let admin_routes = Router::new()
        .route("/api/v1/admin/tenants", get(accounts::list_tenants))
        .route("/api/v1/admin/tenants", post(accounts::create_tenant))
        .route("/api/v1/admin/tenants/:id", get(accounts::get_tenant))
        .route("/api/v1/admin/tenants/:id/keys", get(accounts::list_keys))
        .route("/api/v1/admin/tenants/:id/keys", post(accounts::create_key))
        .route("/api/v1/admin/keys/:key_id", delete(accounts::revoke_key))
        .route("/api/v1/admin/usage", get(accounts::tenant_usage))
        .layer(middleware::from_fn_with_state(Scope::Admin, require_scope));

    let authenticated = Router::new()
        .merge(write)
        .merge(read)
        .merge(remove)
        .merge(admin_routes)
        .merge(legacy::router())
        .layer(middleware::from_fn_with_state(state.clone(), authenticate));

    let open = Router::new()
        .route("/health", get(compress::health))
        .route("/ready", get(compress::ready))
        .route("/metrics", get(admin::metrics))
        .route("/api/v1/openapi.json", get(openapi::document))
        .route("/", get_service(ServeFile::new("static/dashboard.html")))
        .route(
            "/dashboard",
            get_service(ServeFile::new("static/dashboard.html")),
        )
        .nest_service("/static", get_service(ServeDir::new("static")));

    Router::new()
        .merge(open)
        .merge(authenticated)
        .layer(DefaultBodyLimit::max(
            state.config.max_body_size_mb.saturating_mul(1024 * 1024),
        ))
        .layer(cors_layer(&state.config.cors_origins))
        .layer(middleware::from_fn(request_id))
        .with_state(state)
}

fn cors_layer(origins: &[String]) -> CorsLayer {
    if origins.is_empty() {
        return CorsLayer::permissive();
    }

    let parsed: Vec<HeaderValue> = origins
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(parsed))
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers([
            header::CONTENT_TYPE,
            header::ACCEPT,
            header::AUTHORIZATION,
            header::HeaderName::from_static("x-api-key"),
            header::HeaderName::from_static("idempotency-key"),
            header::HeaderName::from_static("x-filename"),
            header::HeaderName::from_static("x-codec"),
            header::HeaderName::from_static("x-level"),
            header::HeaderName::from_static("x-effort"),
            header::HeaderName::from_static("x-verify"),
            header::HeaderName::from_static("x-mime"),
            header::HeaderName::from_static("x-dictionary"),
            header::HeaderName::from_static("x-response-format"),
        ])
        .expose_headers([
            header::CONTENT_DISPOSITION,
            header::HeaderName::from_static("x-request-id"),
            header::HeaderName::from_static("x-syntra-id"),
            header::HeaderName::from_static("x-syntra-original-size"),
            header::HeaderName::from_static("x-syntra-essence-size"),
            header::HeaderName::from_static("x-syntra-savings-pct"),
            header::HeaderName::from_static("x-syntra-plan"),
            header::HeaderName::from_static("x-syntra-content-class"),
            header::HeaderName::from_static("x-syntra-verified"),
            header::HeaderName::from_static("x-syntra-deduplicated"),
            header::HeaderName::from_static("x-syntra-tenant"),
            header::HeaderName::from_static("x-syntra-filename"),
            header::HeaderName::from_static("x-syntra-fidelity-checked"),
            header::HeaderName::from_static("x-syntra-reconstruct-ms"),
        ])
}

async fn request_id(mut req: Request, next: Next) -> Response {
    let incoming = req
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty() && v.len() <= 128 && v.chars().all(|c| c.is_ascii_graphic()))
        .map(str::to_string);

    let id = incoming.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    if let Ok(value) = HeaderValue::from_str(&id) {
        req.headers_mut().insert(
            header::HeaderName::from_static(REQUEST_ID_HEADER),
            value.clone(),
        );
        let mut response = next.run(req).await;
        response
            .headers_mut()
            .insert(header::HeaderName::from_static(REQUEST_ID_HEADER), value);
        return response;
    }

    next.run(req).await
}

fn presented_key(req: &Request) -> Option<String> {
    req.headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            req.headers()
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
}

async fn authenticate(
    State(state): State<Arc<EngineState>>,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let now = container::now_millis();

    let ctx = match presented_key(&req) {
        Some(presented) => {
            let static_match = state
                .config
                .api_keys
                .iter()
                .any(|k| constant_time_eq(k.as_bytes(), presented.as_bytes()));

            if static_match {
                state.default_context()
            } else {
                match TenancyStore::authenticate(&state.db, &presented, now).await {
                    Ok(Some(ctx)) => ctx,
                    Ok(None) => return Err(ApiError::unauthorized("credencial inválida")),
                    Err(e) => return Err(ApiError::internal(e.to_string())),
                }
            }
        }
        None => {
            if state.auth_required() {
                return Err(ApiError::unauthorized("credencial ausente").with_hint(
                    "Envie a chave em 'x-api-key: <chave>' ou 'Authorization: Bearer <chave>'.",
                ));
            }
            state.default_context()
        }
    };

    req.extensions_mut().insert(ctx);
    Ok(next.run(req).await)
}

async fn require_scope(
    State(scope): State<Scope>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let allowed = req
        .extensions()
        .get::<TenantContext>()
        .map(|ctx| ctx.has(scope))
        .unwrap_or(false);

    if allowed {
        Ok(next.run(req).await)
    } else {
        Err(ApiError::forbidden(format!(
            "esta rota exige o escopo '{scope}'"
        )))
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn content_disposition(filename: &str) -> String {
    let ascii: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ' ' | '(' | ')') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let ascii = if ascii.chars().any(|c| c.is_ascii_alphanumeric()) {
        ascii
    } else {
        "download".to_string()
    };

    if filename.is_ascii() {
        format!("attachment; filename=\"{ascii}\"")
    } else {
        let encoded =
            percent_encoding::utf8_percent_encode(filename, percent_encoding::NON_ALPHANUMERIC);
        format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparacao_constante_esta_correta() {
        assert!(constant_time_eq(b"segredo", b"segredo"));
        assert!(!constant_time_eq(b"segredo", b"segred0"));
        assert!(!constant_time_eq(b"segredo", b"segredo-maior"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn content_disposition_ascii_e_simples() {
        assert_eq!(
            content_disposition("relatorio.csv"),
            "attachment; filename=\"relatorio.csv\""
        );
    }

    #[test]
    fn content_disposition_utf8_traz_fallback_ascii() {
        let d = content_disposition("relatório anual (2026).csv");
        assert!(d.contains("filename=\""), "falta o fallback ASCII: {d}");
        assert!(d.contains("filename*=UTF-8''"), "falta o RFC 5987: {d}");
        let fallback = d.split("filename=\"").nth(1).unwrap().split('"').next().unwrap();
        assert!(fallback.is_ascii(), "fallback não é ASCII: {fallback}");
    }

    #[test]
    fn content_disposition_neutraliza_aspas_e_caminho() {
        let d = content_disposition("a\"b/../c.txt");
        let fallback = d.split("filename=\"").nth(1).unwrap().split('"').next().unwrap();
        assert!(!fallback.contains('/'));
    }

    #[test]
    fn nome_vazio_cai_para_download() {
        assert_eq!(
            content_disposition("///"),
            "attachment; filename=\"download\""
        );
    }
}
