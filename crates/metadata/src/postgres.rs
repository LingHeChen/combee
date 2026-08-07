//! PostgreSQL 元数据后端(SQLx + PgPool)。
//!
//! 表结构(设计文档第 6 节的最小形态):目录数据只存控制面信息,
//! 不存放任何用户业务数据。V0 单机部署,`storage_node_id` 等字段
//! 等接入独立 Data Node 时再补充。

use async_trait::async_trait;
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
}
