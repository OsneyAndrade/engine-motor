use anyhow::Result;
use sqlx::{sqlite::SqlitePool, postgres::PgPool};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbType {=
    Sqlite,
    Postgres,
}

pub enum MonitorDB {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

impl MonitorDB {

    pub async fn new(db_url: &str) -> Result<Self> {
        if db_url.starts_with("sqlite://") {
            let path = db_url.trim_start_matches("sqlite://");

            if !std::path::Path::new(path).exists() {
                tokio::fs::File::create(path).await?;
            }

            let pool = SqlitePool::connect(db_url).await?;

            init_sqlite(&pool).await?;

            Ok(MonitorDB::Sqlite(pool))

        } else if db_url.starts_with("postgres://") || db_url.starts_with("postgresql://") {

            let pool = PgPool::connect(db_url).await?;

            init_postgres(&pool).await?;

            Ok(MonitorDB::Postgres(pool))

        } else {
            anyhow::bail!(
                "URL de banco não suportada: {}. Use sqlite:// ou postgres://",
                db_url
            );
        }
    }

    pub fn db_type(&self) -> DbType {
        match self {
            MonitorDB::Sqlite(_) => DbType::Sqlite,
            MonitorDB::Postgres(_) => DbType::Postgres,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn log_file(
        &self,
        hash_hex: &str,
        name: &str,
        mime: &str,
        raw_size: u64,
        essence_size: u64,
        savings_pct: f64,
        algo: &str,
        duration_ms: f64,
        vault_path: &str
    ) -> Result<()> {
        match self {
            MonitorDB::Sqlite(pool) => {
                sqlx::query(
                    "INSERT INTO processed_files
                    (id, original_name, mime, raw_size, essence_size, savings_pct, algorithm, duration_ms, vault_path)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(hash_hex)
                .bind(name)
                .bind(mime)
                .bind(raw_size as i64)
                .bind(essence_size as i64)
                .bind(savings_pct)
                .bind(algo)
                .bind(duration_ms)
                .bind(vault_path)
                .execute(pool)
                .await?;
                Ok(())
            }
            MonitorDB::Postgres(pool) => {
                // PostgreSQL usa $1, $2, etc como placeholder de parâmetro
                sqlx::query(
                    "INSERT INTO processed_files
                    (id, original_name, mime, raw_size, essence_size, savings_pct, algorithm, duration_ms, vault_path)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
                )
                .bind(hash_hex)
                .bind(name)
                .bind(mime)
                .bind(raw_size as i64)
                .bind(essence_size as i64)
                .bind(savings_pct)
                .bind(algo)
                .bind(duration_ms)
                .bind(vault_path)
                .execute(pool)
                .await?;
                Ok(())
            }
        }
    }

    pub async fn record_snapshot(
        &self,
        total_in: u64,
        total_out: u64,
        files_count: u64,
        cpu: f64,
        ram: u64
    ) -> Result<()> {
        match self {
            MonitorDB::Sqlite(pool) => {
                sqlx::query(
                    "INSERT INTO system_snapshots (total_in, essence_out, files_count, cpu_pct, ram_bytes)
                    VALUES (?, ?, ?, ?, ?)"
                )
                .bind(total_in as i64)
                .bind(total_out as i64)
                .bind(files_count as i64)
                .bind(cpu)
                .bind(ram as i64)
                .execute(pool)
                .await?;
                Ok(())
            }
            MonitorDB::Postgres(pool) => {
                sqlx::query(
                    "INSERT INTO system_snapshots (total_in, essence_out, files_count, cpu_pct, ram_bytes)
                    VALUES ($1, $2, $3, $4, $5)"
                )
                .bind(total_in as i64)
                .bind(total_out as i64)
                .bind(files_count as i64)
                .bind(cpu)
                .bind(ram as i64)
                .execute(pool)
                .await?;
                Ok(())
            }
        }
    }

    pub async fn list_files(
        &self,
        limit: i64,
    ) -> Result<Vec<FileRecord>> {
        self.list_files_with_filter(limit, None, None).await
    }

    pub async fn list_files_with_filter(
        &self,
        limit: i64,
        filter_type: Option<&str>,
        filter_value: Option<&str>,
    ) -> Result<Vec<FileRecord>> {
        match self {
            MonitorDB::Sqlite(pool) => {
                let (query, bind_value) = build_sqlite_query(filter_type, filter_value, limit);
                let mut sql_query = sqlx::query_as::<_, FileRecord>(&query);
                if let Some(val) = bind_value {
                    sql_query = sql_query.bind(val);
                }

                let rows = sql_query.fetch_all(pool).await?;
                Ok(rows)
            }
            MonitorDB::Postgres(pool) => {
                let (query, bind_value, limit_value) = build_postgres_query(filter_type, filter_value, limit);

                tracing::warn!("PostgreSQL query: {}", query);
                tracing::warn!("Bind value: {:?}, Limit: {}", bind_value, limit_value);

                let mut sql_query = sqlx::query_as::<_, FileRecord>(&query);

                if let Some(val) = bind_value {
                    tracing::warn!("Binding filter: {}", val);
                    tracing::warn!("Then binding limit: {}", limit_value);
                    sql_query = sql_query.bind(val).bind(limit_value as i64);
                } else {
                    tracing::warn!("Binding only limit: {}", limit_value);
                    sql_query = sql_query.bind(limit_value as i64);
                }

                let rows = sql_query.fetch_all(pool).await?;
                Ok(rows)
            }
        }
    }

    pub async fn get_file_by_hash_and_timestamp(&self, id_hex: &str, created_at: &str) -> Result<Option<FileRecord>> {
        match self {
            MonitorDB::Sqlite(pool) => {
                let row = sqlx::query_as::<_, FileRecord>(
                    "SELECT
                        id,
                        original_name,
                        raw_size,
                        essence_size,
                        savings_pct,
                        algorithm,
                        duration_ms,
                        strftime('%s', timestamp) as created_at
                    FROM processed_files
                    WHERE id = ? AND created_at = datetime(?1, 'unixepoch')
                    LIMIT 1"
                )
                .bind(id_hex)
                .bind(created_at)
                .fetch_optional(pool)
                .await?;
                Ok(row)
            }
            MonitorDB::Postgres(pool) => {
                let row = sqlx::query_as::<_, FileRecord>(
                    "SELECT
                        id,
                        original_name,
                        raw_size,
                        essence_size,
                        savings_pct,
                        algorithm,
                        duration_ms,
                        CAST(EXTRACT(EPOCH FROM timestamp) AS TEXT) as created_at
                    FROM processed_files
                    WHERE id = $1 AND CAST(EXTRACT(EPOCH FROM timestamp) AS TEXT) = $2
                    LIMIT 1"
                )
                .bind(id_hex)
                .bind(created_at)
                .fetch_optional(pool)
                .await?;
                Ok(row)
            }
        }
    }

    pub async fn delete_file(&self, id_hex: &str) -> Result<()> {
        match self {
            MonitorDB::Sqlite(pool) => {
                sqlx::query("DELETE FROM processed_files WHERE id = ?")
                    .bind(id_hex)
                    .execute(pool)
                    .await?;
                Ok(())
            }
            MonitorDB::Postgres(pool) => {
                sqlx::query("DELETE FROM processed_files WHERE id = $1")
                    .bind(id_hex)
                    .execute(pool)
                    .await?;
                Ok(())
            }
        }
    }

    pub async fn get_stats(&self) -> Result<serde_json::Value> {
        match self {
            MonitorDB::Sqlite(pool) => {
                let row: (i64, i64, i64, i64, i64, f64) = sqlx::query_as(
                    "SELECT
                        COUNT(*) as count,
                        COALESCE(CAST(SUM(raw_size) AS INTEGER), 0) as total_in,
                        COALESCE(CAST(SUM(essence_size) AS INTEGER), 0) as total_out,
                        COUNT(CASE WHEN algorithm LIKE '%Zstd%' THEN 1 END) as compressed_count,
                        COUNT(CASE WHEN algorithm = 'Lz4' THEN 1 END) as lz4_count,
                        COALESCE(SUM(duration_ms), 0.0) as total_duration_ms
                    FROM processed_files"
                )
                .fetch_one(pool)
                .await?;

                let (count, total_in, total_out, zstd_count, lz4_count, total_duration_ms) = row;

                let savings = if total_in > 0 {
                    (1.0 - (total_out as f64 / total_in as f64)) * 100.0
                } else {
                    0.0
                };

                Ok(serde_json::json!({
                    "files_processed": count,
                    "bytes_in": total_in,
                    "bytes_out": total_out,
                    "savings_pct": savings,
                    "zstd_count": zstd_count,
                    "lz4_count": lz4_count,
                    "total_duration_ms": total_duration_ms,
                }))
            }
            MonitorDB::Postgres(pool) => {
                tracing::warn!("Buscando estatísticas no PostgreSQL...");

                // Consulta simples sem cast complexo
                let count_row: (i64,) = match sqlx::query_as("SELECT COUNT(*) FROM processed_files")
                    .fetch_one(pool)
                    .await {
                    Ok(r) => {
                        tracing::warn!("Count result: {:?}", r);
                        r
                    },
                    Err(e) => {
                        tracing::error!("Erro no COUNT: {:?}", e);
                        return Err(e.into());
                    }
                };

                let bytes_row: (Option<i64>, Option<i64>) = match sqlx::query_as(
                    "SELECT CAST(COALESCE(SUM(raw_size), 0) AS BIGINT), CAST(COALESCE(SUM(essence_size), 0) AS BIGINT) FROM processed_files"
                )
                .fetch_one(pool)
                .await {
                    Ok(r) => {
                        tracing::warn!("Bytes result: {:?}", r);
                        r
                    },
                    Err(e) => {
                        tracing::error!("Erro no SUM: {:?}", e);
                        return Err(e.into());
                    }
                };

                let lz_row: (i64,) = match sqlx::query_as(
                    "SELECT COUNT(*) FROM processed_files WHERE algorithm = 'Lz4'"
                )
                .fetch_one(pool)
                .await {
                    Ok(r) => {
                        tracing::warn!("LZ4 result: {:?}", r);
                        r
                    },
                    Err(e) => {
                        tracing::error!("Erro no LZ4: {:?}", e);
                        return Err(e.into());
                    }
                };

                let zstd_row: (i64,) = match sqlx::query_as(
                    "SELECT COUNT(*) FROM processed_files WHERE algorithm LIKE '%Zstd%'"
                )
                .fetch_one(pool)
                .await {
                    Ok(r) => {
                        tracing::warn!("Zstd result: {:?}", r);
                        r
                    },
                    Err(e) => {
                        tracing::error!("Erro no Zstd: {:?}", e);
                        return Err(e.into());
                    }
                };

                // Busca soma de duration_ms
                let duration_row: (Option<f64>,) = match sqlx::query_as(
                    "SELECT COALESCE(CAST(SUM(duration_ms) AS DOUBLE PRECISION), 0.0) FROM processed_files"
                )
                .fetch_one(pool)
                .await {
                    Ok(r) => {
                        tracing::warn!("Duration result: {:?}", r);
                        r
                    },
                    Err(e) => {
                        tracing::error!("Erro no SUM duration_ms: {:?}", e);
                        return Err(e.into());
                    }
                };

                let count = count_row.0;
                let total_in = bytes_row.0.unwrap_or(0);
                let total_out = bytes_row.1.unwrap_or(0);
                let lz4_count = lz_row.0;
                let zstd_count = zstd_row.0;
                let total_duration_ms = duration_row.0.unwrap_or(0.0);

                let savings = if total_in > 0 {
                    (1.0 - (total_out as f64 / total_in as f64)) * 100.0
                } else {
                    0.0
                };

                tracing::warn!("Stats finais: count={}, in={}, out={}, duration_ms={}", count, total_in, total_out, total_duration_ms);

                Ok(serde_json::json!({
                    "files_processed": count,
                    "bytes_in": total_in,
                    "bytes_out": total_out,
                    "savings_pct": savings,
                    "zstd_count": zstd_count,
                    "lz4_count": lz4_count,
                    "total_duration_ms": total_duration_ms,
                }))
            }
        }
    }
}

#[derive(sqlx::FromRow)]
pub struct FileRecord {
    /// Hash BLAKE3 do arquivo (hexadecimal)
    pub id: String,

    /// Nome original do arquivo
    pub original_name: String,

    /// Tamanho original em bytes
    pub raw_size: i64,

    /// Tamanho da essência em bytes
    pub essence_size: i64,

    /// Porcentagem de economia alcançada
    pub savings_pct: f64,

    /// Algoritmo de compressão usado
    pub algorithm: String,

    /// Tempo de processamento em ms
    pub duration_ms: f64,

    /// Timestamp de criação (como string Unix)
    pub created_at: String,
}

fn build_sqlite_query(filter_type: Option<&str>, filter_value: Option<&str>, limit: i64) -> (String, Option<String>) {
    // Query base que seleciona as colunas
    let base_select = "SELECT
        id,
        original_name,
        raw_size,
        essence_size,
        savings_pct,
        algorithm,
        duration_ms,
        strftime('%s', timestamp) as created_at
    FROM processed_files";

    // Condições padrão (exclui Passthrough que não comprimiu)
    let mut conditions = vec!["algorithm != 'Passthrough'"];
    let mut bind_value = None;

    // Se há filtro, adiciona a condição
    if let (Some(ft), Some(fv)) = (filter_type, filter_value) {
        if !fv.is_empty() {
            let condition = match ft {
                "hash" => {
                    bind_value = Some(format!("%{}%", fv));
                    "id LIKE ?"
                }
                "name" => {
                    bind_value = Some(format!("%{}%", fv));
                    "original_name LIKE ?"
                }
                _ => "",
            };
            if !condition.is_empty() {
                conditions.push(condition);
            }
        }
    }

    // Combina as condições com AND
    let where_clause = conditions.join(" AND ");

    // Monta a query final
    let query = format!(
        "{} WHERE {} ORDER BY timestamp DESC LIMIT {}",
        base_select, where_clause, limit
    );

    (query, bind_value)
}

fn build_postgres_query(filter_type: Option<&str>, filter_value: Option<&str>, limit: i64) -> (String, Option<String>, i64) {
    let base_select = "SELECT
        id,
        original_name,
        raw_size,
        essence_size,
        savings_pct,
        algorithm,
        duration_ms,
        CAST(EXTRACT(EPOCH FROM timestamp) AS TEXT) as created_at
    FROM processed_files";

    let mut conditions = vec!["algorithm != 'Passthrough'"];
    let mut bind_value = None;
    let mut has_filter = false;

    if let (Some(ft), Some(fv)) = (filter_type, filter_value) {
        if !fv.is_empty() {
            has_filter = true;
            let condition = match ft {
                "hash" => {
                    bind_value = Some(format!("%{}%", fv));
                    "id LIKE $1"
                }
                "name" => {
                    bind_value = Some(format!("%{}%", fv));
                    "original_name LIKE $1"
                }
                _ => "",
            };
            if !condition.is_empty() {
                conditions.push(condition);
            }
        }
    }

    let where_clause = conditions.join(" AND ");

    // Em PostgreSQL, parâmetros são numerados sequencialmente:
    // - Sem filtro: $1 é o LIMIT
    // - Com filtro: $1 é o valor do filtro, $2 é o LIMIT
    let query = if has_filter {
        format!(
            "{} WHERE {} ORDER BY timestamp DESC LIMIT $2",
            base_select, where_clause
        )
    } else {
        format!(
            "{} WHERE {} ORDER BY timestamp DESC LIMIT $1",
            base_select, where_clause
        )
    };

    (query, bind_value, limit)
}

async fn init_sqlite(pool: &SqlitePool) -> Result<()> {
    // ... (código de inicialização mantido igual)
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS processed_files (
            rowid INTEGER PRIMARY KEY AUTOINCREMENT,
            id TEXT NOT NULL,
            original_name TEXT,
            mime TEXT,
            raw_size INTEGER,
            essence_size INTEGER,
            savings_pct REAL,
            algorithm TEXT,
            duration_ms REAL,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
            vault_path TEXT
        )"
    ).execute(pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS system_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            total_in INTEGER,
            essence_out INTEGER,
            files_count INTEGER,
            cpu_pct REAL,
            ram_bytes INTEGER,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
        )"
    ).execute(pool).await?;

    // Índices para performance em buscas
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_processed_files_original_name
        ON processed_files(original_name COLLATE NOCASE)"
    ).execute(pool).await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_processed_files_id
        ON processed_files(id)"
    ).execute(pool).await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_processed_files_timestamp
        ON processed_files(timestamp DESC)"
    ).execute(pool).await?;

    Ok(())
}

async fn init_postgres(pool: &PgPool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS processed_files (
            rowid SERIAL PRIMARY KEY,
            id TEXT NOT NULL,
            original_name TEXT,
            mime TEXT,
            raw_size BIGINT,
            essence_size BIGINT,
            savings_pct DOUBLE PRECISION,
            algorithm TEXT,
            duration_ms DOUBLE PRECISION,
            timestamp TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
            vault_path TEXT
        )"
    ).execute(pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS system_snapshots (
            id SERIAL PRIMARY KEY,
            total_in BIGINT,
            essence_out BIGINT,
            files_count BIGINT,
            cpu_pct DOUBLE PRECISION,
            ram_bytes BIGINT,
            timestamp TIMESTAMP WITH TIME ZONE DEFAULT NOW()
        )"
    ).execute(pool).await?;

    // Índices para performance
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_processed_files_timestamp
        ON processed_files(timestamp DESC)"
    ).execute(pool).await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_processed_files_algorithm
        ON processed_files(algorithm)"
    ).execute(pool).await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_processed_files_original_name
        ON processed_files(original_name)"
    ).execute(pool).await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_processed_files_id
        ON processed_files(id)"
    ).execute(pool).await?;

    Ok(())
}
