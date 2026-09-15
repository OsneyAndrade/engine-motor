use anyhow::{Context, Result};
use sqlx::{postgres::PgPool, sqlite::SqlitePool};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbType {
    Sqlite,
    Postgres,
}

pub enum MonitorDB {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

pub enum Pool<'a> {
    Sqlite(&'a SqlitePool),
    Postgres(&'a PgPool),
}

impl MonitorDB {
    pub async fn new(db_url: &str) -> Result<Self> {
        if db_url.starts_with("sqlite:") {
            let raw = db_url
                .trim_start_matches("sqlite://")
                .trim_start_matches("sqlite:");

            if !raw.is_empty() && raw != ":memory:" {
                let path = std::path::Path::new(raw);
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent).ok();
                    }
                }
                if !path.exists() {
                    std::fs::File::create(path)
                        .with_context(|| format!("criando banco sqlite em {raw}"))?;
                }
            }

            let pool = SqlitePool::connect(db_url)
                .await
                .with_context(|| format!("conectando em {db_url}"))?;
            init_sqlite(&pool).await?;
            let db = MonitorDB::Sqlite(pool);
            crate::tenancy::init_schema(&db).await?;
            crate::usage::init_schema(&db).await?;
            Ok(db)
        } else if db_url.starts_with("postgres://") || db_url.starts_with("postgresql://") {
            let pool = PgPool::connect(db_url)
                .await
                .with_context(|| "conectando no PostgreSQL")?;
            init_postgres(&pool).await?;
            let db = MonitorDB::Postgres(pool);
            crate::tenancy::init_schema(&db).await?;
            crate::usage::init_schema(&db).await?;
            Ok(db)
        } else {
            anyhow::bail!("URL de banco não suportada: {db_url}. Use sqlite: ou postgres://")
        }
    }

    pub fn db_type(&self) -> DbType {
        match self {
            MonitorDB::Sqlite(_) => DbType::Sqlite,
            MonitorDB::Postgres(_) => DbType::Postgres,
        }
    }

    pub fn pool(&self) -> Pool<'_> {
        match self {
            MonitorDB::Sqlite(p) => Pool::Sqlite(p),
            MonitorDB::Postgres(p) => Pool::Postgres(p),
        }
    }

    pub fn ph(&self, n: usize) -> String {
        match self {
            MonitorDB::Sqlite(_) => "?".to_string(),
            MonitorDB::Postgres(_) => format!("${n}"),
        }
    }

    pub fn placeholders(&self, count: usize) -> String {
        (1..=count)
            .map(|n| self.ph(n))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub async fn ping(&self) -> Result<()> {
        match self {
            MonitorDB::Sqlite(pool) => {
                sqlx::query("SELECT 1").execute(pool).await?;
            }
            MonitorDB::Postgres(pool) => {
                sqlx::query("SELECT 1").execute(pool).await?;
            }
        }
        Ok(())
    }

    pub async fn record_snapshot(
        &self,
        total_in: u64,
        total_out: u64,
        files_count: u64,
        cpu: f64,
        ram: u64,
    ) -> Result<()> {
        const COLS: &str =
            "(total_in, essence_out, files_count, cpu_pct, ram_bytes)";
        match self {
            MonitorDB::Sqlite(pool) => {
                sqlx::query(&format!(
                    "INSERT INTO system_snapshots {COLS} VALUES (?, ?, ?, ?, ?)"
                ))
                .bind(total_in as i64)
                .bind(total_out as i64)
                .bind(files_count as i64)
                .bind(cpu)
                .bind(ram as i64)
                .execute(pool)
                .await?;
            }
            MonitorDB::Postgres(pool) => {
                sqlx::query(&format!(
                    "INSERT INTO system_snapshots {COLS} VALUES ($1, $2, $3, $4, $5)"
                ))
                .bind(total_in as i64)
                .bind(total_out as i64)
                .bind(files_count as i64)
                .bind(cpu)
                .bind(ram as i64)
                .execute(pool)
                .await?;
            }
        }
        Ok(())
    }

    pub async fn get_stats(&self) -> Result<serde_json::Value> {
        let (count, total_in, total_out, total_saved, total_cpu_ms) = match self {
            MonitorDB::Sqlite(pool) => {
                let row: (i64, i64, i64, i64, f64) = sqlx::query_as(
                    "SELECT
                        COUNT(*),
                        COALESCE(CAST(SUM(bytes_in) AS INTEGER), 0),
                        COALESCE(CAST(SUM(bytes_out) AS INTEGER), 0),
                        COALESCE(CAST(SUM(bytes_saved) AS INTEGER), 0),
                        COALESCE(SUM(cpu_ms), 0.0)
                     FROM usage_events WHERE operation = 'compress'",
                )
                .fetch_one(pool)
                .await?;
                row
            }
            MonitorDB::Postgres(pool) => {
                let row: (i64, Option<i64>, Option<i64>, Option<i64>, Option<f64>) = sqlx::query_as(
                    "SELECT
                        COUNT(*),
                        CAST(COALESCE(SUM(bytes_in), 0) AS BIGINT),
                        CAST(COALESCE(SUM(bytes_out), 0) AS BIGINT),
                        CAST(COALESCE(SUM(bytes_saved), 0) AS BIGINT),
                        CAST(COALESCE(SUM(cpu_ms), 0.0) AS DOUBLE PRECISION)
                     FROM usage_events WHERE operation = 'compress'",
                )
                .fetch_one(pool)
                .await?;
                (row.0, row.1.unwrap_or(0), row.2.unwrap_or(0), row.3.unwrap_or(0), row.4.unwrap_or(0.0))
            }
        };

        let savings = if total_in > 0 {
            (1.0 - total_out as f64 / total_in as f64) * 100.0
        } else {
            0.0
        };

        Ok(serde_json::json!({
            "files_processed": count,
            "bytes_in": total_in,
            "bytes_out": total_out,
            "bytes_saved": total_saved,
            "savings_pct": savings,
            "total_duration_ms": total_cpu_ms,
        }))
    }
}

