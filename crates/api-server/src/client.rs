//! DataNodeClient 抽象:API Server 与 Data Node 之间的内部接口。
//!
//! 设计文档第 19 节:
//! - `LocalDataNodeClient`:单进程(开发/测试默认);
//! - `RemoteDataNodeClient`:独立 Data Node 进程,走内部 HTTP RPC(`COMBEE_DATA_NODE_URL`)。
//!
//! 后续可替换为 gRPC(`GrpcDataNodeClient`)。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use combee_common::protocol::{
    KvEntry, KvExpireRequest, KvIncrRequest, KvSetItem, KvSetRequest, SqlRequest, SqlResult,
    TransactionRequest,
};
use combee_common::rpc::{
    BackupInfo, RpcDb, RpcFence, RpcKvDel, RpcKvExpire, RpcKvGet, RpcKvIncr, RpcKvKeys, RpcKvSet,
    RpcKvSetItems, RpcResponse, RpcRestore, RpcSql, RpcTransaction,
};
use combee_common::{CombeeError, DatabaseId, NodeId, Result};
use combee_data_node::DataNode;
use combee_metadata::{DEFAULT_TENANT, MetadataStore};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::nodes::NodeRegistry;

#[async_trait]
pub trait DataNodeClient: Send + Sync {
    /// 写操作带 generation(fencing):Data Node 校验,不匹配拒绝。
    async fn execute_sql(
        &self,
        db: DatabaseId,
        req: SqlRequest,
        generation: i64,
    ) -> Result<SqlResult>;
    async fn execute_transaction(
        &self,
        db: DatabaseId,
        req: TransactionRequest,
        generation: i64,
    ) -> Result<Vec<SqlResult>>;

    async fn kv_get(&self, db: DatabaseId, key: String) -> Result<Option<KvEntry>>;
    async fn kv_set(
        &self,
        db: DatabaseId,
        key: String,
        req: KvSetRequest,
        generation: i64,
    ) -> Result<bool>;
    async fn kv_del(&self, db: DatabaseId, key: String, generation: i64) -> Result<bool>;
    async fn kv_exists(&self, db: DatabaseId, key: String) -> Result<bool>;
    async fn kv_mget(&self, db: DatabaseId, keys: Vec<String>) -> Result<Vec<Option<String>>>;
    async fn kv_mset(&self, db: DatabaseId, items: Vec<KvSetItem>, generation: i64) -> Result<()>;
    async fn kv_ttl(&self, db: DatabaseId, key: String) -> Result<Option<i64>>;
    async fn kv_expire(
        &self,
        db: DatabaseId,
        req: KvExpireRequest,
        generation: i64,
    ) -> Result<bool>;
    async fn kv_incr(&self, db: DatabaseId, req: KvIncrRequest, generation: i64) -> Result<i64>;

    /// generation fencing:通知 Data Node 该 Cell 的新 generation(failover 后)。
    async fn fence_cell(&self, db: DatabaseId, generation: i64) -> Result<()>;
    /// 副本追平:立即从对象存储拉取主节点最新归档。
    async fn replicate(&self, db: DatabaseId) -> Result<bool>;

    async fn delete_database(&self, db: DatabaseId) -> Result<()>;
    /// 备份 Cell 快照到对象存储(Data Node 侧执行)。
    async fn backup(&self, db: DatabaseId) -> Result<combee_common::rpc::BackupInfo>;
    /// 从对象存储恢复 Cell(version 缺省取最新)。
    async fn restore(&self, db: DatabaseId, version: Option<String>) -> Result<()>;
    /// WAL 增量备份(主库 + WAL 周期归档)。
    async fn incremental_backup(&self, db: DatabaseId) -> Result<combee_common::rpc::BackupInfo>;
    /// Cell 磁盘占用(主库 + WAL,字节)。
    async fn storage_bytes(&self, db: DatabaseId) -> Result<u64>;
    /// 当前打开的 SQLite 连接数(仅 Local 有意义;远程返回 0)。
    fn active_count(&self) -> usize;
}

/// V0 本地实现:进程内直接调用 Data Node。
pub struct LocalDataNodeClient {
    node: Arc<DataNode>,
}

impl LocalDataNodeClient {
    pub fn new(node: Arc<DataNode>) -> Self {
        Self { node }
    }

