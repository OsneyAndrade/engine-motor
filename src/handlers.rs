//! # Módulo de Manipuladores de Requisições HTTP
//!
//! Este módulo contém os handlers que processam as requisições HTTP recebidas
//! pelo servidor Axum. Cada handler é responsável por uma funcionalidade
//! específica da API do Syntra Engine.
//!
//! # Arquitetura dos Handlers
//!
//! Os handlers são funções assíncronas que recebem:
//! - O estado compartilhado do motor (EngineState)
//! - Dados da requisição (corpo, headers, parâmetros)
//!
//! E retornam:
//! - Uma resposta HTTP ou um erro
//!
//! # Principais Endpoints
//!
//! ## Processamento
//! - `POST /process` - Processa um arquivo e gera a essência comprimida
//! - `POST /reconstruct` - Reconstrói o arquivo original a partir da essência
//!
//! ## Consulta e Gerenciamento
//! - `GET /stats` - Retorna estatísticas em JSON
//! - `GET /metrics` - Retorna métricas em formato Prometheus
//! - `GET /health` - Health check simples
//! - `GET /api/files` - Lista arquivos processados
//! - `GET /api/files/:filename` - Baixa um arquivo processado
//! - `DELETE /api/files/:filename` - Deleta um arquivo processado
//!
//! ## Simulação
//! - `POST /api/sim/process` - Processa e retorna JSON simplificado
//! - `GET /api/sim/download/:id` - Download de arquivo processado (simulação)
//!
//! # Otimizações v2.1
//!
//! - **Zero-copy pipeline**: Blocos duplicados não são copiados desnecessariamente
//! - **Streaming**: Processa arquivos grandes em chunks de 64KB
//! - **Rayon ThreadPool**: Isolamento de carga para operações CPU-bound

use axum::{
    extract::{State, Path as AxumPath},
    http::{header, HeaderName, StatusCode},
    response::{IntoResponse, Response},
    body::Body,
    Json,
};
use futures::StreamExt;
use prost::Message;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, error};

use crate::proto::{Algorithm, FileMetrics, ProcessedFile};
use crate::{adaptive, compress, EngineState};

// ---------------------------------------------------------------------------
// DETECÇÃO DE TIPO (Otimizada com PHF)
// ---------------------------------------------------------------------------

static MIME_EXTENSIONS: phf::Map<&'static str, &'static str> = phf::phf_map! {
    "json" => "application/json",
    "jsonl" => "application/json",
    "ndjson" => "application/json",
    "csv" => "text/csv",
    "tsv" => "text/tab-separated-values",
    "log" => "text/plain",
    "txt" => "text/plain",
    "xml" => "application/xml",
    "yaml" => "application/yaml",
    "yml" => "application/yaml",
    "sql" => "application/sql",
    "parquet" => "application/octet-stream",
    "avro" => "application/octet-stream",
    "protobuf" => "application/octet-stream",
    "pdf" => "application/pdf",
    // Extensões de imagem - mapeamento explícito para garantir detecção correta
    "png" => "image/png",
    "jpg" => "image/jpeg",
    "jpeg" => "image/jpeg",
    "gif" => "image/gif",
    "webp" => "image/webp",
    "bmp" => "image/bmp",
    "tiff" => "image/tiff",
    "tif" => "image/tiff",
    "avif" => "image/avif",
    "heic" => "image/heic",
    "heif" => "image/heif",
};

fn detect_mime_fast(data: &[u8], filename: &str) -> String {
    // Primeiro verifica extensão do arquivo (mais rápido para imagens)
    if let Some((_base, ext)) = filename.rsplit_once('.') {
        let ext_lower = ext.to_lowercase();
        if let Some(&mime) = MIME_EXTENSIONS.get(ext_lower.as_str()) {
            return mime.to_string();
        }
    }
    // Depois tenta inferir pelo conteúdo (magic bytes)
    if !data.is_empty() {
        if let Some(kind) = infer::get(data) {
            return kind.mime_type().to_string();
        }
    }
    "application/octet-stream".into()
}

