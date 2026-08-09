//! MetadataStore 抽象与 V0 的 InMemory 实现。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use combee_common::credit::{
    CreditAccount, CreditTransaction, CreditTransactionType, CreditVoucher, PricingRule,
    PricingStatus, PricingVersion, VoucherStatus,
};
use combee_common::usage::{UsageBucket, UsageKey, UsageMetric};
use combee_common::{CombeeError, DatabaseId, NodeId, Result, TenantId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// V0 尚未接入真实认证,所有数据库归属这一个内置租户。
/// 后续接入用户系统后,由 API key 映射到真实 tenant。
pub const DEFAULT_TENANT: TenantId = TenantId::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0001);

/// Cell 生命周期状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(utoipa::ToSchema, Debug, Clone, Serialize)]
pub struct TenantRecord {
    pub id: TenantId,
    /// 创建时间(unix 秒)。
    pub created_at: u64,
    pub status: String,
}

/// 一条 API key(只存哈希,不存明文)。
#[derive(utoipa::ToSchema, Debug, Clone, Serialize)]
pub struct ApiKeyRecord {
    pub id: Uuid,
    pub tenant_id: TenantId,
    pub name: String,
    pub key_hash: String,
    pub created_at: u64,
    /// 撤销时间(unix 秒);None = 有效。
    pub revoked_at: Option<u64>,
}

/// 一条 Data Node 注册记录(共享 authority,多 API 副本可见)。
#[derive(Debug, Clone, Serialize)]
pub struct DataNodeRecord {
    pub id: NodeId,
    pub addr: String,
    pub capacity: usize,
    pub active_conns: usize,
    /// 最近心跳(unix 秒)。
    pub last_heartbeat_at: u64,
    pub created_at: u64,
}