    /// 优雅关闭 Data Node(供进程退出时调用)。
    pub async fn shutdown(&self) {
        self.node.shutdown().await;
    }
}

#[async_trait]
impl DataNodeClient for LocalDataNodeClient {
    async fn execute_sql(
        &self,
        db: DatabaseId,
        req: SqlRequest,
        generation: i64,
    ) -> Result<SqlResult> {
        self.node.execute_sql(db, req, generation).await
    }

    async fn execute_transaction(
        &self,
        db: DatabaseId,
        req: TransactionRequest,
        generation: i64,
    ) -> Result<Vec<SqlResult>> {
        self.node.execute_transaction(db, req, generation).await
    }

    async fn kv_get(&self, db: DatabaseId, key: String) -> Result<Option<KvEntry>> {
        self.node.kv_get(db, key).await
    }

    async fn kv_set(
        &self,
        db: DatabaseId,
        key: String,
        req: KvSetRequest,
        generation: i64,
    ) -> Result<bool> {
        self.node
            .kv_set(
                db,
                key,
                req.value,
                req.ttl_seconds,
                req.nx,
                req.xx,
                generation,
            )
            .await
    }

    async fn kv_del(&self, db: DatabaseId, key: String, generation: i64) -> Result<bool> {
        self.node.kv_del(db, key, generation).await
    }

    async fn kv_exists(&self, db: DatabaseId, key: String) -> Result<bool> {
        self.node.kv_exists(db, key).await
    }

    async fn kv_mget(&self, db: DatabaseId, keys: Vec<String>) -> Result<Vec<Option<String>>> {
        self.node.kv_mget(db, keys).await
    }

    async fn kv_mset(&self, db: DatabaseId, items: Vec<KvSetItem>, generation: i64) -> Result<()> {
        self.node.kv_mset(db, items, generation).await
    }

    async fn kv_ttl(&self, db: DatabaseId, key: String) -> Result<Option<i64>> {
        self.node.kv_ttl(db, key).await
    }

    async fn kv_expire(
        &self,
        db: DatabaseId,
        req: KvExpireRequest,
        generation: i64,
    ) -> Result<bool> {
        self.node
            .kv_expire(db, req.key, req.ttl_seconds, generation)
            .await
    }

    async fn kv_incr(&self, db: DatabaseId, req: KvIncrRequest, generation: i64) -> Result<i64> {
        self.node
            .kv_incr(db, req.key, req.delta, req.ttl_seconds, generation)
            .await
    }

    async fn delete_database(&self, db: DatabaseId) -> Result<()> {
        self.node.delete_database(db).await
    }

    async fn backup(&self, db: DatabaseId) -> Result<BackupInfo> {
        self.node.backup(db).await
    }

    async fn incremental_backup(&self, db: DatabaseId) -> Result<BackupInfo> {
        self.node.incremental_backup(db).await
    }

    async fn restore(&self, db: DatabaseId, version: Option<String>) -> Result<()> {
        self.node.restore(db, version).await
    }

    async fn fence_cell(&self, db: DatabaseId, generation: i64) -> Result<()> {
        self.node.fence_cell(db, generation);
        Ok(())
    }

    async fn replicate(&self, db: DatabaseId) -> Result<bool> {
        self.node.replicate_from_primary(db).await
    }

    async fn storage_bytes(&self, db: DatabaseId) -> Result<u64> {
        self.node.storage_bytes(db).await
    }

    fn active_count(&self) -> usize {
        self.node.active_count()
    }
}

/// 远程实现:通过内部 HTTP RPC 调用独立的 Data Node 进程。
pub struct RemoteDataNodeClient {
    http: reqwest::Client,
    base: String,
    /// 控制面令牌;配置时每次 RPC 附带 `x-control-token`。
    control_token: Option<String>,
}

impl RemoteDataNodeClient {
    pub fn new(base_url: String) -> Self {
        Self::with_token(base_url, None)
    }

