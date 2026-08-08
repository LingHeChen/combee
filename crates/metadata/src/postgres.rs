//! PostgreSQL 元数据后端(SQLx + PgPool)。
//!
//! 表结构(设计文档第 6 节的最小形态):目录数据只存控制面信息,
//! 不存放任何用户业务数据。V0 单机部署,`storage_node_id` 等字段
//! 等接入独立 Data Node 时再补充。

use async_trait::async_trait;
use combee_common::credit::{
    CreditAccount, CreditTransaction, CreditTransactionType, CreditVoucher, PricingRule,
    PricingStatus, PricingVersion, VoucherStatus,
};
use combee_common::usage::{UsageBucket, UsageKey, UsageMetric};
use combee_common::{CombeeError, DatabaseId, NodeId, Result, TenantId};
use sqlx::Row;
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use uuid::Uuid;

use crate::store::{ApiKeyRecord, DatabaseRecord, DatabaseState, MetadataStore, TenantRecord};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS tenants (
    id UUID PRIMARY KEY,
    created_at BIGINT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active'
);
CREATE TABLE IF NOT EXISTS api_keys (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    created_at BIGINT NOT NULL,
    revoked_at BIGINT
);
CREATE TABLE IF NOT EXISTS usage_buckets (
    tenant_id UUID NOT NULL,
    cell_id UUID,
    metric TEXT NOT NULL,
    bucket_start BIGINT NOT NULL,
    value BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (tenant_id, cell_id, metric, bucket_start)
);
CREATE TABLE IF NOT EXISTS pricing_versions (
    version BIGINT PRIMARY KEY,
    status TEXT NOT NULL,
    effective_at BIGINT NOT NULL,
    created_at BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS pricing_rules (
    pricing_version BIGINT NOT NULL,
    metric TEXT NOT NULL,
    unit_size BIGINT NOT NULL,
    price_units BIGINT NOT NULL,
    PRIMARY KEY (pricing_version, metric)
);
CREATE TABLE IF NOT EXISTS credit_accounts (
    tenant_id UUID PRIMARY KEY,
    balance_units BIGINT NOT NULL DEFAULT 0,
    reserved_units BIGINT NOT NULL DEFAULT 0,
    updated_at BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS credit_transactions (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    txn_type TEXT NOT NULL,
    amount_units BIGINT NOT NULL,
    pricing_version BIGINT,
    reference_id TEXT UNIQUE,
    description TEXT,
    created_at BIGINT NOT NULL,
    balance_after BIGINT
);
CREATE INDEX IF NOT EXISTS idx_credit_txns_tenant_created
    ON credit_transactions (tenant_id, created_at DESC);
CREATE TABLE IF NOT EXISTS credit_vouchers (
    id UUID PRIMARY KEY,
    code_hash TEXT NOT NULL UNIQUE,
    amount_units BIGINT NOT NULL,
    status TEXT NOT NULL,
    campaign TEXT,
    created_at BIGINT NOT NULL,
    expires_at BIGINT,
    redeemed_by UUID,
    redeemed_at BIGINT
);
CREATE TABLE IF NOT EXISTS idempotency_keys (
    idem_key TEXT PRIMARY KEY,
    tenant_id UUID NOT NULL,
    payload TEXT NOT NULL,
    created_at BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS databases (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    state TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    storage_node_id UUID
);
CREATE INDEX IF NOT EXISTS idx_databases_tenant_created
    ON databases (tenant_id, created_at);
";
/// 兼容旧库的迁移:补 storage_node_id 列。
const MIGRATE_STORAGE_NODE: &str =
    "ALTER TABLE databases ADD COLUMN IF NOT EXISTS storage_node_id UUID";
/// 兼容旧库的迁移:补 replica_node_id 列。
const MIGRATE_REPLICA_NODE: &str =
    "ALTER TABLE databases ADD COLUMN IF NOT EXISTS replica_node_id UUID";
/// 兼容旧库的迁移:补 generation 列(fencing)。
const MIGRATE_GENERATION: &str =
    "ALTER TABLE databases ADD COLUMN IF NOT EXISTS generation BIGINT NOT NULL DEFAULT 0";

/// 批量 INSERT 的每批条数。
const BATCH_SIZE: usize = 2_000;

/// PostgreSQL 元数据存储。
#[derive(Clone)]
pub struct PostgresStore {
    pool: PgPool,
}

impl PostgresStore {
    /// 连接 PostgreSQL 并确保 schema 存在。
    pub async fn connect(url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(url)
            .await
            .map_err(|e| CombeeError::Internal(format!("postgres connect: {e}")))?;
        sqlx::raw_sql(SCHEMA)
            .execute(&pool)
            .await
            .map_err(|e| CombeeError::Internal(format!("postgres migrate: {e}")))?;
        sqlx::raw_sql(MIGRATE_STORAGE_NODE)
            .execute(&pool)
            .await
            .map_err(|e| CombeeError::Internal(format!("postgres migrate: {e}")))?;
        sqlx::raw_sql(MIGRATE_REPLICA_NODE)
            .execute(&pool)
            .await
            .map_err(|e| CombeeError::Internal(format!("postgres migrate: {e}")))?;
        sqlx::raw_sql(MIGRATE_GENERATION)
            .execute(&pool)
            .await
            .map_err(|e| CombeeError::Internal(format!("postgres migrate: {e}")))?;
        Ok(Self { pool })
    }

    fn internal(e: sqlx::Error) -> CombeeError {
        CombeeError::Internal(format!("postgres error: {e}"))
    }
}

fn row_to_record(row: &PgRow) -> Result<DatabaseRecord> {
    let id: Uuid = row.try_get("id").map_err(PostgresStore::internal)?;
    let tenant_id: Uuid = row.try_get("tenant_id").map_err(PostgresStore::internal)?;
    let state: String = row.try_get("state").map_err(PostgresStore::internal)?;
    let created_at: i64 = row.try_get("created_at").map_err(PostgresStore::internal)?;
    let storage_node_id: Option<Uuid> = row
        .try_get("storage_node_id")
        .map_err(PostgresStore::internal)?;
    let replica_node_id: Option<Uuid> = row
        .try_get("replica_node_id")
        .map_err(PostgresStore::internal)?;
    let generation: i64 = row.try_get("generation").map_err(PostgresStore::internal)?;
    Ok(DatabaseRecord {
        id: DatabaseId(id),
        tenant_id: TenantId(tenant_id),
        state: DatabaseState::parse(&state)?,
        created_at: created_at as u64,
        storage_node_id: storage_node_id.map(NodeId),
        replica_node_id: replica_node_id.map(NodeId),
        generation,
    })
}

#[async_trait]
impl MetadataStore for PostgresStore {
    async fn create_database(
        &self,
        tenant: TenantId,
        id: DatabaseId,
        storage_node: Option<NodeId>,
    ) -> Result<DatabaseRecord> {
        let now = DatabaseRecord::now_unix() as i64;
        let inserted = sqlx::query(
            "INSERT INTO databases (id, tenant_id, state, created_at, storage_node_id, generation)
             VALUES ($1, $2, $3, $4, $5, 0)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(id.0)
        .bind(tenant.0)
        .bind(DatabaseState::Created.as_str())
        .bind(now)
        .bind(storage_node.map(|n| n.0))
        .execute(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        if inserted.rows_affected() == 0 {
            return Err(CombeeError::DatabaseAlreadyExists(id));
        }
        self.get_database(tenant, id).await
    }

    async fn get_database(&self, tenant: TenantId, id: DatabaseId) -> Result<DatabaseRecord> {
        let row = sqlx::query(
            "SELECT id, tenant_id, state, created_at, storage_node_id, replica_node_id, generation FROM databases WHERE id = $1 AND tenant_id = $2",
        )
        .bind(id.0)
        .bind(tenant.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        match row {
            Some(row) => row_to_record(&row),
            None => Err(CombeeError::DatabaseNotFound(id)),
        }
    }

    async fn list_databases(&self, tenant: TenantId) -> Result<Vec<DatabaseRecord>> {
        let rows = sqlx::query(
            "SELECT id, tenant_id, state, created_at, storage_node_id, replica_node_id FROM databases
             WHERE tenant_id = $1 ORDER BY created_at, id",
        )
        .bind(tenant.0)
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        rows.iter().map(row_to_record).collect()
    }

    async fn delete_database(&self, tenant: TenantId, id: DatabaseId) -> Result<()> {
        let deleted = sqlx::query("DELETE FROM databases WHERE id = $1 AND tenant_id = $2")
            .bind(id.0)
            .bind(tenant.0)
            .execute(&self.pool)
            .await
            .map_err(PostgresStore::internal)?;
        if deleted.rows_affected() == 0 {
            return Err(CombeeError::DatabaseNotFound(id));
        }
        Ok(())
    }

    async fn set_replica_node(
        &self,
        tenant: TenantId,
        id: DatabaseId,
        replica: Option<NodeId>,
    ) -> Result<DatabaseRecord> {
        let updated = sqlx::query(
            "UPDATE databases SET replica_node_id = $1 WHERE id = $2 AND tenant_id = $3",
        )
        .bind(replica.map(|n| n.0))
        .bind(id.0)
        .bind(tenant.0)
        .execute(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        if updated.rows_affected() == 0 {
            return Err(CombeeError::DatabaseNotFound(id));
        }
        self.get_database(tenant, id).await
    }

    async fn list_replicas_of(&self, node: NodeId) -> Result<Vec<DatabaseRecord>> {
        let rows = sqlx::query(
            "SELECT id, tenant_id, state, created_at, storage_node_id, replica_node_id FROM databases
             WHERE replica_node_id = $1 ORDER BY id",
        )
        .bind(node.0)
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        rows.iter().map(row_to_record).collect()
    }

    async fn promote_replica(&self, tenant: TenantId, id: DatabaseId) -> Result<DatabaseRecord> {
        let updated = sqlx::query(
            "UPDATE databases SET storage_node_id = replica_node_id, replica_node_id = NULL,
             generation = generation + 1
             WHERE id = $1 AND tenant_id = $2 AND replica_node_id IS NOT NULL",
        )
        .bind(id.0)
        .bind(tenant.0)
        .execute(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        if updated.rows_affected() == 0 {
            let exists = self.get_database(tenant, id).await;
            return match exists {
                Ok(_) => Err(CombeeError::Internal(format!(
                    "cell {id} has no replica to promote"
                ))),
                Err(e) => Err(e),
            };
        }
        self.get_database(tenant, id).await
    }

    async fn list_all_databases(&self) -> Result<Vec<DatabaseRecord>> {
        let rows = sqlx::query(
            "SELECT id, tenant_id, state, created_at, storage_node_id, replica_node_id, generation
             FROM databases ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        rows.iter().map(row_to_record).collect()
    }

    async fn create_tenant(&self, tenant: TenantId) -> Result<TenantRecord> {
        let now = DatabaseRecord::now_unix() as i64;
        sqlx::query("INSERT INTO tenants (id, created_at, status) VALUES ($1, $2, 'active')")
            .bind(tenant.0)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(PostgresStore::internal)?;
        Ok(TenantRecord {
            id: tenant,
            created_at: now as u64,
            status: "active".into(),
        })
    }

    async fn list_tenants(&self) -> Result<Vec<TenantRecord>> {
        let rows = sqlx::query("SELECT id, created_at, status FROM tenants ORDER BY created_at")
            .fetch_all(&self.pool)
            .await
            .map_err(PostgresStore::internal)?;
        rows.iter()
            .map(|row| {
                Ok(TenantRecord {
                    id: TenantId(row.try_get("id").map_err(PostgresStore::internal)?),
                    created_at: row
                        .try_get::<i64, _>("created_at")
                        .map_err(PostgresStore::internal)? as u64,
                    status: row.try_get("status").map_err(PostgresStore::internal)?,
                })
            })
            .collect()
    }

    async fn create_api_key(&self, tenant: TenantId, key_hash: String) -> Result<ApiKeyRecord> {
        let id = Uuid::new_v4();
        let now = DatabaseRecord::now_unix() as i64;
        sqlx::query(
            "INSERT INTO api_keys (id, tenant_id, key_hash, created_at) VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(tenant.0)
        .bind(&key_hash)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        Ok(ApiKeyRecord {
            id,
            tenant_id: tenant,
            key_hash,
            created_at: now as u64,
            revoked_at: None,
        })
    }

    async fn lookup_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKeyRecord>> {
        let row = sqlx::query(
            "SELECT id, tenant_id, key_hash, created_at, revoked_at FROM api_keys
             WHERE key_hash = $1 AND revoked_at IS NULL",
        )
        .bind(key_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        match row {
            Some(row) => Ok(Some(ApiKeyRecord {
                id: row.try_get("id").map_err(PostgresStore::internal)?,
                tenant_id: TenantId(row.try_get("tenant_id").map_err(PostgresStore::internal)?),
                key_hash: row.try_get("key_hash").map_err(PostgresStore::internal)?,
                created_at: row
                    .try_get::<i64, _>("created_at")
                    .map_err(PostgresStore::internal)? as u64,
                revoked_at: row
                    .try_get::<Option<i64>, _>("revoked_at")
                    .map_err(PostgresStore::internal)?
                    .map(|v| v as u64),
            })),
            None => Ok(None),
        }
    }

    async fn list_api_keys(&self, tenant: TenantId) -> Result<Vec<ApiKeyRecord>> {
        let rows = sqlx::query(
            "SELECT id, tenant_id, key_hash, created_at, revoked_at FROM api_keys
             WHERE tenant_id = $1 ORDER BY created_at",
        )
        .bind(tenant.0)
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        rows.iter()
            .map(|row| {
                Ok(ApiKeyRecord {
                    id: row.try_get("id").map_err(PostgresStore::internal)?,
                    tenant_id: TenantId(row.try_get("tenant_id").map_err(PostgresStore::internal)?),
                    key_hash: row.try_get("key_hash").map_err(PostgresStore::internal)?,
                    created_at: row
                        .try_get::<i64, _>("created_at")
                        .map_err(PostgresStore::internal)? as u64,
                    revoked_at: row
                        .try_get::<Option<i64>, _>("revoked_at")
                        .map_err(PostgresStore::internal)?
                        .map(|v| v as u64),
                })
            })
            .collect()
    }

    async fn revoke_api_key(&self, tenant: TenantId, key_id: Uuid) -> Result<()> {
        let now = DatabaseRecord::now_unix() as i64;
        let updated = sqlx::query(
            "UPDATE api_keys SET revoked_at = $1 WHERE id = $2 AND tenant_id = $3 AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(key_id)
        .bind(tenant.0)
        .execute(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        if updated.rows_affected() == 0 {
            return Err(CombeeError::ApiKeyNotFound);
        }
        Ok(())
    }

    /// 批量创建:UNNEST 数组一次插入一批(已存在的静默跳过),远快于逐条 roundtrip。
    async fn create_databases_batch(&self, tenant: TenantId, ids: &[DatabaseId]) -> Result<()> {
        let now = DatabaseRecord::now_unix() as i64;
        for chunk in ids.chunks(BATCH_SIZE) {
            let uuids: Vec<Uuid> = chunk.iter().map(|d| d.0).collect();
            sqlx::query(
                "INSERT INTO databases (id, tenant_id, state, created_at, generation)
                 SELECT id, $2, $3, $4, 0 FROM unnest($1::uuid[]) AS t(id)
                 ON CONFLICT (id) DO NOTHING",
            )
            .bind(&uuids)
            .bind(tenant.0)
            .bind(DatabaseState::Created.as_str())
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(PostgresStore::internal)?;
        }
        Ok(())
    }
    async fn usage_add(&self, key: &UsageKey, delta: u64) -> Result<()> {
        sqlx::query(
            "INSERT INTO usage_buckets (tenant_id, cell_id, metric, bucket_start, value)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (tenant_id, cell_id, metric, bucket_start)
             DO UPDATE SET value = usage_buckets.value + EXCLUDED.value",
        )
        .bind(key.tenant_id.0)
        .bind(key.cell_id.map(|c| c.0))
        .bind(key.metric.as_str())
        .bind(key.bucket_start)
        .bind(delta as i64)
        .execute(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        Ok(())
    }

    async fn usage_set(&self, key: &UsageKey, value: u64) -> Result<()> {
        sqlx::query(
            "INSERT INTO usage_buckets (tenant_id, cell_id, metric, bucket_start, value)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (tenant_id, cell_id, metric, bucket_start)
             DO UPDATE SET value = EXCLUDED.value",
        )
        .bind(key.tenant_id.0)
        .bind(key.cell_id.map(|c| c.0))
        .bind(key.metric.as_str())
        .bind(key.bucket_start)
        .bind(value as i64)
        .execute(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        Ok(())
    }

    async fn query_usage(
        &self,
        tenant: TenantId,
        cell: Option<DatabaseId>,
        metric: Option<UsageMetric>,
        from_bucket: i64,
        to_bucket: i64,
    ) -> Result<Vec<UsageBucket>> {
        // 四种过滤组合分开构造,避免动态 SQL 绑定类型体操
        let rows = match (cell, metric) {
            (Some(c), Some(m)) => {
                sqlx::query(
                    "SELECT tenant_id, cell_id, metric, bucket_start, value FROM usage_buckets
                     WHERE tenant_id = $1 AND bucket_start >= $2 AND bucket_start <= $3
                       AND cell_id = $4 AND metric = $5",
                )
                .bind(tenant.0)
                .bind(from_bucket)
                .bind(to_bucket)
                .bind(c.0)
                .bind(m.as_str())
                .fetch_all(&self.pool)
                .await
                .map_err(PostgresStore::internal)?
            }
            (Some(c), None) => {
                sqlx::query(
                    "SELECT tenant_id, cell_id, metric, bucket_start, value FROM usage_buckets
                     WHERE tenant_id = $1 AND bucket_start >= $2 AND bucket_start <= $3 AND cell_id = $4",
                )
                .bind(tenant.0)
                .bind(from_bucket)
                .bind(to_bucket)
                .bind(c.0)
                .fetch_all(&self.pool)
                .await
                .map_err(PostgresStore::internal)?
            }
            (None, Some(m)) => {
                sqlx::query(
                    "SELECT tenant_id, cell_id, metric, bucket_start, value FROM usage_buckets
                     WHERE tenant_id = $1 AND bucket_start >= $2 AND bucket_start <= $3 AND metric = $4",
                )
                .bind(tenant.0)
                .bind(from_bucket)
                .bind(to_bucket)
                .bind(m.as_str())
                .fetch_all(&self.pool)
                .await
                .map_err(PostgresStore::internal)?
            }
            (None, None) => {
                sqlx::query(
                    "SELECT tenant_id, cell_id, metric, bucket_start, value FROM usage_buckets
                     WHERE tenant_id = $1 AND bucket_start >= $2 AND bucket_start <= $3",
                )
                .bind(tenant.0)
                .bind(from_bucket)
                .bind(to_bucket)
                .fetch_all(&self.pool)
                .await
                .map_err(PostgresStore::internal)?
            }
        };
        rows.iter()
            .map(|row| {
                Ok(UsageBucket {
                    tenant_id: TenantId(row.try_get("tenant_id").map_err(PostgresStore::internal)?),
                    cell_id: row
                        .try_get::<Option<Uuid>, _>("cell_id")
                        .map_err(PostgresStore::internal)?
                        .map(DatabaseId),
                    metric: UsageMetric::parse(
                        &row.try_get::<String, _>("metric")
                            .map_err(PostgresStore::internal)?,
                    )
                    .ok_or_else(|| CombeeError::Internal("unknown usage metric".into()))?,
                    bucket_start: row
                        .try_get("bucket_start")
                        .map_err(PostgresStore::internal)?,
                    value: row
                        .try_get::<i64, _>("value")
                        .map_err(PostgresStore::internal)? as u64,
                })
            })
            .collect()
    }

    async fn create_pricing_version(&self, rules: Vec<PricingRule>) -> Result<PricingVersion> {
        let now = DatabaseRecord::now_unix() as i64;
        let mut tx = self.pool.begin().await.map_err(PostgresStore::internal)?;
        let version: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) + 1 FROM pricing_versions")
                .fetch_one(&mut *tx)
                .await
                .map_err(PostgresStore::internal)?;
        sqlx::query("UPDATE pricing_versions SET status = 'inactive' WHERE status = 'active'")
            .execute(&mut *tx)
            .await
            .map_err(PostgresStore::internal)?;
        sqlx::query(
            "INSERT INTO pricing_versions (version, status, effective_at, created_at) VALUES ($1, 'active', $2, $2)",
        )
        .bind(version)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(PostgresStore::internal)?;
        for r in &rules {
            sqlx::query(
                "INSERT INTO pricing_rules (pricing_version, metric, unit_size, price_units) VALUES ($1, $2, $3, $4)",
            )
            .bind(version)
            .bind(r.metric.as_str())
            .bind(r.unit_size)
            .bind(r.price_units)
            .execute(&mut *tx)
            .await
            .map_err(PostgresStore::internal)?;
        }
        tx.commit().await.map_err(PostgresStore::internal)?;
        Ok(PricingVersion {
            version,
            status: PricingStatus::Active,
            effective_at: now,
            created_at: now,
        })
    }

    async fn get_active_pricing(&self) -> Result<(PricingVersion, Vec<PricingRule>)> {
        let row = sqlx::query(
            "SELECT version, status, effective_at, created_at FROM pricing_versions WHERE status = 'active'",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        let Some(row) = row else {
            return Ok((
                PricingVersion {
                    version: 0,
                    status: PricingStatus::Active,
                    effective_at: 0,
                    created_at: 0,
                },
                vec![],
            ));
        };
        let version: i64 = row.try_get("version").map_err(PostgresStore::internal)?;
        let rules = sqlx::query(
            "SELECT pricing_version, metric, unit_size, price_units FROM pricing_rules WHERE pricing_version = $1",
        )
        .bind(version)
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        let rules = rules
            .iter()
            .map(|r| {
                Ok(PricingRule {
                    pricing_version: r
                        .try_get("pricing_version")
                        .map_err(PostgresStore::internal)?,
                    metric: UsageMetric::parse(
                        &r.try_get::<String, _>("metric")
                            .map_err(PostgresStore::internal)?,
                    )
                    .ok_or_else(|| CombeeError::Internal("unknown metric".into()))?,
                    unit_size: r.try_get("unit_size").map_err(PostgresStore::internal)?,
                    price_units: r.try_get("price_units").map_err(PostgresStore::internal)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((
            PricingVersion {
                version,
                status: PricingStatus::Active,
                effective_at: row
                    .try_get("effective_at")
                    .map_err(PostgresStore::internal)?,
                created_at: row.try_get("created_at").map_err(PostgresStore::internal)?,
            },
            rules,
        ))
    }

    async fn list_pricing_versions(&self) -> Result<Vec<PricingVersion>> {
        let rows = sqlx::query(
            "SELECT version, status, effective_at, created_at FROM pricing_versions ORDER BY version",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        rows.iter()
            .map(|row| {
                Ok(PricingVersion {
                    version: row.try_get("version").map_err(PostgresStore::internal)?,
                    status: if row
                        .try_get::<String, _>("status")
                        .map_err(PostgresStore::internal)?
                        == "active"
                    {
                        PricingStatus::Active
                    } else {
                        PricingStatus::Inactive
                    },
                    effective_at: row
                        .try_get("effective_at")
                        .map_err(PostgresStore::internal)?,
                    created_at: row.try_get("created_at").map_err(PostgresStore::internal)?,
                })
            })
            .collect()
    }

    async fn get_credit_account(&self, tenant: TenantId) -> Result<CreditAccount> {
        let row = sqlx::query(
            "SELECT tenant_id, balance_units, reserved_units, updated_at FROM credit_accounts WHERE tenant_id = $1",
        )
        .bind(tenant.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        if let Some(row) = row {
            return Ok(CreditAccount {
                tenant_id: TenantId(row.try_get("tenant_id").map_err(PostgresStore::internal)?),
                balance_units: row
                    .try_get("balance_units")
                    .map_err(PostgresStore::internal)?,
                reserved_units: row
                    .try_get("reserved_units")
                    .map_err(PostgresStore::internal)?,
                updated_at: row.try_get("updated_at").map_err(PostgresStore::internal)?,
            });
        }
        let now = DatabaseRecord::now_unix() as i64;
        sqlx::query(
            "INSERT INTO credit_accounts (tenant_id, balance_units, reserved_units, updated_at) VALUES ($1, 0, 0, $2)",
        )
        .bind(tenant.0)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        Ok(CreditAccount {
            tenant_id: tenant,
            balance_units: 0,
            reserved_units: 0,
            updated_at: now,
        })
    }

    async fn list_credit_transactions(
        &self,
        tenant: TenantId,
        limit: i64,
        before: Option<Uuid>,
    ) -> Result<Vec<CreditTransaction>> {
        let rows = match before {
            Some(b) => sqlx::query(
                "SELECT id, tenant_id, txn_type, amount_units, pricing_version, reference_id, description, created_at, balance_after
                 FROM credit_transactions WHERE tenant_id = $1 AND id < $2 ORDER BY created_at DESC, id DESC LIMIT $3",
            )
            .bind(tenant.0)
            .bind(b)
            .bind(limit.clamp(1, 1000))
            .fetch_all(&self.pool)
            .await
            .map_err(PostgresStore::internal)?,
            None => sqlx::query(
                "SELECT id, tenant_id, txn_type, amount_units, pricing_version, reference_id, description, created_at, balance_after
                 FROM credit_transactions WHERE tenant_id = $1 ORDER BY created_at DESC, id DESC LIMIT $2",
            )
            .bind(tenant.0)
            .bind(limit.clamp(1, 1000))
            .fetch_all(&self.pool)
            .await
            .map_err(PostgresStore::internal)?,
        };
        rows.iter().map(row_to_txn).collect()
    }

    async fn find_transaction_by_reference(
        &self,
        reference_id: &str,
    ) -> Result<Option<CreditTransaction>> {
        let row = sqlx::query(
            "SELECT id, tenant_id, txn_type, amount_units, pricing_version, reference_id, description, created_at, balance_after
             FROM credit_transactions WHERE reference_id = $1",
        )
        .bind(reference_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        match row {
            Some(r) => Ok(Some(row_to_txn(&r)?)),
            None => Ok(None),
        }
    }

    async fn append_credit_transaction(
        &self,
        mut txn: CreditTransaction,
    ) -> Result<CreditTransaction> {
        if let Some(ref_id) = &txn.reference_id
            && let Some(existing) = self.find_transaction_by_reference(ref_id).await?
        {
            return Ok(existing);
        }
        let mut tx = self.pool.begin().await.map_err(PostgresStore::internal)?;
        sqlx::query(
            "INSERT INTO credit_transactions (id, tenant_id, txn_type, amount_units, pricing_version, reference_id, description, created_at, balance_after)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NULL)
             ON CONFLICT (reference_id) DO NOTHING",
        )
        .bind(txn.id)
        .bind(txn.tenant_id.0)
        .bind(txn.txn_type.as_str())
        .bind(txn.amount_units)
        .bind(txn.pricing_version)
        .bind(&txn.reference_id)
        .bind(&txn.description)
        .bind(txn.created_at)
        .execute(&mut *tx)
        .await
        .map_err(PostgresStore::internal)?;
        let balance: i64 = sqlx::query_scalar(
            "UPDATE credit_accounts SET balance_units = balance_units + $2, updated_at = $3
             WHERE tenant_id = $1 RETURNING balance_units",
        )
        .bind(txn.tenant_id.0)
        .bind(txn.amount_units)
        .bind(txn.created_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(PostgresStore::internal)?;
        tx.commit().await.map_err(PostgresStore::internal)?;
        txn.balance_after = Some(balance);
        Ok(txn)
    }

    async fn create_vouchers(
        &self,
        amount_units: i64,
        count: u32,
        campaign: Option<String>,
        expires_at: Option<i64>,
    ) -> Result<Vec<(String, CreditVoucher)>> {
        let now = DatabaseRecord::now_unix() as i64;
        let mut out = Vec::new();
        for _ in 0..count {
            let code = combee_common::credit::generate_voucher_code();
            let code_hash = combee_common::credit::hash_voucher_code(&code);
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO credit_vouchers (id, code_hash, amount_units, status, campaign, created_at, expires_at)
                 VALUES ($1, $2, $3, 'active', $4, $5, $6)",
            )
            .bind(id)
            .bind(&code_hash)
            .bind(amount_units)
            .bind(&campaign)
            .bind(now)
            .bind(expires_at)
            .execute(&self.pool)
            .await
            .map_err(PostgresStore::internal)?;
            out.push((
                code,
                CreditVoucher {
                    id,
                    code_hash,
                    amount_units,
                    status: VoucherStatus::Active,
                    campaign: campaign.clone(),
                    created_at: now,
                    expires_at,
                    redeemed_by: None,
                    redeemed_at: None,
                },
            ));
        }
        Ok(out)
    }

    async fn redeem_voucher(&self, code_hash: &str, tenant: TenantId, now: i64) -> Result<i64> {
        let mut tx = self.pool.begin().await.map_err(PostgresStore::internal)?;
        let row = sqlx::query(
            "UPDATE credit_vouchers SET status = 'used', redeemed_by = $2, redeemed_at = $3
             WHERE code_hash = $1 AND status = 'active' AND (expires_at IS NULL OR expires_at > $3)
             RETURNING id, amount_units",
        )
        .bind(code_hash)
        .bind(tenant.0)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(PostgresStore::internal)?;
        let Some(row) = row else {
            return Err(CombeeError::InvalidRequest(
                "voucher invalid, expired or already used".into(),
            ));
        };
        let amount: i64 = row
            .try_get("amount_units")
            .map_err(PostgresStore::internal)?;
        let txn = CreditTransaction {
            id: Uuid::new_v4(),
            tenant_id: tenant,
            txn_type: CreditTransactionType::Voucher,
            amount_units: amount,
            pricing_version: None,
            reference_id: Some(format!("voucher:{code_hash}")),
            description: Some("voucher redemption".into()),
            created_at: now,
            balance_after: None,
        };
        sqlx::query(
            "INSERT INTO credit_transactions (id, tenant_id, txn_type, amount_units, pricing_version, reference_id, description, created_at, balance_after)
             VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, NULL)",
        )
        .bind(txn.id)
        .bind(tenant.0)
        .bind("voucher")
        .bind(amount)
        .bind(&txn.reference_id)
        .bind("voucher redemption")
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(PostgresStore::internal)?;
        sqlx::query_scalar::<_, i64>(
            "UPDATE credit_accounts SET balance_units = balance_units + $2, updated_at = $3
             WHERE tenant_id = $1 RETURNING balance_units",
        )
        .bind(tenant.0)
        .bind(amount)
        .bind(now)
        .fetch_one(&mut *tx)
        .await
        .map_err(PostgresStore::internal)?;
        tx.commit().await.map_err(PostgresStore::internal)?;
        Ok(amount)
    }

    async fn list_vouchers(&self, limit: i64) -> Result<Vec<CreditVoucher>> {
        let rows = sqlx::query(
            "SELECT id, code_hash, amount_units, status, campaign, created_at, expires_at, redeemed_by, redeemed_at
             FROM credit_vouchers ORDER BY created_at DESC LIMIT $1",
        )
        .bind(limit.clamp(1, 1000))
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        rows.iter()
            .map(|row| {
                Ok(CreditVoucher {
                    id: row.try_get("id").map_err(PostgresStore::internal)?,
                    code_hash: row.try_get("code_hash").map_err(PostgresStore::internal)?,
                    amount_units: row
                        .try_get("amount_units")
                        .map_err(PostgresStore::internal)?,
                    status: VoucherStatus::parse(
                        &row.try_get::<String, _>("status")
                            .map_err(PostgresStore::internal)?,
                    )
                    .ok_or_else(|| CombeeError::Internal("bad voucher status".into()))?,
                    campaign: row.try_get("campaign").map_err(PostgresStore::internal)?,
                    created_at: row.try_get("created_at").map_err(PostgresStore::internal)?,
                    expires_at: row.try_get("expires_at").map_err(PostgresStore::internal)?,
                    redeemed_by: row
                        .try_get::<Option<Uuid>, _>("redeemed_by")
                        .map_err(PostgresStore::internal)?
                        .map(TenantId),
                    redeemed_at: row
                        .try_get("redeemed_at")
                        .map_err(PostgresStore::internal)?,
                })
            })
            .collect()
    }

    async fn save_idempotency(
        &self,
        key: &str,
        tenant: TenantId,
        payload: String,
    ) -> Result<Option<String>> {
        let now = DatabaseRecord::now_unix() as i64;
        let inserted = sqlx::query(
            "INSERT INTO idempotency_keys (idem_key, tenant_id, payload, created_at)
             VALUES ($1, $2, $3, $4) ON CONFLICT (idem_key) DO NOTHING RETURNING payload",
        )
        .bind(key)
        .bind(tenant.0)
        .bind(&payload)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        if inserted.is_some() {
            return Ok(None);
        }
        // 冲突:返回已存 payload
        let row = sqlx::query(
            "SELECT payload FROM idempotency_keys WHERE idem_key = $1 AND tenant_id = $2",
        )
        .bind(key)
        .bind(tenant.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(PostgresStore::internal)?;
        Ok(row.map(|r| r.try_get("payload").unwrap_or_default()))
    }
}

/// sqlx 行 → CreditTransaction。
fn row_to_txn(row: &sqlx::postgres::PgRow) -> Result<CreditTransaction> {
    Ok(CreditTransaction {
        id: row.try_get("id").map_err(PostgresStore::internal)?,
        tenant_id: TenantId(row.try_get("tenant_id").map_err(PostgresStore::internal)?),
        txn_type: CreditTransactionType::parse(
            &row.try_get::<String, _>("txn_type")
                .map_err(PostgresStore::internal)?,
        )
        .ok_or_else(|| CombeeError::Internal("bad txn type".into()))?,
        amount_units: row
            .try_get("amount_units")
            .map_err(PostgresStore::internal)?,
        pricing_version: row
            .try_get("pricing_version")
            .map_err(PostgresStore::internal)?,
        reference_id: row
            .try_get("reference_id")
            .map_err(PostgresStore::internal)?,
        description: row
            .try_get("description")
            .map_err(PostgresStore::internal)?,
        created_at: row.try_get("created_at").map_err(PostgresStore::internal)?,
        balance_after: row
            .try_get("balance_after")
            .map_err(PostgresStore::internal)?,
    })
}