/// 一条 Cell 目录记录。
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DatabaseRecord {
    pub id: DatabaseId,
    /// 租户内唯一的人类可读名;`create` 未提供时生成 `cell-<short-id>`。
    pub name: String,
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
        name: Option<&str>,
    ) -> Result<DatabaseRecord>;

    /// 按名查询;不存在报 [`CombeeError::DatabaseNotFound`]。
    async fn get_database_by_name(&self, tenant: TenantId, name: &str) -> Result<DatabaseRecord>;

    /// 幂等 ensure:不存在则创建,存在则复用;并发安全(数据库唯一约束兜底)。
    /// 返回 (record, created)。
    async fn ensure_database_by_name(
        &self,
        tenant: TenantId,
        name: &str,
        storage_node: Option<NodeId>,
    ) -> Result<(DatabaseRecord, bool)>;

    /// 重命名(租户内唯一);冲突报 [`CombeeError::CellNameConflict`]。
    async fn rename_database(
        &self,
        tenant: TenantId,
        id: DatabaseId,
        new_name: &str,
    ) -> Result<DatabaseRecord>;

    /// 重置:保留 id/name,generation +1,清空数据(data-node 删文件)。
    async fn reset_database(&self, tenant: TenantId, id: DatabaseId) -> Result<DatabaseRecord>;

    /// 查询单条记录;不存在报 [`CombeeError::DatabaseNotFound`]。
    async fn get_database(&self, tenant: TenantId, id: DatabaseId) -> Result<DatabaseRecord>;

    /// 按 id 查询(仅供已通过租户校验后的数据节点路由使用;
    /// 不要用作公开数据访问入口,租户隔离由调用方先行校验保证)。
    async fn get_database_by_id(&self, id: DatabaseId) -> Result<DatabaseRecord>;

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
    /// 启动时注入预配置的 API key(COMBEE_API_KEYS / COMBEE_ADMIN_API_KEY);
    /// 每个 key 若不存在则创建独立租户 + key 记录;已存在则跳过。
    async fn bootstrap_api_keys(&self, keys: &[String]) -> Result<()>;

    async fn create_api_key(
        &self,
        tenant: TenantId,
        key_hash: String,
        name: &str,
    ) -> Result<ApiKeyRecord>;

    /// 按哈希查找**未撤销**的 API key(认证用)。
    async fn lookup_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKeyRecord>>;

    /// 列出租户的 API key(不含明文)。
    async fn list_api_keys(&self, tenant: TenantId) -> Result<Vec<ApiKeyRecord>>;

    /// 撤销 API key。不存在报 NotFound。
    async fn revoke_api_key(&self, tenant: TenantId, key_id: Uuid) -> Result<()>;

    // ---- Data Node 注册表(共享 authority;Postgres 模式下多 API 副本共享)----
    /// 注册或更新节点(幂等;address/capacity/last_heartbeat 更新)。
    async fn upsert_data_node(
        &self,
        id: NodeId,
        addr: String,
        capacity: usize,
    ) -> Result<()>;

    /// 上报心跳。未知节点返回 false。
    async fn heartbeat_data_node(&self, id: NodeId, active_conns: usize) -> Result<bool>;

    /// 注销节点。不存在返回 false。
    async fn unregister_data_node(&self, id: NodeId) -> Result<bool>;

    /// 全部节点记录(含心跳时间,用于健康判定)。
    async fn list_data_nodes(&self) -> Result<Vec<DataNodeRecord>>;

    // ---- Usage Metering ----

    /// 累加用量(幂等键:(tenant, cell, metric, bucket));同一键的多次 add 正确累加。
    async fn usage_add(&self, key: &UsageKey, delta: u64) -> Result<()>;

    /// 覆盖快照类指标(如 storage_bytes):同键写入新值。
    async fn usage_set(&self, key: &UsageKey, value: u64) -> Result<()>;

    /// 查询用量桶(时间闭区间,按 bucket_start 升序)。
    async fn query_usage(
        &self,
        tenant: TenantId,
        cell: Option<DatabaseId>,
        metric: Option<UsageMetric>,
        from_bucket: i64,
        to_bucket: i64,
    ) -> Result<Vec<UsageBucket>>;

    // ---- Pricing ----

    /// 创建新定价版本并激活(旧 active 自动置 inactive)。返回版本号。
    async fn create_pricing_version(&self, rules: Vec<PricingRule>) -> Result<PricingVersion>;

    /// 当前 active 定价;无配置时返回 (version 0, 空规则)。
    async fn get_active_pricing(&self) -> Result<(PricingVersion, Vec<PricingRule>)>;

    /// 全部定价版本(管理用)。
    async fn list_pricing_versions(&self) -> Result<Vec<PricingVersion>>;

    // ---- Credits ----

    /// 租户余额账户(不存在则创建 0 余额)。
    async fn get_credit_account(&self, tenant: TenantId) -> Result<CreditAccount>;

    /// 账本(倒序,limit;before 用于游标分页,取 id < before 的记录)。
    async fn list_credit_transactions(
        &self,
        tenant: TenantId,
        limit: i64,
        before: Option<uuid::Uuid>,
    ) -> Result<Vec<CreditTransaction>>;

    /// 按幂等引用查找(usage 结算 / voucher 兑换判重)。
    async fn find_transaction_by_reference(
        &self,
        reference_id: &str,
    ) -> Result<Option<CreditTransaction>>;

    /// 追加账本条目并更新余额(原子)。reference_id 冲突(已存在)时不重复入账,
    /// 返回已存在条目。返回值为入账后的完整条目(含 balance_after)。
    async fn append_credit_transaction(&self, txn: CreditTransaction) -> Result<CreditTransaction>;

    // ---- Voucher ----

    /// 批量生成兑换券(存哈希),返回含明文 code 的列表(明文仅生成时可见)。
    async fn create_vouchers(
        &self,
        amount_units: i64,
        count: u32,
        campaign: Option<String>,
        expires_at: Option<i64>,
    ) -> Result<Vec<(String, CreditVoucher)>>;

    /// 原子兑换:active 且未过期 → 置 used + 入账 + 返回金额;否则 Err。
    async fn redeem_voucher(&self, code_hash: &str, tenant: TenantId, now: i64) -> Result<i64>;

    /// 全部兑换券(管理用,不含明文)。
    async fn list_vouchers(&self, limit: i64) -> Result<Vec<CreditVoucher>>;

    // ---- Idempotency ----

    /// Idempotency-Key 原子保存:插入成功返回 `None`;键已存在返回已存 payload(重放)。
    async fn save_idempotency(
        &self,
        key: &str,
        tenant: TenantId,
        payload: String,
    ) -> Result<Option<String>>;

    /// 登记 Public Beta 候补邮箱(重复邮箱幂等)。
    async fn create_waitlist_entry(&self, email: &str, now: i64) -> Result<()>;

    /// 列出候补邮箱(管理用,按登记时间升序)。
    async fn list_waitlist(&self, limit: i64) -> Result<Vec<WaitlistEntry>>;

    /// 批量创建目录记录(默认实现为逐条循环;PostgreSQL 后端会覆盖为批量 INSERT)。
    /// 已存在的记录静默跳过。
    async fn create_databases_batch(&self, tenant: TenantId, ids: &[DatabaseId]) -> Result<()> {
        for &id in ids {
            let _ = self.create_database(tenant, id, None, None).await?;
        }
        Ok(())
    }
}

