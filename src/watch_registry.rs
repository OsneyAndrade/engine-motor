use anyhow::{anyhow, Result};

use crate::db_monitor::{DbType, MonitorDB};
use crate::{sql_exec, sql_fetch_all};

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct WatchRecord {
    pub path: String,
    pub size: i64,
    pub mtime_ns: Option<i64>,
    pub object_hash: Option<String>,
    pub seen_at_ms: i64,
}

pub async fn init_schema(db: &MonitorDB) -> Result<()> {
    let int = match db.db_type() {
        DbType::Sqlite => "INTEGER",
        DbType::Postgres => "BIGINT",
    };

    let sql = format!(
        "CREATE TABLE IF NOT EXISTS watch_seen (
            path TEXT PRIMARY KEY,
            size {int} NOT NULL,
            mtime_ns {int},
            object_hash TEXT,
            seen_at_ms {int} NOT NULL
        )"
    );
    sql_exec!(db, sql).map_err(|e| anyhow!("criando schema do registro de ingestão: {e}"))?;
    Ok(())
}

pub async fn load_all(db: &MonitorDB) -> Result<Vec<WatchRecord>> {
    let sql = "SELECT path, size, mtime_ns, object_hash, seen_at_ms FROM watch_seen".to_string();
    Ok(sql_fetch_all!(db, WatchRecord, sql)?)
}

pub async fn upsert(db: &MonitorDB, record: &WatchRecord) -> Result<()> {
    let sql = format!(
        "INSERT INTO watch_seen (path, size, mtime_ns, object_hash, seen_at_ms)
         VALUES ({})
         ON CONFLICT (path) DO UPDATE SET
            size = excluded.size,
            mtime_ns = excluded.mtime_ns,
            object_hash = excluded.object_hash,
            seen_at_ms = excluded.seen_at_ms",
        db.placeholders(5)
    );
    sql_exec!(
        db,
        sql,
        record.path.clone(),
        record.size,
        record.mtime_ns,
        record.object_hash.clone(),
        record.seen_at_ms,
    )
    .map_err(|e| anyhow!("gravando registro de ingestão: {e}"))?;
    Ok(())
}

pub async fn remove(db: &MonitorDB, path: &str) -> Result<()> {
    let sql = format!("DELETE FROM watch_seen WHERE path = {}", db.ph(1));
    sql_exec!(db, sql, path.to_string())
        .map_err(|e| anyhow!("removendo registro de ingestão: {e}"))?;
    Ok(())
}
