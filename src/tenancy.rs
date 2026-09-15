use std::collections::BTreeSet;
use std::fmt;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::db_monitor::{DbType, MonitorDB};
use crate::{sql_exec, sql_fetch_all, sql_fetch_optional};

pub const DEFAULT_TENANT_ID: &str = "default";
const KEY_PREFIX: &str = "syn";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Compress,
    Read,
    Delete,
    Admin,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Compress => "compress",
            Scope::Read => "read",
            Scope::Delete => "delete",
            Scope::Admin => "admin",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "compress" | "write" => Some(Scope::Compress),
            "read" => Some(Scope::Read),
            "delete" => Some(Scope::Delete),
            "admin" => Some(Scope::Admin),
            _ => None,
        }
    }

    pub fn all() -> BTreeSet<Scope> {
        [Scope::Compress, Scope::Read, Scope::Delete, Scope::Admin]
            .into_iter()
            .collect()
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn scopes_to_string(scopes: &BTreeSet<Scope>) -> String {
    scopes
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

pub fn scopes_from_string(raw: &str) -> BTreeSet<Scope> {
    raw.split(',').filter_map(Scope::parse).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanKind {
    SavingsShare,
    IngestedGb,
}

impl PlanKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PlanKind::SavingsShare => "savings_share",
            PlanKind::IngestedGb => "ingested_gb",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "savings_share" | "savings" => Some(PlanKind::SavingsShare),
            "ingested_gb" | "ingested" => Some(PlanKind::IngestedGb),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceConfig {
    #[serde(default = "default_share_pct")]
    pub share_pct: f64,
    #[serde(default = "default_reference_gb_month_cents")]
    pub reference_gb_month_cents: f64,
    #[serde(default = "default_per_gb_cents")]
    pub per_gb_cents: PerEffortCents,
    #[serde(default)]
    pub currency: Currency,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerEffortCents {
    #[serde(default = "default_fast_cents")]
    pub fast: f64,
    #[serde(default = "default_balanced_cents")]
    pub balanced: f64,
    #[serde(default = "default_max_cents")]
    pub max: f64,
}

impl PerEffortCents {
    pub fn for_effort(&self, effort: &str) -> f64 {
        match effort {
            "fast" => self.fast,
            "max" => self.max,
            _ => self.balanced,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Currency {
    Brl,
    Usd,
}

impl Default for Currency {
    fn default() -> Self {
        Currency::Brl
    }
}

impl Currency {
    pub fn as_str(self) -> &'static str {
        match self {
            Currency::Brl => "BRL",
            Currency::Usd => "USD",
        }
    }
}

fn default_share_pct() -> f64 {
    30.0
}
fn default_reference_gb_month_cents() -> f64 {
    12.0
}
fn default_fast_cents() -> f64 {
    150.0
}
fn default_balanced_cents() -> f64 {
    400.0
}
fn default_max_cents() -> f64 {
    1600.0
}
fn default_per_gb_cents() -> PerEffortCents {
    PerEffortCents {
        fast: default_fast_cents(),
        balanced: default_balanced_cents(),
        max: default_max_cents(),
    }
}

impl Default for PriceConfig {
    fn default() -> Self {
        PriceConfig {
            share_pct: default_share_pct(),
            reference_gb_month_cents: default_reference_gb_month_cents(),
            per_gb_cents: default_per_gb_cents(),
            currency: Currency::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub plan: PlanKind,
    pub price: PriceConfig,
    pub active: bool,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct ApiKeyRecord {
    pub key_id: String,
    pub tenant_id: String,
    pub name: String,
    pub scopes: BTreeSet<Scope>,
    pub expires_at_ms: Option<i64>,
    pub revoked_at_ms: Option<i64>,
    pub last_used_at_ms: Option<i64>,
    pub created_at_ms: i64,
}

impl ApiKeyRecord {
    pub fn usable_at(&self, now_ms: i64) -> bool {
        self.revoked_at_ms.is_none() && self.expires_at_ms.map(|e| e > now_ms).unwrap_or(true)
    }
}

#[derive(Debug, Clone)]
pub struct IssuedKey {
    pub record: ApiKeyRecord,
    pub secret: String,
}

#[derive(Debug, Clone)]
pub struct TenantContext {
    pub tenant: Tenant,
    pub key_id: Option<String>,
    pub scopes: BTreeSet<Scope>,
}

impl TenantContext {
    pub fn has(&self, scope: Scope) -> bool {
        self.scopes.contains(&scope) || self.scopes.contains(&Scope::Admin)
    }

    pub fn tenant_id(&self) -> &str {
        &self.tenant.id
    }
}

pub fn generate_key(tenant_id: &str, name: &str, scopes: BTreeSet<Scope>, now_ms: i64) -> IssuedKey {
    let key_id = hex::encode(&uuid::Uuid::new_v4().as_bytes()[..6]);
    let mut raw = Vec::with_capacity(32);
    raw.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    raw.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    let secret = hex::encode(&raw[..32]);

    IssuedKey {
        record: ApiKeyRecord {
            key_id: key_id.clone(),
            tenant_id: tenant_id.to_string(),
            name: name.to_string(),
            scopes,
            expires_at_ms: None,
            revoked_at_ms: None,
            last_used_at_ms: None,
            created_at_ms: now_ms,
        },
        secret: format!("{KEY_PREFIX}_{key_id}_{secret}"),
    }
}

pub fn hash_secret(secret: &str) -> String {
    hex::encode(blake3::hash(secret.as_bytes()).as_bytes())
}

pub fn split_presented_key(presented: &str) -> Option<(String, String)> {
    let mut parts = presented.trim().splitn(3, '_');
    let prefix = parts.next()?;
    let key_id = parts.next()?;
    let secret = parts.next()?;
    if prefix != KEY_PREFIX || key_id.is_empty() || secret.is_empty() {
        return None;
    }
    Some((key_id.to_string(), secret.to_string()))
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[derive(sqlx::FromRow)]
struct TenantRow {
    id: String,
    name: String,
    plan: String,
    price_config: String,
    active: i64,
    created_at_ms: i64,
}

impl TenantRow {
    fn into_tenant(self) -> Tenant {
        Tenant {
            id: self.id,
            name: self.name,
            plan: PlanKind::parse(&self.plan).unwrap_or(PlanKind::SavingsShare),
            price: serde_json::from_str(&self.price_config).unwrap_or_default(),
            active: self.active != 0,
            created_at_ms: self.created_at_ms,
        }
    }
}

#[derive(sqlx::FromRow)]
struct KeyRow {
    key_id: String,
    tenant_id: String,
    name: String,
    key_hash: String,
    scopes: String,
    expires_at_ms: Option<i64>,
    revoked_at_ms: Option<i64>,
    last_used_at_ms: Option<i64>,
    created_at_ms: i64,
}

impl KeyRow {
    fn into_record(self) -> ApiKeyRecord {
        ApiKeyRecord {
            key_id: self.key_id,
            tenant_id: self.tenant_id,
            name: self.name,
            scopes: scopes_from_string(&self.scopes),
            expires_at_ms: self.expires_at_ms,
            revoked_at_ms: self.revoked_at_ms,
            last_used_at_ms: self.last_used_at_ms,
            created_at_ms: self.created_at_ms,
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GrantRow {
    pub object_hash: String,
    pub refs: i64,
    pub original_size: i64,
    pub essence_size: i64,
    pub name: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

pub async fn init_schema(db: &MonitorDB) -> Result<()> {
    let int = match db.db_type() {
        DbType::Sqlite => "INTEGER",
        DbType::Postgres => "BIGINT",
    };

    let statements = vec![
        format!(
            "CREATE TABLE IF NOT EXISTS tenants (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                plan TEXT NOT NULL,
                price_config TEXT NOT NULL,
                active {int} NOT NULL,
                created_at_ms {int} NOT NULL
            )"
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS api_keys (
                key_id TEXT PRIMARY KEY,
                tenant_id TEXT NOT NULL,
                name TEXT NOT NULL,
                key_hash TEXT NOT NULL,
                scopes TEXT NOT NULL,
                expires_at_ms {int},
                revoked_at_ms {int},
                last_used_at_ms {int},
                created_at_ms {int} NOT NULL
            )"
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS object_grants (
                tenant_id TEXT NOT NULL,
                object_hash TEXT NOT NULL,
                refs {int} NOT NULL,
                original_size {int} NOT NULL,
                essence_size {int} NOT NULL,
                name TEXT NOT NULL,
                created_at_ms {int} NOT NULL,
                updated_at_ms {int} NOT NULL,
                PRIMARY KEY (tenant_id, object_hash)
            )"
        ),
        "CREATE INDEX IF NOT EXISTS idx_keys_tenant ON api_keys(tenant_id)".to_string(),
        "CREATE INDEX IF NOT EXISTS idx_grants_hash ON object_grants(object_hash)".to_string(),
    ];

    for stmt in statements {
        sql_exec!(db, stmt).map_err(|e| anyhow!("criando schema de tenancy: {e}"))?;
    }
    Ok(())
}

pub struct TenancyStore;

impl TenancyStore {
    pub async fn upsert_tenant(db: &MonitorDB, tenant: &Tenant) -> Result<()> {
        let price = serde_json::to_string(&tenant.price)?;
        let existing = Self::find_tenant(db, &tenant.id).await?;

        if existing.is_some() {
            let sql = format!(
                "UPDATE tenants SET name = {}, plan = {}, price_config = {}, active = {} WHERE id = {}",
                db.ph(1), db.ph(2), db.ph(3), db.ph(4), db.ph(5)
            );
            sql_exec!(db, sql, tenant.name.clone(), tenant.plan.as_str().to_string(),
                      price, tenant.active as i64, tenant.id.clone())?;
        } else {
            let sql = format!(
                "INSERT INTO tenants (id, name, plan, price_config, active, created_at_ms) VALUES ({})",
                db.placeholders(6)
            );
            sql_exec!(db, sql, tenant.id.clone(), tenant.name.clone(),
                      tenant.plan.as_str().to_string(), price,
                      tenant.active as i64, tenant.created_at_ms)?;
        }
        Ok(())
    }

    pub async fn find_tenant(db: &MonitorDB, id: &str) -> Result<Option<Tenant>> {
        let sql = format!("SELECT * FROM tenants WHERE id = {}", db.ph(1));
        let row = sql_fetch_optional!(db, TenantRow, sql, id.to_string())?;
        Ok(row.map(TenantRow::into_tenant))
    }

    pub async fn list_tenants(db: &MonitorDB) -> Result<Vec<Tenant>> {
        let rows = sql_fetch_all!(db, TenantRow, "SELECT * FROM tenants ORDER BY created_at_ms".to_string())?;
        Ok(rows.into_iter().map(TenantRow::into_tenant).collect())
    }

    pub async fn ensure_default_tenant(db: &MonitorDB, now_ms: i64) -> Result<Tenant> {
        if let Some(t) = Self::find_tenant(db, DEFAULT_TENANT_ID).await? {
            return Ok(t);
        }
        let tenant = Tenant {
            id: DEFAULT_TENANT_ID.to_string(),
            name: "Tenant padrão".to_string(),
            plan: PlanKind::SavingsShare,
            price: PriceConfig::default(),
            active: true,
            created_at_ms: now_ms,
        };
        Self::upsert_tenant(db, &tenant).await?;
        Ok(tenant)
    }

    pub async fn count_keys(db: &MonitorDB) -> Result<i64> {
        #[derive(sqlx::FromRow)]
        struct Count {
            total: i64,
        }
        let row = sql_fetch_optional!(db, Count, "SELECT COUNT(*) AS total FROM api_keys".to_string())?;
        Ok(row.map(|c| c.total).unwrap_or(0))
    }

    pub async fn store_key(db: &MonitorDB, issued: &IssuedKey) -> Result<()> {
        let r = &issued.record;
        let sql = format!(
            "INSERT INTO api_keys (key_id, tenant_id, name, key_hash, scopes, expires_at_ms, revoked_at_ms, last_used_at_ms, created_at_ms) VALUES ({})",
            db.placeholders(9)
        );
        let (_, secret) = split_presented_key(&issued.secret)
            .ok_or_else(|| anyhow!("chave emitida em formato inválido"))?;
        sql_exec!(db, sql, r.key_id.clone(), r.tenant_id.clone(), r.name.clone(),
                  hash_secret(&secret), scopes_to_string(&r.scopes),
                  r.expires_at_ms, r.revoked_at_ms, r.last_used_at_ms, r.created_at_ms)?;
        Ok(())
    }

    pub async fn list_keys(db: &MonitorDB, tenant_id: &str) -> Result<Vec<ApiKeyRecord>> {
        let sql = format!(
            "SELECT * FROM api_keys WHERE tenant_id = {} ORDER BY created_at_ms DESC",
            db.ph(1)
        );
        let rows = sql_fetch_all!(db, KeyRow, sql, tenant_id.to_string())?;
        Ok(rows.into_iter().map(KeyRow::into_record).collect())
    }

    pub async fn revoke_key(db: &MonitorDB, key_id: &str, now_ms: i64) -> Result<bool> {
        let sql = format!(
            "UPDATE api_keys SET revoked_at_ms = {} WHERE key_id = {} AND revoked_at_ms IS NULL",
            db.ph(1), db.ph(2)
        );
        let n = sql_exec!(db, sql, now_ms, key_id.to_string())?;
        Ok(n > 0)
    }

    pub async fn authenticate(
        db: &MonitorDB,
        presented: &str,
        now_ms: i64,
    ) -> Result<Option<TenantContext>> {
        let Some((key_id, secret)) = split_presented_key(presented) else {
            return Ok(None);
        };

        let sql = format!("SELECT * FROM api_keys WHERE key_id = {}", db.ph(1));
        let Some(row) = sql_fetch_optional!(db, KeyRow, sql, key_id)? else {
            return Ok(None);
        };

        if !constant_time_eq(&row.key_hash, &hash_secret(&secret)) {
            return Ok(None);
        }

        let record = row.into_record();
        if !record.usable_at(now_ms) {
            return Ok(None);
        }

        let Some(tenant) = Self::find_tenant(db, &record.tenant_id).await? else {
            return Ok(None);
        };
        if !tenant.active {
            return Ok(None);
        }

        let touch = format!(
            "UPDATE api_keys SET last_used_at_ms = {} WHERE key_id = {}",
            db.ph(1), db.ph(2)
        );
        let _ = sql_exec!(db, touch, now_ms, record.key_id.clone());

        Ok(Some(TenantContext {
            tenant,
            key_id: Some(record.key_id),
            scopes: record.scopes,
        }))
    }

    pub async fn find_grant(
        db: &MonitorDB,
        tenant_id: &str,
        object_hash: &str,
    ) -> Result<Option<GrantRow>> {
        let sql = format!(
            "SELECT object_hash, refs, original_size, essence_size, name, created_at_ms, updated_at_ms
             FROM object_grants WHERE tenant_id = {} AND object_hash = {}",
            db.ph(1), db.ph(2)
        );
        Ok(sql_fetch_optional!(db, GrantRow, sql, tenant_id.to_string(), object_hash.to_string())?)
    }

    pub async fn add_grant(
        db: &MonitorDB,
        tenant_id: &str,
        object_hash: &str,
        name: &str,
        original_size: i64,
        essence_size: i64,
        now_ms: i64,
    ) -> Result<(i64, bool)> {
        match Self::find_grant(db, tenant_id, object_hash).await? {
            Some(existing) => {
                let refs = existing.refs + 1;
                let sql = format!(
                    "UPDATE object_grants SET refs = {}, updated_at_ms = {}
                     WHERE tenant_id = {} AND object_hash = {}",
                    db.ph(1), db.ph(2), db.ph(3), db.ph(4)
                );
                sql_exec!(db, sql, refs, now_ms, tenant_id.to_string(), object_hash.to_string())?;
                Ok((refs, false))
            }
            None => {
                let sql = format!(
                    "INSERT INTO object_grants (tenant_id, object_hash, refs, original_size, essence_size, name, created_at_ms, updated_at_ms) VALUES ({})",
                    db.placeholders(8)
                );
                sql_exec!(db, sql, tenant_id.to_string(), object_hash.to_string(), 1i64,
                          original_size, essence_size, name.to_string(), now_ms, now_ms)?;
                Ok((1, true))
            }
        }
    }

    pub async fn release_grant(
        db: &MonitorDB,
        tenant_id: &str,
        object_hash: &str,
        purge: bool,
        now_ms: i64,
    ) -> Result<Option<i64>> {
        let Some(existing) = Self::find_grant(db, tenant_id, object_hash).await? else {
            return Ok(None);
        };

        if !purge && existing.refs > 1 {
            let refs = existing.refs - 1;
            let sql = format!(
                "UPDATE object_grants SET refs = {}, updated_at_ms = {}
                 WHERE tenant_id = {} AND object_hash = {}",
                db.ph(1), db.ph(2), db.ph(3), db.ph(4)
            );
            sql_exec!(db, sql, refs, now_ms, tenant_id.to_string(), object_hash.to_string())?;
            return Ok(Some(refs));
        }

        let sql = format!(
            "DELETE FROM object_grants WHERE tenant_id = {} AND object_hash = {}",
            db.ph(1), db.ph(2)
        );
        sql_exec!(db, sql, tenant_id.to_string(), object_hash.to_string())?;
        Ok(Some(0))
    }

    pub async fn count_grants_for_object(db: &MonitorDB, object_hash: &str) -> Result<i64> {
        #[derive(sqlx::FromRow)]
        struct Count {
            total: i64,
        }
        let sql = format!(
            "SELECT COUNT(*) AS total FROM object_grants WHERE object_hash = {}",
            db.ph(1)
        );
        let row = sql_fetch_optional!(db, Count, sql, object_hash.to_string())?;
        Ok(row.map(|c| c.total).unwrap_or(0))
    }

    pub async fn list_grants(
        db: &MonitorDB,
        tenant_id: &str,
        limit: i64,
        name_like: Option<&str>,
    ) -> Result<Vec<GrantRow>> {
        let base = "SELECT object_hash, refs, original_size, essence_size, name, created_at_ms, updated_at_ms
                    FROM object_grants WHERE tenant_id = ";
        match name_like {
            Some(pattern) => {
                let sql = format!(
                    "{base}{} AND name LIKE {} ORDER BY updated_at_ms DESC LIMIT {limit}",
                    db.ph(1), db.ph(2)
                );
                let escaped = format!("%{}%", pattern.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
                Ok(sql_fetch_all!(db, GrantRow, sql, tenant_id.to_string(), escaped)?)
            }
            None => {
                let sql = format!(
                    "{base}{} ORDER BY updated_at_ms DESC LIMIT {limit}",
                    db.ph(1)
                );
                Ok(sql_fetch_all!(db, GrantRow, sql, tenant_id.to_string())?)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escopos_ida_e_volta() {
        let s = Scope::all();
        let raw = scopes_to_string(&s);
        assert_eq!(raw, "compress,read,delete,admin");
        assert_eq!(scopes_from_string(&raw), s);
        assert!(scopes_from_string("compress, read , lixo").contains(&Scope::Read));
        assert_eq!(scopes_from_string("lixo").len(), 0);
    }

    #[test]
    fn admin_implica_todos_os_escopos() {
        let ctx = TenantContext {
            tenant: Tenant {
                id: "t".into(),
                name: "t".into(),
                plan: PlanKind::SavingsShare,
                price: PriceConfig::default(),
                active: true,
                created_at_ms: 0,
            },
            key_id: None,
            scopes: [Scope::Admin].into_iter().collect(),
        };
        assert!(ctx.has(Scope::Compress));
        assert!(ctx.has(Scope::Delete));

        let restrito = TenantContext {
            scopes: [Scope::Read].into_iter().collect(),
            ..ctx.clone()
        };
        assert!(restrito.has(Scope::Read));
        assert!(!restrito.has(Scope::Delete));
    }

    #[test]
    fn chave_emitida_tem_formato_e_segredo_forte() {
        let k = generate_key("t1", "ci", Scope::all(), 1);
        let (key_id, secret) = split_presented_key(&k.secret).expect("formato inválido");
        assert_eq!(key_id, k.record.key_id);
        assert_eq!(key_id.len(), 12);
        assert_eq!(secret.len(), 64);
        assert!(k.secret.starts_with("syn_"));

        let outra = generate_key("t1", "ci", Scope::all(), 1);
        assert_ne!(k.secret, outra.secret);
    }

    #[test]
    fn segredo_nunca_e_recuperavel_do_hash() {
        let k = generate_key("t1", "ci", Scope::all(), 1);
        let (_, secret) = split_presented_key(&k.secret).unwrap();
        let h = hash_secret(&secret);
        assert_eq!(h.len(), 64);
        assert_ne!(h, secret);
        assert_eq!(h, hash_secret(&secret));
    }

    #[test]
    fn formato_de_chave_invalido_e_recusado() {
        for ruim in ["", "syn", "syn_", "syn_abc", "outro_abc_def", "abc_def", "syn__x"] {
            assert!(split_presented_key(ruim).is_none(), "aceitou {ruim:?}");
        }
    }

    #[test]
    fn comparacao_constante() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
    }

    #[test]
    fn chave_expirada_ou_revogada_nao_serve() {
        let mut r = ApiKeyRecord {
            key_id: "k".into(),
            tenant_id: "t".into(),
            name: "n".into(),
            scopes: Scope::all(),
            expires_at_ms: None,
            revoked_at_ms: None,
            last_used_at_ms: None,
            created_at_ms: 0,
        };
        assert!(r.usable_at(1000));

        r.expires_at_ms = Some(500);
        assert!(!r.usable_at(1000));
        assert!(r.usable_at(400));

        r.expires_at_ms = None;
        r.revoked_at_ms = Some(10);
        assert!(!r.usable_at(1000));
    }

    #[test]
    fn config_de_preco_aceita_json_parcial() {
        let p: PriceConfig = serde_json::from_str(r#"{"share_pct": 25}"#).unwrap();
        assert_eq!(p.share_pct, 25.0);
        assert_eq!(p.reference_gb_month_cents, default_reference_gb_month_cents());
        assert_eq!(p.per_gb_cents.max, default_max_cents());

        let vazio: PriceConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(vazio.share_pct, default_share_pct());
        assert_eq!(vazio.currency, Currency::Brl);
    }

    #[test]
    fn preco_por_esforco_cai_em_balanced_quando_desconhecido() {
        let p = default_per_gb_cents();
        assert_eq!(p.for_effort("fast"), p.fast);
        assert_eq!(p.for_effort("max"), p.max);
        assert_eq!(p.for_effort("balanced"), p.balanced);
        assert_eq!(p.for_effort("inexistente"), p.balanced);
    }
}