    pub fn with_token(base_url: String, control_token: Option<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
            base: base_url.trim_end_matches('/').to_string(),
            control_token,
        }
    }

    /// POST 一个 RPC 请求并解析 `RpcResponse`,错误还原为 [`CombeeError`]。
    async fn call<T, R>(&self, path: &str, body: &T) -> Result<R>
    where
        T: Serialize + Sync,
        R: DeserializeOwned,
    {
        let mut req = self.http.post(format!("{}/{}", self.base, path)).json(body);
        if let Some(token) = &self.control_token {
            req = req.header("x-control-token", token);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| CombeeError::Internal(format!("data node rpc {path}: {e}")))?;
        let rpc: RpcResponse<R> = resp
            .json()
            .await
            .map_err(|e| CombeeError::Internal(format!("data node rpc {path} decode: {e}")))?;
        rpc.into_result()
    }
}

#[async_trait]
impl DataNodeClient for RemoteDataNodeClient {
    async fn execute_sql(
        &self,
        db: DatabaseId,
        req: SqlRequest,
        generation: i64,
    ) -> Result<SqlResult> {
        self.call(
            "rpc/execute_sql",
            &RpcSql {
                db,
                req,
                generation,
            },
        )
        .await
    }

    async fn execute_transaction(
        &self,
        db: DatabaseId,
        req: TransactionRequest,
        generation: i64,
    ) -> Result<Vec<SqlResult>> {
        self.call(
            "rpc/execute_transaction",
            &RpcTransaction {
                db,
                req,
                generation,
            },
        )
        .await
    }

    async fn kv_get(&self, db: DatabaseId, key: String) -> Result<Option<KvEntry>> {
        self.call("rpc/kv_get", &RpcKvGet { db, key }).await
    }

    async fn kv_set(
        &self,
        db: DatabaseId,
        key: String,
        req: KvSetRequest,
        generation: i64,
    ) -> Result<bool> {
        self.call(
            "rpc/kv_set",
            &RpcKvSet {
                db,
                key,
                req,
                generation,
            },
        )
        .await
    }

    async fn kv_del(&self, db: DatabaseId, key: String, generation: i64) -> Result<bool> {
        self.call(
            "rpc/kv_del",
            &RpcKvDel {
                db,
                key,
                generation,
            },
        )
        .await
    }

    async fn kv_exists(&self, db: DatabaseId, key: String) -> Result<bool> {
        self.call("rpc/kv_exists", &RpcKvGet { db, key }).await
    }

    async fn kv_mget(&self, db: DatabaseId, keys: Vec<String>) -> Result<Vec<Option<String>>> {
        self.call("rpc/kv_mget", &RpcKvKeys { db, keys }).await
    }

    async fn kv_mset(&self, db: DatabaseId, items: Vec<KvSetItem>, generation: i64) -> Result<()> {
        self.call(
            "rpc/kv_mset",
            &RpcKvSetItems {
                db,
                items,
                generation,
            },
        )
        .await
    }

    async fn kv_ttl(&self, db: DatabaseId, key: String) -> Result<Option<i64>> {
        self.call("rpc/kv_ttl", &RpcKvGet { db, key }).await
    }

    async fn kv_expire(
        &self,
        db: DatabaseId,
        req: KvExpireRequest,
        generation: i64,
    ) -> Result<bool> {
        self.call(
            "rpc/kv_expire",
            &RpcKvExpire {
                db,
                req,
                generation,
            },
        )
        .await
    }

    async fn kv_incr(&self, db: DatabaseId, req: KvIncrRequest, generation: i64) -> Result<i64> {
        self.call(
            "rpc/kv_incr",
            &RpcKvIncr {
                db,
                req,
                generation,
            },
        )
        .await
    }

    async fn delete_database(&self, db: DatabaseId) -> Result<()> {
        self.call("rpc/delete_database", &RpcDb { db }).await
    }

    async fn backup(&self, db: DatabaseId) -> Result<BackupInfo> {
        self.call("rpc/backup", &RpcDb { db }).await
    }

    async fn incremental_backup(&self, db: DatabaseId) -> Result<BackupInfo> {
        self.call("rpc/incremental_backup", &RpcDb { db }).await
    }

    async fn restore(&self, db: DatabaseId, version: Option<String>) -> Result<()> {
        self.call("rpc/restore", &RpcRestore { db, version }).await
    }

    async fn fence_cell(&self, db: DatabaseId, generation: i64) -> Result<()> {
        self.call("rpc/fence_cell", &RpcFence { db, generation })
            .await
    }