async fn init_sqlite(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS system_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            total_in INTEGER,
            essence_out INTEGER,
            files_count INTEGER,
            cpu_pct REAL,
            ram_bytes INTEGER,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn init_postgres(pool: &PgPool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS system_snapshots (
            id BIGSERIAL PRIMARY KEY,
            total_in BIGINT,
            essence_out BIGINT,
            files_count BIGINT,
            cpu_pct DOUBLE PRECISION,
            ram_bytes BIGINT,
            timestamp TIMESTAMP WITH TIME ZONE DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sobe_sqlite_na_memoria_e_agrega_o_ledger() {
        let db = MonitorDB::new("sqlite::memory:").await.unwrap();
        assert_eq!(db.db_type(), DbType::Sqlite);
        db.ping().await.unwrap();

        let stats = db.get_stats().await.unwrap();
        assert_eq!(stats["files_processed"], 0);
        assert_eq!(stats["bytes_in"], 0);

        let tenant = crate::tenancy::TenancyStore::ensure_default_tenant(&db, 1)
            .await
            .unwrap();

        for (bytes_in, bytes_out) in [(10_000i64, 2_000i64), (5_000, 5_000)] {
            crate::usage::record(
                &db,
                &crate::usage::UsageEvent {
                    tenant_id: tenant.id.clone(),
                    key_id: None,
                    idempotency_key: None,
                    operation: crate::usage::Operation::Compress,
                    object_hash: Some("a".repeat(64)),
                    bytes_in,
                    bytes_out,
                    bytes_saved: (bytes_in - bytes_out).max(0),
                    effort: "balanced".to_string(),
                    codec: "zstd:12".to_string(),
                    cpu_ms: 5.0,
                    dedup_scope: crate::usage::DedupScope::None,
                    request_id: None,
                    occurred_at_ms: 100,
                },
            )
            .await
            .unwrap();
        }

        let stats = db.get_stats().await.unwrap();
        assert_eq!(stats["files_processed"], 2);
        assert_eq!(stats["bytes_in"], 15_000);
        assert_eq!(stats["bytes_out"], 7_000);
        assert_eq!(stats["bytes_saved"], 8_000);
    }

    #[tokio::test]
    async fn url_invalida_e_rejeitada() {
        assert!(MonitorDB::new("mysql://x").await.is_err());
        assert!(MonitorDB::new("").await.is_err());
    }
}