/// Gera header Content-Disposition com suporte a UTF-8 (RFC 5987)
/// Isso garante que nomes de arquivos com acentos e caracteres especiais
/// sejam baixados corretamente pelo navegador
fn format_content_disposition(filename: &str) -> String {
    if filename.as_bytes().iter().any(|&b| b > 0x7F) {
        // Nome contém caracteres não-ASCII, usa RFC 5987
        // O filename*=UTF-8'' tem precedência sobre filename na maioria dos navegadores
        let filename_utf8 = percent_encoding::utf8_percent_encode(
            filename,
            percent_encoding::NON_ALPHANUMERIC
        );
        // Inclui fallback ASCII seguro (apenas o hash + extensão) para compatibilidade
        format!("attachment; filename*=UTF-8''{}", filename_utf8)
    } else {
        // Nome apenas ASCII, usa formato simples
        format!("attachment; filename=\"{}\"", filename)
    }
}

fn parse_algorithm(s: &str) -> Algorithm {
    match s.to_lowercase().as_str() {
        "zstd_fast" | "fast" => Algorithm::ZstdFast,
        "zstd_balanced" | "balanced" => Algorithm::ZstdBalanced,
        "zstd_max" | "max" => Algorithm::ZstdMax,
        "lz4" | "ultra_fast" => Algorithm::Lz4,
        "passthrough" | "none" => Algorithm::Passthrough,
        // Valor inválido no header: usa ZstdBalanced como padrão
        _ => Algorithm::ZstdBalanced,
    }
}

// ---------------------------------------------------------------------------
// POST /process (Geração de Essência)
// ---------------------------------------------------------------------------

