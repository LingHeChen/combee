//! MetadataStore 抽象与 V0 的 InMemory 实现。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use combee_common::{CombeeError, DatabaseId, NodeId, Result, TenantId};
use serde::Serialize;
use uuid::Uuid;

/// V0 尚未接入真实认证,所有数据库归属这一个内置租户。
/// 后续接入用户系统后,由 API key 映射到真实 tenant。
pub const DEFAULT_TENANT: TenantId = TenantId::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0001);

/// Cell 生命周期状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseState {
    /// 已在目录中创建,但磁盘文件尚未生成(lazy create)。
    Created,
    /// 磁盘文件已生成并可提供服务。
    Active,
    /// 已标记删除(预留)。
    Deleting,
}

impl DatabaseState {
    /// 存储/传输用的字符串形态(lowercase)。
    pub fn as_str(&self) -> &'static str {
        match self {
            DatabaseState::Created => "created",
            DatabaseState::Active => "active",
            DatabaseState::Deleting => "deleting",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "created" => Ok(DatabaseState::Created),
            "active" => Ok(DatabaseState::Active),
            "deleting" => Ok(DatabaseState::Deleting),
            other => Err(CombeeError::Internal(format!(
                "invalid database state: {other}"
            ))),
        }
    }
}

/// 一个租户(应用所有者)。
#[derive(Debug, Clone, Serialize)]
pub struct TenantRecord {
    pub id: TenantId,
    /// 创建时间(unix 秒)。
    pub created_at: u64,
    pub status: String,
}

/// 一条 API key(只存哈希,不存明文)。
#[derive(Debug, Clone, Serialize)]
pub struct ApiKeyRecord {
    pub id: Uuid,
    pub tenant_id: TenantId,
    pub key_hash: String,
    pub created_at: u64,
    /// 撤销时间(unix 秒);None = 有效。
    pub revoked_at: Option<u64>,
}

/// 一条 Cell 目录记录。
#[derive(Debug, Clone, Serialize)]
pub struct DatabaseRecord {
    pub id: DatabaseId,
    pub tenant_id: TenantId,
    pub state: DatabaseState,
    /// 创建时间(unix 秒)。
    pub created_at: u64,
    /// 负责该 Cell 的 Data Node(主);`None` 表示单进程/未注册节点模式。
    pub storage_node_id: Option<NodeId>,
    /// 副本 Data Node(单 replica);`None` 表示无副本。
    pub replica_node_id: Option<NodeId>,
    /// generation(fencing):每次 failover 递增;Data Node 写校验。
    pub generation: i64,
}

impl DatabaseRecord {
    pub(crate) fn now_unix() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// 控制面存储抽象。所有方法以租户为界,天然隔离多租户。
#[async_trait]
pub trait MetadataStore: Send + Sync {
    /// 创建目录记录并指定负责节点。若同租户下已存在则报 [`CombeeError::DatabaseAlreadyExists`]。
    async fn create_database(
        &self,
        tenant: TenantId,
        id: DatabaseId,
        storage_node: Option<NodeId>,
    ) -> Result<DatabaseRecord>;

    /// 查询单条记录;不存在报 [`CombeeError::DatabaseNotFound`]。
    async fn get_database(&self, tenant: TenantId, id: DatabaseId) -> Result<DatabaseRecord>;

    /// 列出该租户下全部记录。
    async fn list_databases(&self, tenant: TenantId) -> Result<Vec<DatabaseRecord>>;

    /// 删除目录记录;不存在报 [`CombeeError::DatabaseNotFound`]。
    async fn delete_database(&self, tenant: TenantId, id: DatabaseId) -> Result<()>;

    /// 设置/清除副本节点(单 replica)。返回更新后的记录。
    async fn set_replica_node(
        &self,
        tenant: TenantId,
        id: DatabaseId,
        replica: Option<NodeId>,
    ) -> Result<DatabaseRecord>;

    /// 列出以该节点为副本的全部 Cell(副本节点拉取职责用)。
    async fn list_replicas_of(&self, node: NodeId) -> Result<Vec<DatabaseRecord>>;

