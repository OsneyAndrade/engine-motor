use std::sync::Arc;

use axum::extract::State;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::codec::CATALOG;
use crate::planner::Effort;
use crate::service::EngineState;
use crate::transform::{self, Transform};

pub async fn metrics(State(state): State<Arc<EngineState>>) -> Response {
    let body = state.metrics.to_prometheus();
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
        .into_response()
}

pub async fn stats(State(state): State<Arc<EngineState>>) -> Json<serde_json::Value> {
    let db_stats = state.db.get_stats().await.unwrap_or_else(|e| {
        tracing::error!("falha ao ler estatísticas do banco: {e}");
        serde_json::json!({})
    });

    let process = state.metrics.to_json(
        state.dictionaries.active_count(),
        state.dictionaries.total_count(),
        state.dictionaries.hits(),
    );

    let lifetime_in = db_stats["bytes_in"].as_i64().unwrap_or(0).max(0) as u64;
    let lifetime_out = db_stats["bytes_out"].as_i64().unwrap_or(0).max(0) as u64;
    let lifetime_savings = if lifetime_in > 0 {
        (1.0 - lifetime_out as f64 / lifetime_in as f64) * 100.0
    } else {
        0.0
    };

    Json(serde_json::json!({
        "engine": "Syntra Engine",
        "version": env!("CARGO_PKG_VERSION"),
        "container_version": crate::container::CONTAINER_VERSION,
        "config": {
            "default_effort": state.config.default_effort.as_str(),
            "verify_on_write": state.config.verify_on_write,
            "dictionaries_enabled": state.config.dictionaries_enabled,
            "auth_enabled": state.config.auth_enabled(),
            "max_body_mb": state.config.max_body_size_mb,
            "compress_permits": state.max_compress_permits,
            "available_permits": state.sem_compress.available_permits(),
        },
        "process": process,
        "lifetime": {
            "bytes_in": lifetime_in,
            "bytes_out": lifetime_out,
            "savings_pct": super::dto::round2(lifetime_savings),
            "items_processed": db_stats["files_processed"].as_i64().unwrap_or(0),
            "total_duration_ms": super::dto::round2(db_stats["total_duration_ms"].as_f64().unwrap_or(0.0)),
        },
        "vault": {
            "objects_indexed": state.vault.len(),
            "path": state.config.vault_path,
        },
        "database": {
            "backend": match state.db.db_type() {
                crate::db_monitor::DbType::Sqlite => "sqlite",
                crate::db_monitor::DbType::Postgres => "postgres",
            },
        },
    }))
}

pub async fn codecs() -> Json<serde_json::Value> {
    let codecs: Vec<serde_json::Value> = CATALOG
        .iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "family": c.family,
                "level_range": [c.min_level, c.max_level],
                "default_level": c.default_level,
                "supports_dictionary": c.supports_dictionary,
                "notes": c.notes,
            })
        })
        .collect();

    let transforms: Vec<serde_json::Value> = [
        (Transform::Delta, "Séries numéricas, timestamps, IDs sequenciais, PCM."),
        (Transform::ByteSplit, "Arrays de float/int: separa planos de bytes (estilo Parquet)."),
        (Transform::Rle, "Bitmaps, máscaras e regiões constantes (PackBits)."),
        (Transform::CsvColumnar, "CSV/TSV: transpõe para layout colunar."),
    ]
    .iter()
    .map(|(t, notes)| {
        serde_json::json!({
            "name": transform::transform_name(*t),
            "widths": if matches!(t, Transform::Delta | Transform::ByteSplit) {
                serde_json::json!(transform::WIDTHS)
            } else {
                serde_json::json!([1])
            },
            "notes": notes,
        })
    })
    .collect();

    Json(serde_json::json!({
        "codecs": codecs,
        "transforms": transforms,
        "efforts": [
            {
                "name": Effort::Fast.as_str(),
                "candidates": 1,
                "notes": "Plano heurístico único. Latência mínima.",
            },
            {
                "name": Effort::Balanced.as_str(),
                "candidates": 4,
                "notes": "Mede poucos candidatos. Default.",
            },
            {
                "name": Effort::Max.as_str(),
                "candidates": 16,
                "notes": "Varredura ampla de transforms × codecs. Densidade máxima.",
            },
        ],
        "content_classes": [
            "tabular", "structured_text", "plain_text",
            "numeric_binary", "binary", "pre_compressed",
            "media", "incompressible",
        ],
    }))
}

pub async fn dictionaries(State(state): State<Arc<EngineState>>) -> Json<serde_json::Value> {
    let active: Vec<serde_json::Value> = state
        .dictionaries
        .categories()
        .into_iter()
        .map(|(class, id, size)| {
            serde_json::json!({ "content_class": class, "dictionary_id": id, "size_bytes": size })
        })
        .collect();

    Json(serde_json::json!({
        "enabled": state.config.dictionaries_enabled,
        "path": state.config.dict_path,
        "active_count": state.dictionaries.active_count(),
        "total_known": state.dictionaries.total_count(),
        "trained_this_run": state.dictionaries.trained_count(),
        "hits": state.dictionaries.hits(),
        "active": active,
        "notes": "Dicionários são endereçados por conteúdo: retreinar gera um id novo e \
                  nunca invalida essências já gravadas.",
    }))
}