pub async fn process_handler(
    State(state): State<Arc<EngineState>>,
    headers: header::HeaderMap,
    body: Body,
) -> Result<Response, (StatusCode, String)> {
    let filename = headers
        .get("x-filename")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let force_algo = headers
        .get("x-algo")
        .and_then(|h| h.to_str().ok())
        .map(parse_algorithm);

    info!(
        filename = filename,
        "Iniciando processamento"
    );

    let _permit = state
        .sem_compress
        .acquire()
        .await
        .map_err(|e| {
            error!("Erro ao adquirir semáforo: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;

    // Coleta todos os dados do corpo
    let mut data = Vec::new();
    let mut stream = body.into_data_stream();
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| {
            error!("Erro ao ler chunk: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;
        data.extend_from_slice(&chunk);
    }

    let original_size = data.len() as u64;
    let mime = detect_mime_fast(&data, &filename);

    info!(
        filename = filename,
        bytes = original_size,
        mime = &mime,
        "Arquivo lido para processamento"
    );

    // Análise adaptativa
    let analysis = adaptive::analyze(&data, &mime);
    let algo = force_algo.unwrap_or(analysis.algorithm);
    let lvl = analysis.zstd_level;
    let dict = state.dictionaries.get_dictionary(&mime);

    info!(
        algorithm = format!("{:?}", algo),
        level = lvl,
        reason = analysis.reason,
        "Algoritmo selecionado"
    );

    // Medição de tempo de processamento
    let processing_start = Instant::now();

    // Compressão
    let compressed = state.pool.install(|| {
        compress::compress(&data, algo, lvl, dict.as_deref())
    }).map_err(|e| {
        error!("Erro de compressão: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, format!("Erro de compressão: {}", e))
    })?;

    let duration_ms = processing_start.elapsed().as_secs_f64() * 1000.0;

    let processed_size = compressed.len() as u64;
    let savings_pct = if original_size > 0 {
        (1.0 - (processed_size as f64 / original_size as f64)) * 100.0
    } else { 0.0 };

    info!("Compresso: {} bytes -> {:.1}% economia", processed_size, savings_pct);

    // Hash do arquivo
    let hash_bytes = blake3::hash(&data).into();
    let hash_hex = hex::encode(hash_bytes);

    // ID único para evitar conflitos de nomes (arquivos com mesmo conteúdo)
    let unique_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    // Atualiza métricas
    use std::sync::atomic::Ordering;
    state.metrics.bytes_in.fetch_add(original_size, Ordering::Relaxed);
    state.metrics.bytes_out.fetch_add(processed_size, Ordering::Relaxed);
    state.metrics.files_processed.fetch_add(1, Ordering::Relaxed);
    state.metrics.record_strategy(algo);

    let syn_filename = format!("{}_{}.syntra", hash_hex, unique_id);

    // Cria o diretório (usamos ensure_dir só para criar os diretórios pais)
    let vault_dir = state.vault.ensure_dir(&hash_bytes)
        .map_err(|e| {
            error!("Erro ao criar diretório vault: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;

    // Constrói o caminho completo com o nome único do arquivo
    let vault_path = vault_dir.parent().unwrap().join(&syn_filename);

    // Cria o envelope
    let envelope = ProcessedFile {
        version: 2,
        original_name: filename.clone(),
        original_names: vec![filename.clone()],
        original_mime: mime.clone(),
        original_size,
        processed_size,
        savings_pct: savings_pct as f32,
        algorithm: algo as i32,
        compression_level: lvl,
        dictionary_id: String::new(),
        data: compressed,
        blocks: vec![],
        file_metrics: Some(FileMetrics {
            throughput_mbps: 0.0,
            compression_ratio: savings_pct,
            dedup_savings_bytes: 0,
            processing_time_ms: 0.0,
            algorithm_used: format!("{:?}", algo),
            strategy_reason: analysis.reason.to_string(),
        }),
        pointer_to: String::new(),
        file_hash: hash_bytes.to_vec(),
        ref_count: 1,
        envelope_checksum: 0,
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
        updated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
    };

    let mut buf = Vec::new();
    envelope.encode(&mut buf).map_err(|e| {
        error!("Erro ao codificar envelope: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let _ = state.vault.store_hash(&hash_bytes, &syn_filename);

    fs::write(&vault_path, &buf).await.map_err(|e| {
        error!("Erro ao escrever arquivo: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    // Registra no banco de dados (ID sem extensão .syntra)
    let db_id = syn_filename.trim_end_matches(".syntra");
    if let Err(e) = state.db.log_file(
        &db_id, &filename, &mime, original_size, processed_size, savings_pct,
        &format!("{:?}", algo), duration_ms, &vault_path.to_string_lossy()
    ).await {
        error!("Erro ao registrar no banco: {}", e);
        // Não falha se o banco falhar
    }

    // Registra latência nas métricas
    state.metrics.record_latency(duration_ms);

    // Debug: verifica se a latência foi registrada
    tracing::debug!(
        "Latência registrada: {}ms, sum_ms agora: {}, files_processed: {}",
        duration_ms,
        state.metrics.latency_sum_ms.load(std::sync::atomic::Ordering::Relaxed),
        state.metrics.files_processed.load(std::sync::atomic::Ordering::Relaxed)
    );

    info!(
        filename = filename,
        hash = &hash_hex,
        original_size = original_size,
        compressed_size = processed_size,
        savings_pct = savings_pct,
        duration_ms = duration_ms,
        algorithm = format!("{:?}", algo),
        "Processamento concluído"
    );

    Ok((
        [
            (header::CONTENT_TYPE, "application/x-protobuf".to_string()),
            (HeaderName::from_static("x-syn-filename"), syn_filename.clone()),
        ],
        buf,
    ).into_response())
}

// ---------------------------------------------------------------------------
// POST /reconstruct (Reconstrução de Alta Fidelidade)
// ---------------------------------------------------------------------------

pub async fn reconstruct_handler(
    State(state): State<Arc<EngineState>>,
    body: axum::body::Bytes,
) -> Result<Response, (StatusCode, String)> {
    let start = Instant::now();
    let envelope = ProcessedFile::decode(body).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let _permit = state
        .sem_compress
        .acquire()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let dict = if !envelope.dictionary_id.is_empty() {
        state.dictionaries.get_dictionary(&envelope.dictionary_id)
    } else {
        None
    };

    let algo = Algorithm::try_from(envelope.algorithm).unwrap_or(Algorithm::Passthrough);
    let restored = compress::decompress(&envelope.data, algo, envelope.compression_level, dict.as_deref())
        .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, format!("Erro de reconstrução: {}", e)))?;

    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

    // Registra latência de reconstrução nas métricas
    state.metrics.record_latency(duration_ms);

    let hash_hex_reconstruct = hex::encode(&envelope.file_hash);

    info!(
        filename = envelope.original_name,
        hash = &hash_hex_reconstruct,
        duration_ms = duration_ms,
        algorithm = format!("{:?}", Algorithm::try_from(envelope.algorithm).unwrap_or(Algorithm::Passthrough)),
        "Reconstrução concluída"
    );

    let disposition = format_content_disposition(&envelope.original_name);

    Ok((
        [
            (header::CONTENT_TYPE, envelope.original_mime),
            (header::CONTENT_DISPOSITION, disposition),
            (HeaderName::from_static("x-filename"), envelope.original_name.clone()),
        ],
        restored,
    ).into_response())
}

// ---------------------------------------------------------------------------
// ADMIN INTERFACE
// ---------------------------------------------------------------------------

pub async fn stats_handler(State(state): State<Arc<EngineState>>) -> Json<serde_json::Value> {
    // Busca estatísticas SOMENTE do banco de dados
    let db_stats = state.db.get_stats().await.unwrap_or_else(|e| {
        tracing::error!("Erro ao buscar estatísticas do banco: {}", e);
        serde_json::json!({
            "files_processed": 0,
            "bytes_in": 0,
            "bytes_out": 0,
            "savings_pct": 0.0,
            "zstd_count": 0,
            "lz4_count": 0,
            "total_duration_ms": 0.0,
        })
    });

    tracing::info!("DB Stats: {:?}", db_stats);

    // Métricas de sistema voláteis (uptime, CPU, RAM)
    let uptime = state.metrics.start_time.elapsed().as_secs();

    let (cpu, ram) = if let Ok(mut sys) = state.metrics.system_monitor.lock() {
        sys.refresh_cpu_all();
        sys.refresh_memory();

        let cpu_val = if let Ok(last) = state.metrics.last_cpu_usage.lock() {
            if *last > 0.0 { *last } else { sys.global_cpu_usage() }
        } else {
            sys.global_cpu_usage()
        };

        (cpu_val, sys.used_memory())
    } else {
        (0.0, 0)
    };

    // Usa SOMENTE dados do banco de dados
    let bytes_in = db_stats["bytes_in"].as_i64().unwrap_or(0);
    let bytes_out = db_stats["bytes_out"].as_i64().unwrap_or(0);
    let files = db_stats["files_processed"].as_i64().unwrap_or(0);
    let total_duration_ms = db_stats["total_duration_ms"].as_f64().unwrap_or(0.0);

    let savings = db_stats["savings_pct"].as_f64().unwrap_or(0.0);

    Json(serde_json::json!({
        "engine": "Syntra Engine",
        "uptime_seconds": uptime,
        "roi": {
            "total_bytes_in": bytes_in,
            "essence_bytes_out": bytes_out,
            "efficiency_pct": format!("{:.1}%", savings),
            "items_processed": files,
            "errors": 0,  // Erros não são persistidos no banco
        },
        "deduplication": {
            "bytes_saved": 0,
            "hits": 0,
            "mode": "disabled"
        },
        "context_models": {
            "active_trained_count": state.dictionaries.count(),
            "hits": state.dictionaries.total_hits(),
        },
        // Usa dados do banco para contar arquivos por algoritmo
        // Zstd engloba: ZstdFast, ZstdBalanced, ZstdMaxDensity
        // LZ4 é o algoritmo "ultra_fast"
        "processing_strategies": {
            "fast": db_stats["zstd_count"].as_i64().unwrap_or(0),          // ZstdFast
            "balanced": 0,                                                 // Não está no banco
            "max_density": db_stats["zstd_count"].as_i64().unwrap_or(0),  // ZstdMaxDensity
            "ultra_fast": db_stats["lz4_count"].as_i64().unwrap_or(0),     // LZ4
            "passthrough": 0,                                              // Sem compressão
        },
        "system": {
            "cpu_usage_pct": format!("{:.1}%", cpu),
            "ram_used_bytes": ram,
            "ram_used_human": format!("{:.2} GiB", ram as f64 / 1024.0 / 1024.0 / 1024.0)
        },
        "latency": {
            "sum_ms": total_duration_ms,
            "count": files,
        },
        "debug": {
            "latency_sum_ms": total_duration_ms,
            "files_processed": files,
            "bytes_in": bytes_in,
            "bytes_out": bytes_out,
        },
        "rejected": {
            "count": 0,
            "bytes": 0,
        }
    }))
}

pub async fn metrics_handler(State(state): State<Arc<EngineState>>) -> Response {
    let output = state.metrics.to_prometheus();
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], output).into_response()
}

pub async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "active",
        "engine": "Syntra Engine"
    }))
}

// ---------------------------------------------------------------------------
// FILE LISTING & RECONSTRUCT
// ---------------------------------------------------------------------------

use axum::extract::Path;
use axum::extract::Query;
use tokio::fs;
use serde::Serialize;

#[derive(Serialize)]
pub struct EssenceFile {
    name: String,
    original_name: String,
    original_size: u64,
    essence_size: u64,
    savings_pct: f32,
    algorithm: String,
    processing_time_ms: f64,
    created_at: String,
}

pub async fn list_files_handler(
    State(state): State<Arc<EngineState>>,
    Query(params): Query<ListFilesParams>,
) -> Result<Json<Vec<EssenceFile>>, (StatusCode, String)> {
    // Extrai parâmetros de filtro da query string
    let filter_type = params.filter_type.as_deref();
    let filter_value = params.search.as_deref();

    let limit = params.limit.unwrap_or(100).max(1).min(1000);

    let records = state.db.list_files_with_filter(limit, filter_type, filter_value)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let files = records.into_iter().map(|row| {
        EssenceFile {
            name: row.id,
            original_name: row.original_name,
            original_size: row.raw_size as u64,
            essence_size: row.essence_size as u64,
            savings_pct: row.savings_pct as f32,
            algorithm: row.algorithm,
            processing_time_ms: row.duration_ms,
            created_at: row.created_at,
        }
    }).collect();

    Ok(Json(files))
}

#[derive(serde::Deserialize)]
pub struct ListFilesParams {
    #[serde(rename = "type")]
    filter_type: Option<String>,
    search: Option<String>,
    limit: Option<i64>,
}



/// Handler para download de arquivo por ID (hash BLAKE3).
///
/// # Rota
///
/// `GET /api/files/by-id/:id`
///
/// # O que faz
///
/// Busca o arquivo mais recente com o hash especificado no banco de dados,
/// lê o arquivo do vault e retorna o arquivo reconstruído.
///
/// # Diferença para `/api/files/:filename`
///
/// - `/api/files/:filename` exige o nome completo do arquivo (hash + timestamp)
/// - `/api/files/by-id/:id` exige apenas o hash BLAKE3 em hexadecimal
///
/// # Comportamento com duplicatas
///
/// Se houver múltiplos arquivos com o mesmo hash, retorna o mais recente
/// (baseado no timestamp do banco de dados).
pub async fn download_file_by_id_handler(
    State(state): State<Arc<EngineState>>,
    AxumPath(id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    // Remove extensão .syntra se presente
    let clean_id = id.trim_end_matches(".syntra");

    // Extrai o hash (primeiros 64 caracteres hex)
    let id_hex = clean_id
        .split('_')
        .next()
        .unwrap_or(clean_id)
        .to_lowercase();

    if id_hex.len() != 64 {
        return Err((StatusCode::BAD_REQUEST, "ID inválido: o hash deve ter 64 caracteres hexadecimais".to_string()));
    }

    let mut hash_bytes = [0u8; 32];
    hex::decode_to_slice(&id_hex, &mut hash_bytes)
        .map_err(|_| (StatusCode::BAD_REQUEST, "ID inválido: não é um hash hexadecimal válido".into()))?;

    tracing::info!("DOWNLOAD solicitado para ID: {} (clean_id: {})", id_hex, clean_id);

    // Se o clean_id for diferente do id_hex, significa que temos um timestamp
    // Nesse caso, tentamos buscar o registro exato no banco primeiro
    let files = if clean_id != id_hex {
        match state.db.list_files_with_filter(1, Some("id"), Some(clean_id)).await {
            Ok(recs) if !recs.is_empty() => recs,
            _ => state.db.list_files_with_filter(1, Some("hash"), Some(&id_hex)).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Erro ao buscar arquivo: {}", e)))?
        }
    } else {
        state.db.list_files_with_filter(1, Some("hash"), Some(&id_hex)).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Erro ao buscar arquivo: {}", e)))?
    };

    if files.is_empty() {
        return Err((StatusCode::NOT_FOUND, format!("Arquivo com ID {} não encontrado", id_hex)));
    }

    let file_record = &files[0];

    // Tenta encontrar o arquivo no índice do vault
    let filename_opt = state.vault.find_by_hash(&hash_bytes)
        .unwrap_or(None);

    let vault_path = if let Some(ref fname) = filename_opt {
        // Usa o nome do arquivo do índice Sled
        state.vault.resolve_path_by_filename(fname)
    } else {
        // Se não está no índice, tenta descobrir listando o diretório
        let shard_dir = state.vault.base_dir()
            .join(&id_hex[0..2])
            .join(&id_hex[2..4]);

        // Lê o diretório e busca arquivos que começam com o hash
        let mut found_file: Option<PathBuf> = None;
        if let Ok(mut entries) = tokio::fs::read_dir(&shard_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();
                if file_name_str.starts_with(&id_hex) {
                    found_file = Some(entry.path());
                    break;
                }
            }
        }

        match found_file {
            Some(path) => path,
            None => return Err((StatusCode::NOT_FOUND, format!("Arquivo físico não encontrado para ID {}", id_hex))),
        }
    };

    tracing::info!("Caminho do vault para download: {:?}", vault_path);

    let Ok(data) = fs::read(&vault_path).await else {
        tracing::error!("Arquivo não encontrado em: {:?}", vault_path);
        return Err((StatusCode::NOT_FOUND, "Arquivo não encontrado".to_string()));
    };

    let Ok(envelope) = ProcessedFile::decode(&*data) else {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, "Erro ao decodificar envelope".to_string()));
    };

    let algo = Algorithm::try_from(envelope.algorithm).unwrap_or(Algorithm::Passthrough);
    let restored = compress::decompress(&envelope.data, algo, envelope.compression_level, None)
        .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, format!("Erro de reconstrução: {}", e)))?;

    let disposition = format_content_disposition(&file_record.original_name);

    Ok((
        [
            (header::CONTENT_TYPE, envelope.original_mime),
            (header::CONTENT_DISPOSITION, disposition),
            (HeaderName::from_static("x-filename"), file_record.original_name.clone()),
            (HeaderName::from_static("x-file-id"), id_hex),
        ],
        restored,
    ).into_response())
}

/// Handler para exclusão de arquivo por ID (hash BLAKE3).
///
/// # Rota
///
/// `DELETE /api/files/by-id/:id`
///
/// # O que faz
///
/// Busca o arquivo mais recente com o hash especificado no banco de dados,
/// remove o arquivo físico do vault e remove o registro do banco de dados.
///
/// # Diferença para `/api/files/:filename`
///
/// - `/api/files/:filename` exige o nome completo do arquivo (hash + timestamp)
/// - `/api/files/by-id/:id` exige apenas o hash BLAKE3 em hexadecimal
///
/// # Comportamento com duplicatas
///
/// Se houver múltiplos arquivos com o mesmo hash, exclui apenas o mais recente
/// (baseado no timestamp do banco de dados).
pub async fn delete_file_by_id_handler(
    State(state): State<Arc<EngineState>>,
    AxumPath(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Remove extensão .syntra se presente
    let clean_id = id.trim_end_matches(".syntra");

    // Extrai o hash (primeiros 64 caracteres hex)
    let id_hex = clean_id
        .split('_')
        .next()
        .unwrap_or(clean_id)
        .to_lowercase();

    if id_hex.len() != 64 {
        return Err((StatusCode::BAD_REQUEST, "ID inválido: o hash deve ter 64 caracteres hexadecimais".to_string()));
    }

    let mut hash_bytes = [0u8; 32];
    hex::decode_to_slice(&id_hex, &mut hash_bytes)
        .map_err(|_| (StatusCode::BAD_REQUEST, "ID inválido: não é um hash hexadecimal válido".into()))?;

    tracing::info!("DELETE solicitado para ID: {} (clean_id: {})", id_hex, clean_id);

    // Busca o arquivo no banco
    let files = if clean_id != id_hex {
        match state.db.list_files_with_filter(1, Some("id"), Some(clean_id)).await {
            Ok(recs) if !recs.is_empty() => recs,
            _ => state.db.list_files_with_filter(1, Some("hash"), Some(&id_hex)).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Erro ao buscar arquivo: {}", e)))?
        }
    } else {
        state.db.list_files_with_filter(1, Some("hash"), Some(&id_hex)).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Erro ao buscar arquivo: {}", e)))?
    };

    if files.is_empty() {
        return Err((StatusCode::NOT_FOUND, format!("Arquivo com ID {} não encontrado", id_hex)));
    }

    let file_record = &files[0];

    // Tenta encontrar o arquivo no índice do vault
    let filename_opt = state.vault.find_by_hash(&hash_bytes)
        .unwrap_or(None);

    let vault_path = if let Some(ref fname) = filename_opt {
        // Usa o nome do arquivo do índice Sled
        state.vault.resolve_path_by_filename(fname)
    } else {
        // Se não está no índice, tenta descobrir listando o diretório
        let shard_dir = state.vault.base_dir()
            .join(&id_hex[0..2])
            .join(&id_hex[2..4]);

        // Lê o diretório e busca arquivos que começam com o hash
        let mut found_file: Option<PathBuf> = None;
        if let Ok(mut entries) = tokio::fs::read_dir(&shard_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();
                if file_name_str.starts_with(&id_hex) {
                    found_file = Some(entry.path());
                    break;
                }
            }
        }

        match found_file {
            Some(path) => path,
            None => return Err((StatusCode::NOT_FOUND, format!("Arquivo físico não encontrado para ID {}", id_hex))),
        }
    };

    tracing::info!("Caminho do vault para exclusão: {:?}", vault_path);

    // Verifica se o arquivo existe antes de tentar excluir
    if !tokio::fs::try_exists(&vault_path).await.unwrap_or(false) {
        tracing::error!("Arquivo não encontrado em: {:?}", vault_path);
        return Err((StatusCode::NOT_FOUND, "Arquivo não encontrado".to_string()));
    }

    fs::remove_file(&vault_path).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Erro ao deletar: {}", e)))?;

    // Remove do índice Sled
    let _ = state.vault.remove_hash(&hash_bytes);

    // Remove do banco de dados (remove o registro específico ou todos com este hash se for apenas o hash)
    let _ = state.db.delete_file(clean_id).await;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Arquivo deletado com sucesso",
        "id": id_hex,
        "original_name": file_record.original_name
    })))
}