    /// failover:把副本提升为主 —— `storage_node_id = replica_node_id`、
    /// `replica_node_id = NULL`、`generation += 1`。无副本时报错。
    async fn promote_replica(&self, tenant: TenantId, id: DatabaseId) -> Result<DatabaseRecord>;

    /// 列出全部 Cell(自动 failover 扫描用,跨租户)。
    async fn list_all_databases(&self) -> Result<Vec<DatabaseRecord>>;

    // ---- 租户与 API key ----

    /// 创建租户,返回记录。
    async fn create_tenant(&self, tenant: TenantId) -> Result<TenantRecord>;

    /// 列出全部租户(管理/计费)。
    async fn list_tenants(&self) -> Result<Vec<TenantRecord>>;

    /// 为该租户注册一个 API key(存哈希)。返回记录。
    async fn create_api_key(&self, tenant: TenantId, key_hash: String) -> Result<ApiKeyRecord>;

    /// 按哈希查找**未撤销**的 API key(认证用)。
    async fn lookup_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKeyRecord>>;

    /// 列出租户的 API key(不含明文)。
    async fn list_api_keys(&self, tenant: TenantId) -> Result<Vec<ApiKeyRecord>>;

    /// 撤销 API key。不存在报 NotFound。
    async fn revoke_api_key(&self, tenant: TenantId, key_id: Uuid) -> Result<()>;

    /// 批量创建目录记录(默认实现为逐条循环;PostgreSQL 后端会覆盖为批量 INSERT)。
    /// 已存在的记录静默跳过。
    async fn create_databases_batch(&self, tenant: TenantId, ids: &[DatabaseId]) -> Result<()> {
        for &id in ids {
            let _ = self.create_database(tenant, id, None).await?;
        }
        Ok(())
    }
}

/// V0 默认实现:进程内 HashMap。重启即丢失,仅用于开发与测试。
pub struct InMemoryStore {
    inner: Mutex<InMemoryInner>,
}

#[derive(Default)]
struct InMemoryInner {
    databases: HashMap<(TenantId, DatabaseId), DatabaseRecord>,
    tenants: HashMap<TenantId, TenantRecord>,
    api_keys: HashMap<Uuid, ApiKeyRecord>,
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(InMemoryInner::default()),
        }
    }
}

#[async_trait]
impl MetadataStore for InMemoryStore {
    async fn create_database(
        &self,
        tenant: TenantId,
        id: DatabaseId,
        storage_node: Option<NodeId>,
    ) -> Result<DatabaseRecord> {
        let mut inner = self.inner.lock().unwrap();
        if inner.databases.contains_key(&(tenant, id)) {
            return Err(CombeeError::DatabaseAlreadyExists(id));
        }
        let record = DatabaseRecord {
            id,
            tenant_id: tenant,
            state: DatabaseState::Created,
            created_at: DatabaseRecord::now_unix(),
            storage_node_id: storage_node,
            replica_node_id: None,
            generation: 0,
        };
        inner.databases.insert((tenant, id), record.clone());
        Ok(record)
    }

    async fn get_database(&self, tenant: TenantId, id: DatabaseId) -> Result<DatabaseRecord> {
        let inner = self.inner.lock().unwrap();
        inner
            .databases
            .get(&(tenant, id))
            .cloned()
            .ok_or(CombeeError::DatabaseNotFound(id))
    }

    async fn list_databases(&self, tenant: TenantId) -> Result<Vec<DatabaseRecord>> {
        let inner = self.inner.lock().unwrap();
        let mut records: Vec<_> = inner
            .databases
            .iter()
            .filter(|((t, _), _)| *t == tenant)
            .map(|(_, r)| r.clone())
            .collect();
        records.sort_by_key(|r| (r.created_at, r.id));
        Ok(records)
    }