/// V0 默认实现:进程内 HashMap。重启即丢失,仅用于开发与测试。
pub struct InMemoryStore {
    inner: Mutex<InMemoryInner>,
}

/// Public Beta 候补邮箱条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitlistEntry {
    pub email: String,
    pub created_at: i64,
}

#[derive(Default)]
struct InMemoryInner {
    databases: HashMap<(TenantId, DatabaseId), DatabaseRecord>,
    tenants: HashMap<TenantId, TenantRecord>,
    api_keys: HashMap<Uuid, ApiKeyRecord>,
    data_nodes: HashMap<NodeId, DataNodeRecord>,
    usage: HashMap<UsageKey, u64>,
    pricing_versions: HashMap<i64, PricingVersion>,
    pricing_rules: HashMap<i64, Vec<PricingRule>>,
    credit_accounts: HashMap<TenantId, CreditAccount>,
    credit_transactions: HashMap<uuid::Uuid, CreditTransaction>,
    credit_txn_by_ref: HashMap<String, uuid::Uuid>,
    vouchers: HashMap<uuid::Uuid, CreditVoucher>,
    voucher_by_hash: HashMap<String, uuid::Uuid>,
    idempotency: HashMap<String, (TenantId, String)>,
    waitlist: Vec<WaitlistEntry>,
    /// (tenant, name) → id;保证租户内名字唯一。
    name_index: HashMap<(TenantId, String), DatabaseId>,
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
        name: Option<&str>,
    ) -> Result<DatabaseRecord> {
        let mut inner = self.inner.lock().unwrap();
        if inner.databases.contains_key(&(tenant, id)) {
            return Err(CombeeError::DatabaseAlreadyExists(id));
        }
        let name = match name {
            Some(n) => {
                if inner.name_index.contains_key(&(tenant, n.to_string())) {
                    return Err(CombeeError::CellAlreadyExists(n.to_string()));
                }
                n.to_string()
            }
            None => format!(
                "cell-{}",
                id.to_string().replace('-', "").get(..8).unwrap_or("")
            ),
        };
        let record = DatabaseRecord {
            id,
            tenant_id: tenant,
            name,
            state: DatabaseState::Created,
            created_at: DatabaseRecord::now_unix(),
            storage_node_id: storage_node,
            replica_node_id: None,
            generation: 0,
        };
        inner.name_index.insert((tenant, record.name.clone()), id);
        inner.databases.insert((tenant, id), record.clone());
        Ok(record)
    }

    async fn get_database_by_name(&self, tenant: TenantId, name: &str) -> Result<DatabaseRecord> {
        let inner = self.inner.lock().unwrap();
        let id = inner
            .name_index
            .get(&(tenant, name.to_string()))
            .ok_or(CombeeError::DatabaseNotFound(DatabaseId::new()))?;
        inner
            .databases
            .get(&(tenant, *id))
            .cloned()
            .ok_or(CombeeError::DatabaseNotFound(*id))
    }

    async fn ensure_database_by_name(
        &self,
        tenant: TenantId,
        name: &str,
        storage_node: Option<NodeId>,
    ) -> Result<(DatabaseRecord, bool)> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(id) = inner.name_index.get(&(tenant, name.to_string())) {
            let rec = inner
                .databases
                .get(&(tenant, *id))
                .cloned()
                .ok_or(CombeeError::DatabaseNotFound(*id))?;
            return Ok((rec, false));
        }
        let id = DatabaseId::new();
        let record = DatabaseRecord {
            id,
            tenant_id: tenant,
            name: name.to_string(),
            state: DatabaseState::Created,
            created_at: DatabaseRecord::now_unix(),
            storage_node_id: storage_node,
            replica_node_id: None,
            generation: 0,
        };
        inner.name_index.insert((tenant, name.to_string()), id);
        inner.databases.insert((tenant, id), record.clone());
        Ok((record, true))
    }

    async fn rename_database(
        &self,
        tenant: TenantId,
        id: DatabaseId,
        new_name: &str,
    ) -> Result<DatabaseRecord> {
        let mut inner = self.inner.lock().unwrap();
        let rec = inner
            .databases
            .get(&(tenant, id))
            .ok_or(CombeeError::DatabaseNotFound(id))?;
        let old_name = rec.name.clone();
        if inner
            .name_index
            .contains_key(&(tenant, new_name.to_string()))
        {
            return Err(CombeeError::CellNameConflict(new_name.to_string()));
        }
        let mut rec = rec.clone();
        rec.name = new_name.to_string();
        inner.name_index.remove(&(tenant, old_name));
        inner.name_index.insert((tenant, new_name.to_string()), id);
        inner.databases.insert((tenant, id), rec.clone());
        Ok(rec)
    }

    async fn reset_database(&self, tenant: TenantId, id: DatabaseId) -> Result<DatabaseRecord> {
        let mut inner = self.inner.lock().unwrap();
        let mut rec = inner
            .databases
            .get(&(tenant, id))
            .ok_or(CombeeError::DatabaseNotFound(id))?
            .clone();
        rec.generation += 1;
        inner.databases.insert((tenant, id), rec.clone());
        Ok(rec)
    }

    async fn get_database(&self, tenant: TenantId, id: DatabaseId) -> Result<DatabaseRecord> {
        let inner = self.inner.lock().unwrap();
        inner
            .databases
            .get(&(tenant, id))
            .cloned()
            .ok_or(CombeeError::DatabaseNotFound(id))
    }

    async fn get_database_by_id(&self, id: DatabaseId) -> Result<DatabaseRecord> {
        let inner = self.inner.lock().unwrap();
        inner
            .databases
            .values()
            .find(|rec| rec.id == id)
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
        // 同步清理名字索引,保证删除后 ensure 同名重建
        inner.name_index.retain(|_, v| *v != id);
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

    async fn create_api_key(
        &self,
        tenant: TenantId,
        key_hash: String,
        name: &str,
    ) -> Result<ApiKeyRecord> {
        let mut inner = self.inner.lock().unwrap();
        let record = ApiKeyRecord {
            id: Uuid::new_v4(),
            tenant_id: tenant,
            name: name.to_string(),
            key_hash,
            created_at: DatabaseRecord::now_unix(),
            revoked_at: None,
        };
        inner.api_keys.insert(record.id, record.clone());
        Ok(record)
    }

    async fn bootstrap_api_keys(&self, keys: &[String]) -> Result<()> {
        for key in keys {
            let hash = combee_common::api_key::hash(key);
            if self.lookup_api_key_by_hash(&hash).await?.is_some() {
                continue;
            }
            let tenant = TenantId::new();
            self.create_tenant(tenant).await?;
            self.create_api_key(tenant, hash, "bootstrap").await?;
        }
        Ok(())
    }

    async fn upsert_data_node(&self, id: NodeId, addr: String, capacity: usize) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let now = DatabaseRecord::now_unix();
        match inner.data_nodes.get_mut(&id) {
            Some(rec) => {
                rec.addr = addr;
                rec.capacity = capacity;
                rec.last_heartbeat_at = now;
            }
            None => {
                inner.data_nodes.insert(
                    id,
                    DataNodeRecord {
                        id,
                        addr,
                        capacity,
                        active_conns: 0,
                        last_heartbeat_at: now,
                        created_at: now,
                    },
                );
            }
        }
        Ok(())
    }

    async fn heartbeat_data_node(&self, id: NodeId, active_conns: usize) -> Result<bool> {
        let mut inner = self.inner.lock().unwrap();
        match inner.data_nodes.get_mut(&id) {
            Some(rec) => {
                rec.active_conns = active_conns;
                rec.last_heartbeat_at = DatabaseRecord::now_unix();
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn unregister_data_node(&self, id: NodeId) -> Result<bool> {
        let mut inner = self.inner.lock().unwrap();
        Ok(inner.data_nodes.remove(&id).is_some())
    }

    async fn list_data_nodes(&self) -> Result<Vec<DataNodeRecord>> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.data_nodes.values().cloned().collect())
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

    async fn usage_add(&self, key: &UsageKey, delta: u64) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        *inner.usage.entry(key.clone()).or_insert(0) += delta;
        Ok(())
    }

    async fn usage_set(&self, key: &UsageKey, value: u64) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.usage.insert(key.clone(), value);
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
        let inner = self.inner.lock().unwrap();
        let mut out: Vec<UsageBucket> = inner
            .usage
            .iter()
            .filter(|(k, _)| {
                k.tenant_id == tenant
                    && cell.map(|c| k.cell_id == Some(c)).unwrap_or(true)
                    && metric.map(|m| k.metric == m).unwrap_or(true)
                    && k.bucket_start >= from_bucket
                    && k.bucket_start <= to_bucket
            })
            .map(|(k, v)| UsageBucket {
                tenant_id: k.tenant_id,
                cell_id: k.cell_id,
                metric: k.metric,
                bucket_start: k.bucket_start,
                value: *v,
            })
            .collect();
        out.sort_by_key(|b| (b.bucket_start, b.metric.as_str().to_string(), b.cell_id));
        Ok(out)
    }

    async fn create_pricing_version(&self, rules: Vec<PricingRule>) -> Result<PricingVersion> {
        let mut inner = self.inner.lock().unwrap();
        let version = inner.pricing_versions.keys().max().copied().unwrap_or(0) + 1;
        let now = DatabaseRecord::now_unix() as i64;
        // 旧 active → inactive
        for v in inner.pricing_versions.values_mut() {
            v.status = PricingStatus::Inactive;
        }
        let rec = PricingVersion {
            version,
            status: PricingStatus::Active,
            effective_at: now,
            created_at: now,
        };
        inner.pricing_versions.insert(version, rec.clone());
        inner.pricing_rules.insert(version, rules);
        Ok(rec)
    }

    async fn get_active_pricing(&self) -> Result<(PricingVersion, Vec<PricingRule>)> {
        let inner = self.inner.lock().unwrap();
        if let Some((v, rec)) = inner
            .pricing_versions
            .iter()
            .find(|(_, r)| r.status == PricingStatus::Active)
            .map(|(v, r)| (*v, r.clone()))
        {
            Ok((
                rec,
                inner.pricing_rules.get(&v).cloned().unwrap_or_default(),
            ))
        } else {
            Ok((
                PricingVersion {
                    version: 0,
                    status: PricingStatus::Active,
                    effective_at: 0,
                    created_at: 0,
                },
                vec![],
            ))
        }
    }

    async fn list_pricing_versions(&self) -> Result<Vec<PricingVersion>> {
        let inner = self.inner.lock().unwrap();
        let mut vs: Vec<_> = inner.pricing_versions.values().cloned().collect();
        vs.sort_by_key(|v| v.version);
        Ok(vs)
    }

    async fn get_credit_account(&self, tenant: TenantId) -> Result<CreditAccount> {
        let mut inner = self.inner.lock().unwrap();
        Ok(inner
            .credit_accounts
            .entry(tenant)
            .or_insert_with(|| CreditAccount {
                tenant_id: tenant,
                balance_units: 0,
                reserved_units: 0,
                updated_at: DatabaseRecord::now_unix() as i64,
            })
            .clone())
    }

    async fn list_credit_transactions(
        &self,
        tenant: TenantId,
        limit: i64,
        before: Option<uuid::Uuid>,
    ) -> Result<Vec<CreditTransaction>> {
        let inner = self.inner.lock().unwrap();
        let mut txns: Vec<_> = inner
            .credit_transactions
            .values()
            .filter(|t| t.tenant_id == tenant && before.map(|b| t.id < b).unwrap_or(true))
            .cloned()
            .collect();
        txns.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        txns.truncate(limit.clamp(1, 1000) as usize);
        Ok(txns)
    }

    async fn find_transaction_by_reference(
        &self,
        reference_id: &str,
    ) -> Result<Option<CreditTransaction>> {
        let inner = self.inner.lock().unwrap();
        Ok(inner
            .credit_txn_by_ref
            .get(reference_id)
            .and_then(|id| inner.credit_transactions.get(id))
            .cloned())
    }

    async fn append_credit_transaction(
        &self,
        mut txn: CreditTransaction,
    ) -> Result<CreditTransaction> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(ref_id) = &txn.reference_id
            && let Some(existing) = inner
                .credit_txn_by_ref
                .get(ref_id)
                .and_then(|id| inner.credit_transactions.get(id))
                .cloned()
        {
            return Ok(existing); // 幂等:已入账
        }
        let account = inner
            .credit_accounts
            .entry(txn.tenant_id)
            .or_insert_with(|| CreditAccount {
                tenant_id: txn.tenant_id,
                balance_units: 0,
                reserved_units: 0,
                updated_at: DatabaseRecord::now_unix() as i64,
            });
        account.balance_units = account.balance_units.saturating_add(txn.amount_units);
        account.updated_at = DatabaseRecord::now_unix() as i64;
        txn.balance_after = Some(account.balance_units);
        let id = txn.id;
        if let Some(ref_id) = txn.reference_id.clone() {
            inner.credit_txn_by_ref.insert(ref_id, id);
        }
        inner.credit_transactions.insert(id, txn.clone());
        Ok(txn)
    }

    async fn create_vouchers(
        &self,
        amount_units: i64,
        count: u32,
        campaign: Option<String>,
        expires_at: Option<i64>,
    ) -> Result<Vec<(String, CreditVoucher)>> {
        let mut inner = self.inner.lock().unwrap();
        let mut out = Vec::new();
        for _ in 0..count {
            let code = combee_common::credit::generate_voucher_code();
            let code_hash = combee_common::credit::hash_voucher_code(&code);
            let v = CreditVoucher {
                id: uuid::Uuid::new_v4(),
                code_hash: code_hash.clone(),
                amount_units,
                status: VoucherStatus::Active,
                campaign: campaign.clone(),
                created_at: DatabaseRecord::now_unix() as i64,
                expires_at,
                redeemed_by: None,
                redeemed_at: None,
            };
            inner.voucher_by_hash.insert(code_hash, v.id);
            inner.vouchers.insert(v.id, v.clone());
            out.push((code, v));
        }
        Ok(out)
    }

    async fn redeem_voucher(&self, code_hash: &str, tenant: TenantId, now: i64) -> Result<i64> {
        let mut inner = self.inner.lock().unwrap();
        let id = inner
            .voucher_by_hash
            .get(code_hash)
            .cloned()
            .ok_or(CombeeError::InvalidRequest("voucher not found".into()))?;
        let voucher = inner
            .vouchers
            .get_mut(&id)
            .ok_or(CombeeError::InvalidRequest("voucher not found".into()))?;
        if voucher.status != VoucherStatus::Active {
            return Err(CombeeError::InvalidRequest(
                "voucher already used or revoked".into(),
            ));
        }
        if voucher.expires_at.map(|e| e < now).unwrap_or(false) {
            return Err(CombeeError::InvalidRequest("voucher expired".into()));
        }
        let amount = voucher.amount_units;
        voucher.status = VoucherStatus::Used;
        voucher.redeemed_by = Some(tenant);
        voucher.redeemed_at = Some(now);
        let account = inner
            .credit_accounts
            .entry(tenant)
            .or_insert_with(|| CreditAccount {
                tenant_id: tenant,
                balance_units: 0,
                reserved_units: 0,
                updated_at: DatabaseRecord::now_unix() as i64,
            });
        account.balance_units = account.balance_units.saturating_add(amount);
        account.updated_at = now;
        let txn = CreditTransaction {
            id: uuid::Uuid::new_v4(),
            tenant_id: tenant,
            txn_type: CreditTransactionType::Voucher,
            amount_units: amount,
            pricing_version: None,
            reference_id: Some(format!("voucher:{code_hash}")),
            description: Some("voucher redemption".into()),
            created_at: now,
            balance_after: Some(account.balance_units),
        };
        inner
            .credit_txn_by_ref
            .insert("voucher:".to_string() + code_hash, txn.id);
        inner.credit_transactions.insert(txn.id, txn);
        Ok(amount)
    }

    async fn list_vouchers(&self, limit: i64) -> Result<Vec<CreditVoucher>> {
        let inner = self.inner.lock().unwrap();
        let mut vs: Vec<_> = inner.vouchers.values().cloned().collect();
        vs.sort_by_key(|v| v.created_at);
        vs.truncate(limit.clamp(1, 1000) as usize);
        Ok(vs)
    }

    async fn save_idempotency(
        &self,
        key: &str,
        tenant: TenantId,
        payload: String,
    ) -> Result<Option<String>> {
        let mut inner = self.inner.lock().unwrap();
        if let Some((_, existing)) = inner.idempotency.get(key) {
            return Ok(Some(existing.clone()));
        }
        inner.idempotency.insert(key.to_string(), (tenant, payload));
        Ok(None)
    }

    async fn create_waitlist_entry(&self, email: &str, now: i64) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        if inner.waitlist.iter().all(|e| e.email != email) {
            inner.waitlist.push(WaitlistEntry {
                email: email.to_string(),
                created_at: now,
            });
        }
        Ok(())
    }

    async fn list_waitlist(&self, limit: i64) -> Result<Vec<WaitlistEntry>> {
        let inner = self.inner.lock().unwrap();
        let mut v = inner.waitlist.clone();
        v.sort_by_key(|e| e.created_at);
        v.truncate(limit.clamp(1, 1000) as usize);
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use combee_common::credit::{
        CREDIT_UNITS_PER_CREDIT, CreditTransaction, CreditTransactionType, PricingRule,
        PricingStatus,
    };

    fn tenant(n: u128) -> TenantId {
        TenantId::from_u128(n)
    }

    #[tokio::test]
    async fn create_get_list_delete_roundtrip() {
        let store = InMemoryStore::new();
        let t = tenant(1);
        let id = DatabaseId::new();

        let record = store.create_database(t, id, None, None).await.unwrap();
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
        store.create_database(t, id, None, None).await.unwrap();
        let err = store.create_database(t, id, None, None).await.unwrap_err();
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

        store.create_database(t_a, id, None, None).await.unwrap();

        // B 看不到 A 的库
        assert!(matches!(
            store.get_database(t_b, id).await.unwrap_err(),
            CombeeError::DatabaseNotFound(_)
        ));
        assert!(store.list_databases(t_b).await.unwrap().is_empty());

        // B 可以创建同 id 的库(键是 (tenant, id))
        store.create_database(t_b, id, None, None).await.unwrap();
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
        store
            .create_database(t, id, Some(primary), None)
            .await
            .unwrap();

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
        store.create_database(t, id2, None, None).await.unwrap();
        store.create_database(t, id1, None, None).await.unwrap();

        let list = store.list_databases(t).await.unwrap();
        assert_eq!(list.len(), 2);
        let mut sorted = vec![id1, id2];
        sorted.sort();
        let ids: Vec<DatabaseId> = list.iter().map(|r| r.id).collect();
        assert_eq!(ids, sorted, "deterministic (created_at, id) ordering");
        assert!(list[0].created_at <= list[1].created_at);
    }

    #[test]
    fn credits_ledger_append_only_and_idempotent() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = InMemoryStore::new();
            let t = TenantId::new();

            let g = CreditTransaction {
                id: uuid::Uuid::new_v4(),
                tenant_id: t,
                txn_type: CreditTransactionType::Grant,
                amount_units: 500 * CREDIT_UNITS_PER_CREDIT,
                pricing_version: None,
                reference_id: Some("grant:alpha".into()),
                description: Some("alpha tester".into()),
                created_at: 100,
                balance_after: None,
            };
            let entry = store.append_credit_transaction(g.clone()).await.unwrap();
            assert_eq!(entry.balance_after, Some(500 * CREDIT_UNITS_PER_CREDIT));

            // 同 reference 重复入账 → 幂等返回已有,不重复
            let again = store.append_credit_transaction(g).await.unwrap();
            assert_eq!(again.balance_after, Some(500 * CREDIT_UNITS_PER_CREDIT));
            let account = store.get_credit_account(t).await.unwrap();
            assert_eq!(account.balance_units, 500 * CREDIT_UNITS_PER_CREDIT);

            // usage 扣费(负金额)
            let u = CreditTransaction {
                id: uuid::Uuid::new_v4(),
                tenant_id: t,
                txn_type: CreditTransactionType::Usage,
                amount_units: -10_000_000,
                pricing_version: Some(7),
                reference_id: Some("usage:cell:kv_read:1700000040".into()),
                description: None,
                created_at: 200,
                balance_after: None,
            };
            let e2 = store.append_credit_transaction(u).await.unwrap();
            assert_eq!(e2.balance_after, Some(490 * CREDIT_UNITS_PER_CREDIT));

            let txns = store.list_credit_transactions(t, 10, None).await.unwrap();
            assert_eq!(txns.len(), 2);
            assert_eq!(txns[0].txn_type, CreditTransactionType::Usage);

            // 余额重建 = sum(amount)
            let sum: i64 = store
                .list_credit_transactions(t, 1000, None)
                .await
                .unwrap()
                .iter()
                .map(|x| x.amount_units)
                .sum();
            assert_eq!(sum, 490 * CREDIT_UNITS_PER_CREDIT, "余额可从账本重建");
        });
    }

    #[test]
    fn voucher_redeem_single_use_and_expired() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = InMemoryStore::new();
            let t = TenantId::new();
            let (code, _) = store
                .create_vouchers(
                    50 * CREDIT_UNITS_PER_CREDIT,
                    1,
                    Some("alpha".into()),
                    Some(1_000_000),
                )
                .await
                .unwrap()
                .into_iter()
                .next()
                .unwrap();
            let hash = combee_common::credit::hash_voucher_code(&code);

            assert!(
                store.redeem_voucher(&hash, t, 2_000_000).await.is_err(),
                "过期不可用"
            );

            let amount = store.redeem_voucher(&hash, t, 500_000).await.unwrap();
            assert_eq!(amount, 50 * CREDIT_UNITS_PER_CREDIT);
            assert_eq!(
                store.get_credit_account(t).await.unwrap().balance_units,
                50 * CREDIT_UNITS_PER_CREDIT
            );

            assert!(
                store.redeem_voucher(&hash, t, 600_000).await.is_err(),
                "不可二次兑换"
            );
            assert_eq!(
                store.get_credit_account(t).await.unwrap().balance_units,
                50 * CREDIT_UNITS_PER_CREDIT
            );
        });
    }

    #[test]
    fn pricing_version_activation_and_rules() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = InMemoryStore::new();
            let rules = vec![PricingRule {
                pricing_version: 0,
                metric: UsageMetric::KvRead,
                unit_size: 1_000,
                price_units: 10,
            }];
            let v1 = store.create_pricing_version(rules).await.unwrap();
            assert_eq!(v1.version, 1);
            assert_eq!(v1.status, PricingStatus::Active);

            let (active, rules) = store.get_active_pricing().await.unwrap();
            assert_eq!(active.version, 1);
            assert_eq!(rules.len(), 1);

            let v2 = store.create_pricing_version(vec![]).await.unwrap();
            assert_eq!(v2.version, 2);
            let (active, _) = store.get_active_pricing().await.unwrap();
            assert_eq!(active.version, 2);
            let versions = store.list_pricing_versions().await.unwrap();
            assert_eq!(versions.len(), 2);
            assert_eq!(versions[0].status, PricingStatus::Inactive);
        });
    }
}
