use anyhow::{anyhow, Result};
use serde::Serialize;

use crate::db_monitor::{DbType, MonitorDB};
use crate::tenancy::{PlanKind, PriceConfig};
use crate::{sql_exec, sql_fetch_all, sql_fetch_optional};

const BYTES_PER_GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Compress,
    Decompress,
    Read,
    Analyze,
    Delete,
}

impl Operation {
    pub fn as_str(self) -> &'static str {
        match self {
            Operation::Compress => "compress",
            Operation::Decompress => "decompress",
            Operation::Read => "read",
            Operation::Analyze => "analyze",
            Operation::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupScope {
    None,
    Tenant,
    Global,
}

impl DedupScope {
    pub fn as_str(self) -> &'static str {
        match self {
            DedupScope::None => "none",
            DedupScope::Tenant => "tenant",
            DedupScope::Global => "global",
        }
    }
}

#[derive(Debug, Clone)]
pub struct UsageEvent {
    pub tenant_id: String,
    pub key_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub operation: Operation,
    pub object_hash: Option<String>,
    pub bytes_in: i64,
    pub bytes_out: i64,
    pub bytes_saved: i64,
    pub effort: String,
    pub codec: String,
    pub cpu_ms: f64,
    pub dedup_scope: DedupScope,
    pub request_id: Option<String>,
    pub occurred_at_ms: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StoredEvent {
    pub id: String,
    pub tenant_id: String,
    pub operation: String,
    pub object_hash: Option<String>,
    pub bytes_in: i64,
    pub bytes_out: i64,
    pub bytes_saved: i64,
    pub effort: String,
    pub codec: String,
    pub cpu_ms: f64,
    pub dedup_scope: String,
    pub occurred_at_ms: i64,
}

pub enum RecordOutcome {
    Recorded(String),
    Replayed(StoredEvent),
}

#[derive(Debug, Clone, Serialize)]
pub struct OperationBucket {
    pub operation: String,
    pub events: i64,
    pub bytes_in: i64,
    pub bytes_out: i64,
    pub bytes_saved: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EffortBucket {
    pub effort: String,
    pub events: i64,
    pub bytes_in: i64,
    pub bytes_saved: i64,
    pub cpu_ms: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageTotals {
    pub events: i64,
    pub bytes_in: i64,
    pub bytes_out: i64,
    pub bytes_saved: i64,
    pub cpu_ms: f64,
    pub dedup_tenant: i64,
    pub dedup_global: i64,
    pub by_operation: Vec<OperationBucket>,
    pub by_effort: Vec<EffortBucket>,
}

impl UsageTotals {
    pub fn compress_bytes_in(&self) -> i64 {
        self.by_operation
            .iter()
            .filter(|b| b.operation == "compress")
            .map(|b| b.bytes_in)
            .sum()
    }

    pub fn compress_bytes_saved(&self) -> i64 {
        self.by_operation
            .iter()
            .filter(|b| b.operation == "compress")
            .map(|b| b.bytes_saved)
            .sum()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct QuoteLine {
    pub label: String,
    pub quantity_gib: f64,
    pub unit_cents_per_gib: f64,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Quote {
    pub plan: String,
    pub formula: String,
    pub currency: String,
    pub amount_cents: i64,
    pub lines: Vec<QuoteLine>,
}

pub fn quote(plan: PlanKind, price: &PriceConfig, totals: &UsageTotals) -> Quote {
    let mut lines = Vec::new();

    match plan {
        PlanKind::SavingsShare => {
            let gib = totals.compress_bytes_saved() as f64 / BYTES_PER_GIB;
            let unit = price.reference_gb_month_cents * price.share_pct / 100.0;
            lines.push(QuoteLine {
                label: "armazenamento economizado".to_string(),
                quantity_gib: round4(gib),
                unit_cents_per_gib: round4(unit),
                amount_cents: (gib * unit).round() as i64,
            });
        }
        PlanKind::IngestedGb => {
            for bucket in &totals.by_effort {
                let gib = bucket.bytes_in as f64 / BYTES_PER_GIB;
                let unit = price.per_gb_cents.for_effort(&bucket.effort);
                lines.push(QuoteLine {
                    label: format!("ingestão (esforço {})", bucket.effort),
                    quantity_gib: round4(gib),
                    unit_cents_per_gib: round4(unit),
                    amount_cents: (gib * unit).round() as i64,
                });
            }
        }
    }

    let formula = match plan {
        PlanKind::SavingsShare => format!(
            "{}% de {} centavos por GiB-mês de armazenamento evitado",
            price.share_pct, price.reference_gb_month_cents
        ),
        PlanKind::IngestedGb => format!(
            "GiB ingeridos por tier de esforço (fast {}, balanced {}, max {} centavos)",
            price.per_gb_cents.fast, price.per_gb_cents.balanced, price.per_gb_cents.max
        ),
    };

    Quote {
        plan: plan.as_str().to_string(),
        formula,
        currency: price.currency.as_str().to_string(),
        amount_cents: lines.iter().map(|l| l.amount_cents).sum(),
        lines,
    }
}

fn round4(v: f64) -> f64 {
    if v.is_finite() {
        (v * 10_000.0).round() / 10_000.0
    } else {
        0.0
    }
}

pub async fn init_schema(db: &MonitorDB) -> Result<()> {
    let (int, real) = match db.db_type() {
        DbType::Sqlite => ("INTEGER", "REAL"),
        DbType::Postgres => ("BIGINT", "DOUBLE PRECISION"),
    };

    let statements = vec![
        format!(
            "CREATE TABLE IF NOT EXISTS usage_events (
                id TEXT PRIMARY KEY,
                tenant_id TEXT NOT NULL,
                key_id TEXT,
                idempotency_key TEXT,
                operation TEXT NOT NULL,
                object_hash TEXT,
                bytes_in {int} NOT NULL,
                bytes_out {int} NOT NULL,
                bytes_saved {int} NOT NULL,
                effort TEXT NOT NULL,
                codec TEXT NOT NULL,
                cpu_ms {real} NOT NULL,
                dedup_scope TEXT NOT NULL,
                request_id TEXT,
                occurred_at_ms {int} NOT NULL
            )"
        ),
        "CREATE INDEX IF NOT EXISTS idx_usage_tenant_time
         ON usage_events(tenant_id, occurred_at_ms)".to_string(),
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_usage_idempotency
         ON usage_events(tenant_id, idempotency_key)".to_string(),
    ];

    for stmt in statements {
        sql_exec!(db, stmt).map_err(|e| anyhow!("criando schema de uso: {e}"))?;
    }
    Ok(())
}

const EVENT_COLUMNS: &str =
    "id, tenant_id, operation, object_hash, bytes_in, bytes_out, bytes_saved, \
     effort, codec, cpu_ms, dedup_scope, occurred_at_ms";

pub async fn find_by_idempotency(
    db: &MonitorDB,
    tenant_id: &str,
    key: &str,
) -> Result<Option<StoredEvent>> {
    let sql = format!(
        "SELECT {EVENT_COLUMNS} FROM usage_events
         WHERE tenant_id = {} AND idempotency_key = {}",
        db.ph(1),
        db.ph(2)
    );
    Ok(sql_fetch_optional!(db, StoredEvent, sql, tenant_id.to_string(), key.to_string())?)
}

pub async fn record(db: &MonitorDB, event: &UsageEvent) -> Result<RecordOutcome> {
    if let Some(key) = &event.idempotency_key {
        if let Some(existing) = find_by_idempotency(db, &event.tenant_id, key).await? {
            return Ok(RecordOutcome::Replayed(existing));
        }
    }

    let id = uuid::Uuid::new_v4().to_string();
    let sql = format!(
        "INSERT INTO usage_events
         (id, tenant_id, key_id, idempotency_key, operation, object_hash,
          bytes_in, bytes_out, bytes_saved, effort, codec, cpu_ms,
          dedup_scope, request_id, occurred_at_ms)
         VALUES ({})",
        db.placeholders(15)
    );

    let inserted = sql_exec!(
        db,
        sql,
        id.clone(),
        event.tenant_id.clone(),
        event.key_id.clone(),
        event.idempotency_key.clone(),
        event.operation.as_str().to_string(),
        event.object_hash.clone(),
        event.bytes_in,
        event.bytes_out,
        event.bytes_saved,
        event.effort.clone(),
        event.codec.clone(),
        event.cpu_ms,
        event.dedup_scope.as_str().to_string(),
        event.request_id.clone(),
        event.occurred_at_ms,
    );

    match inserted {
        Ok(_) => Ok(RecordOutcome::Recorded(id)),
        Err(e) => {
            if let Some(key) = &event.idempotency_key {
                if let Some(existing) = find_by_idempotency(db, &event.tenant_id, key).await? {
                    return Ok(RecordOutcome::Replayed(existing));
                }
            }
            Err(anyhow!("gravando evento de uso: {e}"))
        }
    }
}

#[derive(sqlx::FromRow)]
struct TotalsRow {
    events: i64,
    bytes_in: i64,
    bytes_out: i64,
    bytes_saved: i64,
    cpu_ms: f64,
    dedup_tenant: i64,
    dedup_global: i64,
}

#[derive(sqlx::FromRow)]
struct OperationRow {
    operation: String,
    events: i64,
    bytes_in: i64,
    bytes_out: i64,
    bytes_saved: i64,
}

#[derive(sqlx::FromRow)]
struct EffortRow {
    effort: String,
    events: i64,
    bytes_in: i64,
    bytes_saved: i64,
    cpu_ms: f64,
}

pub async fn totals(
    db: &MonitorDB,
    tenant_id: &str,
    from_ms: i64,
    to_ms: i64,
) -> Result<UsageTotals> {
    let window = format!(
        "WHERE tenant_id = {} AND occurred_at_ms >= {} AND occurred_at_ms < {}",
        db.ph(1),
        db.ph(2),
        db.ph(3)
    );

    let sql = format!(
        "SELECT
            COUNT(*) AS events,
            CAST(COALESCE(SUM(bytes_in), 0) AS BIGINT) AS bytes_in,
            CAST(COALESCE(SUM(bytes_out), 0) AS BIGINT) AS bytes_out,
            CAST(COALESCE(SUM(bytes_saved), 0) AS BIGINT) AS bytes_saved,
            COALESCE(SUM(cpu_ms), 0.0) AS cpu_ms,
            CAST(COALESCE(SUM(CASE WHEN dedup_scope = 'tenant' THEN 1 ELSE 0 END), 0) AS BIGINT) AS dedup_tenant,
            CAST(COALESCE(SUM(CASE WHEN dedup_scope = 'global' THEN 1 ELSE 0 END), 0) AS BIGINT) AS dedup_global
         FROM usage_events {window}"
    );
    let row = sql_fetch_optional!(db, TotalsRow, sql, tenant_id.to_string(), from_ms, to_ms)?;

    let sql_op = format!(
        "SELECT operation,
            COUNT(*) AS events,
            CAST(COALESCE(SUM(bytes_in), 0) AS BIGINT) AS bytes_in,
            CAST(COALESCE(SUM(bytes_out), 0) AS BIGINT) AS bytes_out,
            CAST(COALESCE(SUM(bytes_saved), 0) AS BIGINT) AS bytes_saved
         FROM usage_events {window} GROUP BY operation ORDER BY operation"
    );
    let ops = sql_fetch_all!(db, OperationRow, sql_op, tenant_id.to_string(), from_ms, to_ms)?;

    let sql_effort = format!(
        "SELECT effort,
            COUNT(*) AS events,
            CAST(COALESCE(SUM(bytes_in), 0) AS BIGINT) AS bytes_in,
            CAST(COALESCE(SUM(bytes_saved), 0) AS BIGINT) AS bytes_saved,
            COALESCE(SUM(cpu_ms), 0.0) AS cpu_ms
         FROM usage_events {window} AND operation = 'compress'
         GROUP BY effort ORDER BY effort"
    );
    let efforts = sql_fetch_all!(db, EffortRow, sql_effort, tenant_id.to_string(), from_ms, to_ms)?;

    let base = row.unwrap_or(TotalsRow {
        events: 0,
        bytes_in: 0,
        bytes_out: 0,
        bytes_saved: 0,
        cpu_ms: 0.0,
        dedup_tenant: 0,
        dedup_global: 0,
    });

    Ok(UsageTotals {
        events: base.events,
        bytes_in: base.bytes_in,
        bytes_out: base.bytes_out,
        bytes_saved: base.bytes_saved,
        cpu_ms: base.cpu_ms,
        dedup_tenant: base.dedup_tenant,
        dedup_global: base.dedup_global,
        by_operation: ops
            .into_iter()
            .map(|r| OperationBucket {
                operation: r.operation,
                events: r.events,
                bytes_in: r.bytes_in,
                bytes_out: r.bytes_out,
                bytes_saved: r.bytes_saved,
            })
            .collect(),
        by_effort: efforts
            .into_iter()
            .map(|r| EffortBucket {
                effort: r.effort,
                events: r.events,
                bytes_in: r.bytes_in,
                bytes_saved: r.bytes_saved,
                cpu_ms: r.cpu_ms,
            })
            .collect(),
    })
}

pub async fn recent(
    db: &MonitorDB,
    tenant_id: &str,
    limit: i64,
) -> Result<Vec<StoredEvent>> {
    let sql = format!(
        "SELECT {EVENT_COLUMNS} FROM usage_events WHERE tenant_id = {}
         ORDER BY occurred_at_ms DESC LIMIT {}",
        db.ph(1),
        limit.clamp(1, 1000)
    );
    Ok(sql_fetch_all!(db, StoredEvent, sql, tenant_id.to_string())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tenancy::{Currency, PerEffortCents};

    fn totals_fake(compress_in: i64, compress_saved: i64, efforts: Vec<(&str, i64)>) -> UsageTotals {
        UsageTotals {
            events: 1,
            bytes_in: compress_in,
            bytes_out: compress_in - compress_saved,
            bytes_saved: compress_saved,
            cpu_ms: 10.0,
            dedup_tenant: 0,
            dedup_global: 0,
            by_operation: vec![OperationBucket {
                operation: "compress".to_string(),
                events: 1,
                bytes_in: compress_in,
                bytes_out: compress_in - compress_saved,
                bytes_saved: compress_saved,
            }],
            by_effort: efforts
                .into_iter()
                .map(|(e, b)| EffortBucket {
                    effort: e.to_string(),
                    events: 1,
                    bytes_in: b,
                    bytes_saved: 0,
                    cpu_ms: 1.0,
                })
                .collect(),
        }
    }

    #[test]
    fn plano_de_participacao_na_economia() {
        let gib = 1024 * 1024 * 1024;
        let price = PriceConfig {
            share_pct: 30.0,
            reference_gb_month_cents: 12.0,
            per_gb_cents: default_cents(),
            currency: Currency::Brl,
        };
        let t = totals_fake(10 * gib, 8 * gib, vec![("max", 10 * gib)]);
        let q = quote(PlanKind::SavingsShare, &price, &t);

        assert_eq!(q.amount_cents, (8.0f64 * 12.0 * 0.30).round() as i64);
        assert_eq!(q.lines.len(), 1);
        assert_eq!(q.lines[0].quantity_gib, 8.0);
        assert_eq!(q.currency, "BRL");
    }

    #[test]
    fn plano_por_gib_ingerido_cobra_por_tier() {
        let gib = 1024 * 1024 * 1024;
        let price = PriceConfig {
            share_pct: 0.0,
            reference_gb_month_cents: 0.0,
            per_gb_cents: PerEffortCents { fast: 100.0, balanced: 400.0, max: 1600.0 },
            currency: Currency::Usd,
        };
        let t = totals_fake(3 * gib, gib, vec![("fast", gib), ("max", 2 * gib)]);
        let q = quote(PlanKind::IngestedGb, &price, &t);

        assert_eq!(q.amount_cents, 100 + 3200);
        assert_eq!(q.lines.len(), 2);
        assert_eq!(q.currency, "USD");
    }

    #[test]
    fn leitura_nao_entra_na_fatura_dos_dois_planos() {
        let gib = 1024 * 1024 * 1024;
        let mut t = totals_fake(gib, gib / 2, vec![("balanced", gib)]);
        t.by_operation.push(OperationBucket {
            operation: "read".to_string(),
            events: 50,
            bytes_in: 0,
            bytes_out: 100 * gib,
            bytes_saved: 0,
        });

        let price = PriceConfig::default();
        let antes = quote(PlanKind::SavingsShare, &price, &t).amount_cents;
        let por_gib = quote(PlanKind::IngestedGb, &price, &t).amount_cents;

        assert!(antes > 0);
        assert!(por_gib > 0);
        assert_eq!(t.compress_bytes_saved(), gib / 2);
        assert_eq!(t.compress_bytes_in(), gib);
    }

    #[test]
    fn uso_zerado_nao_gera_cobranca() {
        let t = UsageTotals {
            events: 0,
            bytes_in: 0,
            bytes_out: 0,
            bytes_saved: 0,
            cpu_ms: 0.0,
            dedup_tenant: 0,
            dedup_global: 0,
            by_operation: vec![],
            by_effort: vec![],
        };
        let price = PriceConfig::default();
        assert_eq!(quote(PlanKind::SavingsShare, &price, &t).amount_cents, 0);
        assert_eq!(quote(PlanKind::IngestedGb, &price, &t).amount_cents, 0);
    }

    #[test]
    fn formula_e_explicita_na_cotacao() {
        let price = PriceConfig::default();
        let t = totals_fake(1024, 512, vec![("fast", 1024)]);
        assert!(quote(PlanKind::SavingsShare, &price, &t).formula.contains("%"));
        assert!(quote(PlanKind::IngestedGb, &price, &t).formula.contains("fast"));
    }

    fn default_cents() -> PerEffortCents {
        PerEffortCents { fast: 150.0, balanced: 400.0, max: 1600.0 }
    }
}