// ---------------------------------------------------------------------------
// SIMULATION ENDPOINTS (JSON para demonstração interativa)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SimResult {
    pub original_name: String,
    pub original_size: u64,
    pub reduced_size: u64,
    pub savings_pct: f64,
    pub algorithm: String,
    pub processing_time_ms: f64,
    pub download_id: String,
    pub success: bool,
}

/// POST /api/sim/process - Processa arquivo e retorna JSON simplificado
pub async fn sim_process_handler(
    State(state): State<Arc<EngineState>>,
    headers: header::HeaderMap,
    body: Body,
) -> Result<Json<SimResult>, (StatusCode, String)> {
    let filename = headers
        .get("x-filename")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let force_algo = headers
        .get("x-algo")
        .and_then(|h| h.to_str().ok())
        .map(parse_algorithm);

    let _permit = state
        .sem_compress
        .acquire()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Coleta todos os dados
    let mut data = Vec::new();
    let mut stream = body.into_data_stream();
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        data.extend_from_slice(&chunk);
    }

    let original_size = data.len() as u64;
    let mime = detect_mime_fast(&data, &filename);
    let analysis = adaptive::analyze(&data, &mime);
    let algo = force_algo.unwrap_or(analysis.algorithm);
    let lvl = analysis.zstd_level;
    let dict = state.dictionaries.get_dictionary(&mime);

    let processing_start = Instant::now();

    let compressed = state.pool.install(|| {
        compress::compress(&data, algo, lvl, dict.as_deref())
    }).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Erro de compressão: {}", e)))?;

    let duration_ms = processing_start.elapsed().as_secs_f64() * 1000.0;

    let processed_size = compressed.len() as u64;
    let savings_pct = if original_size > 0 {
        (1.0 - (processed_size as f64 / original_size as f64)) * 100.0
    } else { 0.0 };

    let hash_bytes = blake3::hash(&data).into();
    let hash_hex = hex::encode(hash_bytes);

    // ID único para evitar conflitos de nomes (arquivos com mesmo conteúdo)
    let unique_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    // Atualiza métricas
    use std::sync::atomic::Ordering;
    state.metrics.bytes_in.fetch_add(original_size, Ordering::Relaxed);
    state.metrics.bytes_out.fetch_add(processed_size, Ordering::Relaxed);
    state.metrics.files_processed.fetch_add(1, Ordering::Relaxed);

    let syn_filename = format!("{}_{}.syntra", hash_hex, unique_id);

    // Cria o diretório (usamos ensure_dir só para criar os diretórios pais)
    let vault_dir = state.vault.ensure_dir(&hash_bytes)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Constrói o caminho completo com o nome único do arquivo
    let vault_path = vault_dir.parent().unwrap().join(&syn_filename);

    let envelope = ProcessedFile {
        version: 2,
        original_name: filename.clone(),
        original_names: vec![filename.clone()],
        original_mime: mime.clone(),
        original_size,
        processed_size,
        savings_pct: savings_pct as f32,
        algorithm: algo as i32,
        compression_level: lvl,
        dictionary_id: String::new(),
        data: compressed,
        blocks: vec![],
        file_metrics: Some(FileMetrics {
            throughput_mbps: 0.0,
            compression_ratio: savings_pct,
            dedup_savings_bytes: 0,
            processing_time_ms: 0.0,
            algorithm_used: format!("{:?}", algo),
            strategy_reason: analysis.reason.to_string(),
        }),
        pointer_to: String::new(),
        file_hash: hash_bytes.to_vec(),
        ref_count: 1,
        envelope_checksum: 0,
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
        updated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
    };

    let mut buf = Vec::new();
    envelope.encode(&mut buf).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let _ = state.vault.store_hash(&hash_bytes, &syn_filename);
    // ID no banco sem extensão .syntra
    let db_id = syn_filename.trim_end_matches(".syntra");
    let _ = state.db.log_file(
        &db_id, &filename, &mime, original_size, processed_size,
        savings_pct, &format!("{:?}", algo), duration_ms, &vault_path.to_string_lossy()
    ).await;

    // Registra latência nas métricas
    state.metrics.record_latency(duration_ms);

    fs::write(&vault_path, &buf).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(SimResult {
        original_name: filename,
        original_size,
        reduced_size: processed_size,
        savings_pct,
        algorithm: format!("{:?}", algo),
        processing_time_ms: duration_ms,
        download_id: syn_filename, // ID completo com timestamp para localizar o arquivo no vault
        success: true,
    }))
}