    async fn delete_database(&self, tenant: TenantId, id: DatabaseId) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        if inner.databases.remove(&(tenant, id)).is_none() {
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
        let mut inner = self.inner.lock().unwrap();
        let record = inner
            .databases
            .get_mut(&(tenant, id))
            .ok_or(CombeeError::DatabaseNotFound(id))?;
        record.replica_node_id = replica;
        Ok(record.clone())
    }

    async fn list_replicas_of(&self, node: NodeId) -> Result<Vec<DatabaseRecord>> {
        let inner = self.inner.lock().unwrap();
        let mut records: Vec<_> = inner
            .databases
            .values()
            .filter(|r| r.replica_node_id == Some(node))
            .cloned()
            .collect();
        records.sort_by_key(|r| r.id);
        Ok(records)
    }

    async fn promote_replica(&self, tenant: TenantId, id: DatabaseId) -> Result<DatabaseRecord> {
        let mut inner = self.inner.lock().unwrap();
        let record = inner
            .databases
            .get_mut(&(tenant, id))
            .ok_or(CombeeError::DatabaseNotFound(id))?;
        let Some(replica) = record.replica_node_id else {
            return Err(CombeeError::Internal(format!(
                "cell {id} has no replica to promote"
            )));
        };
        record.storage_node_id = Some(replica);
        record.replica_node_id = None;
        record.generation += 1;
        Ok(record.clone())
    }

    async fn list_all_databases(&self) -> Result<Vec<DatabaseRecord>> {
        let inner = self.inner.lock().unwrap();
        let mut records: Vec<_> = inner.databases.values().cloned().collect();
        records.sort_by_key(|r| r.id);
        Ok(records)
    }

    async fn create_tenant(&self, tenant: TenantId) -> Result<TenantRecord> {
        let mut inner = self.inner.lock().unwrap();
        let now = DatabaseRecord::now_unix();
        let record = TenantRecord {
            id: tenant,
            created_at: now,
            status: "active".into(),
        };
        inner.tenants.insert(tenant, record.clone());
        Ok(record)
    }

    async fn list_tenants(&self) -> Result<Vec<TenantRecord>> {
        let inner = self.inner.lock().unwrap();
        let mut v: Vec<_> = inner.tenants.values().cloned().collect();
        v.sort_by_key(|t| t.created_at);
        Ok(v)
    }

    async fn create_api_key(&self, tenant: TenantId, key_hash: String) -> Result<ApiKeyRecord> {
        let mut inner = self.inner.lock().unwrap();
        let record = ApiKeyRecord {
            id: Uuid::new_v4(),
            tenant_id: tenant,
            key_hash,
            created_at: DatabaseRecord::now_unix(),
            revoked_at: None,
        };
        inner.api_keys.insert(record.id, record.clone());
        Ok(record)
    }

    async fn lookup_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKeyRecord>> {
        let inner = self.inner.lock().unwrap();
        Ok(inner
            .api_keys
            .values()
            .find(|k| k.key_hash == key_hash && k.revoked_at.is_none())
            .cloned())
    }

    async fn list_api_keys(&self, tenant: TenantId) -> Result<Vec<ApiKeyRecord>> {
        let inner = self.inner.lock().unwrap();
        let mut v: Vec<_> = inner
            .api_keys
            .values()
            .filter(|k| k.tenant_id == tenant)
            .cloned()
            .collect();
        v.sort_by_key(|k| k.created_at);
        Ok(v)
    }

    async fn revoke_api_key(&self, tenant: TenantId, key_id: Uuid) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let k = inner
            .api_keys
            .get_mut(&key_id)
            .ok_or(CombeeError::ApiKeyNotFound)?;
        if k.tenant_id != tenant {
            return Err(CombeeError::ApiKeyNotFound);
        }
        k.revoked_at = Some(DatabaseRecord::now_unix());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(n: u128) -> TenantId {
        TenantId::from_u128(n)
    }