    async fn replicate(&self, db: DatabaseId) -> Result<bool> {
        self.call("rpc/replicate", &RpcDb { db }).await
    }

    async fn storage_bytes(&self, db: DatabaseId) -> Result<u64> {
        self.call("rpc/storage_bytes", &RpcDb { db }).await
    }

    fn active_count(&self) -> usize {
        0 // 远程无法同步读取,仅 Local 有意义
    }
}

// ---- 按数据库路由的 DataNodeProvider ----

/// 按 Cell 提供对应的 Data Node 客户端(placement 路由)。
#[async_trait]
pub trait DataNodeProvider: Send + Sync {
    /// 根据 Cell 的 storage_node_id 解析出客户端。
    async fn client_for(&self, db: DatabaseId) -> Result<Arc<dyn DataNodeClient>>;
    /// 按节点 ID 解析出客户端(failover 等控制面操作用)。
    async fn client_for_node(&self, node: NodeId) -> Result<Arc<dyn DataNodeClient>>;
}

/// 单进程模式:所有 Cell 都走同一个本地客户端。
pub struct LocalProvider {
    local: Arc<dyn DataNodeClient>,
}

impl LocalProvider {
    pub fn new(local: Arc<dyn DataNodeClient>) -> Self {
        Self { local }
    }
}

#[async_trait]
impl DataNodeProvider for LocalProvider {
    async fn client_for(&self, _db: DatabaseId) -> Result<Arc<dyn DataNodeClient>> {
        Ok(self.local.clone())
    }

    async fn client_for_node(&self, _node: NodeId) -> Result<Arc<dyn DataNodeClient>> {
        Ok(self.local.clone())
    }
}

/// 多节点模式:按 Cell 的 storage_node_id 路由到对应 Data Node 的 RPC 客户端。
/// - 节点缺失/心跳超时 → 报错(保证数据位置确定);
/// - `storage_node_id = None`(未注册节点的单机模式)→ 用 local 兜底。
pub struct RoutingProvider {
    registry: Arc<NodeRegistry>,
    metadata: Arc<dyn MetadataStore>,
    /// 按节点缓存的远程客户端(节点重启换端口时需重建 registry 或重启 API Server)。
    clients: std::sync::Mutex<HashMap<NodeId, Arc<dyn DataNodeClient>>>,
    local: Option<Arc<dyn DataNodeClient>>,
}

impl RoutingProvider {
    pub fn new(
        registry: Arc<NodeRegistry>,
        metadata: Arc<dyn MetadataStore>,
        local: Option<Arc<dyn DataNodeClient>>,
    ) -> Self {
        Self {
            registry,
            metadata,
            clients: std::sync::Mutex::new(HashMap::new()),
            local,
        }
    }
}

#[async_trait]
impl DataNodeProvider for RoutingProvider {
    async fn client_for(&self, db: DatabaseId) -> Result<Arc<dyn DataNodeClient>> {
        let record = self.metadata.get_database(DEFAULT_TENANT, db).await?;
        match record.storage_node_id {
            Some(node_id) => {
                let addr = self.registry.addr(node_id).ok_or_else(|| {
                    CombeeError::Internal(format!("data node {node_id} unavailable"))
                })?;
                let mut clients = self.clients.lock().unwrap();
                if let Some(c) = clients.get(&node_id) {
                    return Ok(c.clone());
                }
                let client: Arc<dyn DataNodeClient> = Arc::new(RemoteDataNodeClient::new(addr));
                clients.insert(node_id, client.clone());
                Ok(client)
            }
            None => self.local.clone().ok_or_else(|| {
                CombeeError::Internal("database has no storage node assigned".into())
            }),
        }
    }

    async fn client_for_node(&self, node: NodeId) -> Result<Arc<dyn DataNodeClient>> {
        let addr = self
            .registry
            .addr(node)
            .ok_or_else(|| CombeeError::Internal(format!("data node {node} unavailable")))?;
        let mut clients = self.clients.lock().unwrap();
        if let Some(c) = clients.get(&node) {
            return Ok(c.clone());
        }
        let client: Arc<dyn DataNodeClient> = Arc::new(RemoteDataNodeClient::new(addr));
        clients.insert(node, client.clone());
        Ok(client)
    }
}