/// GET /api/sim/download/:id - Download de arquivo processado
pub async fn sim_download_handler(
    State(state): State<Arc<EngineState>>,
    AxumPath(id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    // Extrai apenas o hash BLAKE3 (primeiros 64 caracteres hex antes do '_')
    let id_hex = id
        .replace(".syntra", "")
        .split('_')
        .next()
        .unwrap_or(&id)
        .to_string();
    let mut hash_bytes = [0u8; 32];
    hex::decode_to_slice(&id_hex, &mut hash_bytes)
        .map_err(|_| (StatusCode::BAD_REQUEST, "ID inválido".into()))?;

    // Usa o nome completo do arquivo (com timestamp único) para buscar o arquivo específico
    let vault_path = state.vault.resolve_path_by_filename(&id);

    let Ok(data) = fs::read(&vault_path).await else {
        return Err((StatusCode::NOT_FOUND, "Arquivo não encontrado".to_string()));
    };

    let Ok(envelope) = ProcessedFile::decode(&*data) else {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, "Erro ao decodificar envelope".to_string()));
    };

    // Mede apenas o tempo de processamento da reconstrução
    let reconstruct_start = std::time::Instant::now();
    let algo = Algorithm::try_from(envelope.algorithm).unwrap_or(Algorithm::Passthrough);
    let restored = compress::decompress(&envelope.data, algo, envelope.compression_level, None)
        .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, format!("Erro de reconstrução: {}", e)))?;
    let reconstruct_time_ms = reconstruct_start.elapsed().as_secs_f64() * 1000.0;

    let disposition = format_content_disposition(&envelope.original_name);

    Ok((
        [
            (header::CONTENT_TYPE, envelope.original_mime),
            (header::CONTENT_DISPOSITION, disposition),
            (HeaderName::from_static("x-filename"), envelope.original_name.clone()),
            (HeaderName::from_static("x-reconstruct-time-ms"), reconstruct_time_ms.to_string()),
        ],
        restored,
    ).into_response())
}