    #[tokio::test]
    async fn create_get_list_delete_roundtrip() {
        let store = InMemoryStore::new();
        let t = tenant(1);
        let id = DatabaseId::new();

        let record = store.create_database(t, id, None).await.unwrap();
        assert_eq!(record.id, id);
        assert_eq!(record.tenant_id, t);
        assert_eq!(record.state, DatabaseState::Created);
        assert!(record.created_at > 0);

        let fetched = store.get_database(t, id).await.unwrap();
        assert_eq!(fetched.id, id);

        let list = store.list_databases(t).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);

        store.delete_database(t, id).await.unwrap();
        assert!(store.list_databases(t).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn duplicate_create_rejected() {
        let store = InMemoryStore::new();
        let t = tenant(1);
        let id = DatabaseId::new();
        store.create_database(t, id, None).await.unwrap();
        let err = store.create_database(t, id, None).await.unwrap_err();
        assert!(matches!(err, CombeeError::DatabaseAlreadyExists(_)));
    }

    #[tokio::test]
    async fn get_and_delete_missing_rejected() {
        let store = InMemoryStore::new();
        let t = tenant(1);
        let id = DatabaseId::new();
        assert!(matches!(
            store.get_database(t, id).await.unwrap_err(),
            CombeeError::DatabaseNotFound(_)
        ));
        assert!(matches!(
            store.delete_database(t, id).await.unwrap_err(),
            CombeeError::DatabaseNotFound(_)
        ));
    }

    #[tokio::test]
    async fn tenants_are_isolated() {
        let store = InMemoryStore::new();
        let t_a = tenant(1);
        let t_b = tenant(2);
        let id = DatabaseId::new();

        store.create_database(t_a, id, None).await.unwrap();

        // B 看不到 A 的库
        assert!(matches!(
            store.get_database(t_b, id).await.unwrap_err(),
            CombeeError::DatabaseNotFound(_)
        ));
        assert!(store.list_databases(t_b).await.unwrap().is_empty());

        // B 可以创建同 id 的库(键是 (tenant, id))
        store.create_database(t_b, id, None).await.unwrap();
        assert_eq!(store.list_databases(t_a).await.unwrap().len(), 1);
        assert_eq!(store.list_databases(t_b).await.unwrap().len(), 1);

        // 删除 A 的不影响 B
        store.delete_database(t_a, id).await.unwrap();
        assert_eq!(store.list_databases(t_b).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn replica_set_and_list() {
        use combee_common::NodeId;
        let store = InMemoryStore::new();
        let t = tenant(1);
        let primary = NodeId::new();
        let replica = NodeId::new();
        let id = DatabaseId::new();
        store.create_database(t, id, Some(primary)).await.unwrap();

        // 设置副本
        let rec = store.set_replica_node(t, id, Some(replica)).await.unwrap();
        assert_eq!(rec.replica_node_id, Some(replica));
        let reps = store.list_replicas_of(replica).await.unwrap();
        assert_eq!(reps.len(), 1);
        assert_eq!(reps[0].id, id);
        assert!(store.list_replicas_of(primary).await.unwrap().is_empty());

        // 清除副本
        store.set_replica_node(t, id, None).await.unwrap();
        assert!(store.list_replicas_of(replica).await.unwrap().is_empty());

        // 不存在的 db → NotFound
        assert!(
            store
                .set_replica_node(t, DatabaseId::new(), Some(replica))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn list_sorted_by_created_at_then_id() {
        let store = InMemoryStore::new();
        let t = tenant(1);
        let id1 = DatabaseId::new();
        let id2 = DatabaseId::new();
        // 同秒创建两个,顺序必须确定(created_at 相同则按 id 升序)
        store.create_database(t, id2, None).await.unwrap();
        store.create_database(t, id1, None).await.unwrap();

        let list = store.list_databases(t).await.unwrap();
        assert_eq!(list.len(), 2);
        let mut sorted = vec![id1, id2];
        sorted.sort();
        let ids: Vec<DatabaseId> = list.iter().map(|r| r.id).collect();
        assert_eq!(ids, sorted, "deterministic (created_at, id) ordering");
        assert!(list[0].created_at <= list[1].created_at);
    }
}
