//! DataNodeClient 抽象:API Server 与 Data Node 之间的内部接口。
//!
//! 设计文档第 19 节:
//! - `LocalDataNodeClient`:单进程(开发/测试默认);
//! - `RemoteDataNodeClient`:独立 Data Node 进程,走内部 HTTP RPC(`COMBEE_DATA_NODE_URL`)。
//!
//! 后续可替换为 gRPC(`GrpcDataNodeClient`)。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
use combee_metadata::MetadataStore;
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

    /// 生命周期:确保 Cell 磁盘文件已初始化(create 后调用,使状态进入 active)。
    async fn ensure_database(&self, db: DatabaseId) -> Result<()>;
    async fn delete_database(&self, db: DatabaseId) -> Result<()>;
    /// 备份 Cell 快照到对象存储(Data Node 侧执行)。
    async fn backup(&self, db: DatabaseId) -> Result<combee_common::rpc::BackupInfo>;
    /// 从对象存储恢复 Cell(version 缺省取最新)。
    async fn restore(&self, db: DatabaseId, version: Option<String>) -> Result<()>;
    /// WAL 增量备份(主库 + WAL 周期归档)。
    async fn incremental_backup(&self, db: DatabaseId) -> Result<combee_common::rpc::BackupInfo>;
    /// Cell 磁盘占用(主库 + WAL,字节)。
    async fn storage_bytes(&self, db: DatabaseId) -> Result<u64>;
    /// 重置:删除 Cell 数据文件与缓存(目录记录/元数据保留,generation 由 metadata 递增)。
    async fn reset_database(&self, db: DatabaseId) -> Result<()>;
    /// KV 前缀扫描(浏览):返回 keys 与下一页游标。
    async fn kv_scan(
        &self,
        db: DatabaseId,
        prefix: String,
        limit: u32,
        cursor: String,
    ) -> Result<combee_common::rpc::RpcKvScanResult>;
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

    async fn ensure_database(&self, db: DatabaseId) -> Result<()> {
        self.node.ensure_database(db).await
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

    async fn reset_database(&self, db: DatabaseId) -> Result<()> {
        self.node.reset_database(db).await
    }

    async fn kv_scan(
        &self,
        db: DatabaseId,
        prefix: String,
        limit: u32,
        cursor: String,
    ) -> Result<combee_common::rpc::RpcKvScanResult> {
        self.node.kv_scan(db, prefix, limit, cursor).await
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
    /// RPC 失败回调(如:失效该 Cell 的路由缓存,让下一次请求立即走新路由)。
    on_error: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl RemoteDataNodeClient {
    pub fn new(base_url: String) -> Self {
        Self::with_hooks(base_url, None, None)
    }

    pub fn with_token(base_url: String, control_token: Option<String>) -> Self {
        Self::with_hooks(base_url, control_token, None)
    }

    pub fn with_hooks(
        base_url: String,
        control_token: Option<String>,
        on_error: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
            base: base_url.trim_end_matches('/').to_string(),
            control_token,
            on_error,
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
            tracing::debug!(
                service = "combee-api",
                event = "rpc.request",
                rpc_path = %path,
                token_prefix = token.chars().take(8).collect::<String>(),
            );
        } else {
            tracing::warn!(
                service = "combee-api",
                event = "rpc.request.no_token",
                rpc_path = %path,
                reason = "control token not configured",
            );
        }
        // request_id 贯穿:从 task-local 读取并随 RPC 透传
        if let Ok(rid) = crate::REQUEST_ID.try_with(|id| id.clone())
            && !rid.is_empty()
        {
            req = req.header("x-request-id", &rid);
        }
        // 节点不可达 / 响应损坏属于路由类失败:触发 on_error(失效路由缓存),
        // 让下一次请求立即从 authority 重新解析。业务错误(RPC 内错误码)不触发。
        let resp = req.send().await.map_err(|e| {
            self.notify_error();
            CombeeError::Internal(format!("data node rpc {path}: {e}"))
        })?;
        // 先检查 HTTP status:404(端点缺失/旧节点)/401(控制面认证失败)/500
        // 都给出明确错误,而不是误报 decode failure
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            self.notify_error();
            return Err(CombeeError::Internal(format!(
                "data node rpc {path}: HTTP {status}: {text}"
            )));
        }
        let rpc: RpcResponse<R> = resp.json().await.map_err(|e| {
            self.notify_error();
            CombeeError::Internal(format!("data node rpc {path} decode: {e}"))
        })?;
        rpc.into_result()
    }

    /// RPC 失败回调(RoutingProvider 用于失效该 Cell 的路由缓存)。
    fn notify_error(&self) {
        if let Some(f) = &self.on_error {
            f();
        }
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

    async fn ensure_database(&self, db: DatabaseId) -> Result<()> {
        self.call("rpc/ensure_database", &RpcDb { db }).await
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

    async fn reset_database(&self, db: DatabaseId) -> Result<()> {
        self.call("rpc/reset_database", &RpcDb { db }).await
    }

    async fn kv_scan(
        &self,
        db: DatabaseId,
        prefix: String,
        limit: u32,
        cursor: String,
    ) -> Result<combee_common::rpc::RpcKvScanResult> {
        self.call(
            "rpc/kv_scan",
            &combee_common::rpc::RpcKvScan {
                db,
                prefix,
                limit,
                cursor,
            },
        )
        .await
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
    /// 使某 Cell 的路由缓存失效(迁移/failover 后调用;默认实现:无缓存,空操作)。
    fn invalidate_route(&self, _db: DatabaseId) {}
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
    /// Cell → 主节点的路由缓存(TTL),避免热路径每次查 PostgreSQL。
    /// Arc:供 RPC 失败回调共享,以在失败时失效缓存。
    route_cache: Arc<std::sync::Mutex<HashMap<DatabaseId, (NodeId, Instant)>>>,
}

/// 路由缓存有效期:failover 后最多这么久收敛(failover 本身也会触发失效)。
const ROUTE_CACHE_TTL: Duration = Duration::from_secs(5);

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
            route_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }
}

impl RoutingProvider {
    /// 使某 Cell 的路由缓存失效(failover 后调用)。
    pub fn invalidate_route(&self, db: DatabaseId) {
        self.route_cache.lock().unwrap().remove(&db);
    }

    /// 创建绑定 Cell 的远程客户端;RPC 失败时自动失效该 Cell 的路由缓存,
    /// 让下一次请求立即从 authority(PostgreSQL)重新解析。
    fn make_client(
        &self,
        db: DatabaseId,
        node_id: NodeId,
        addr: String,
    ) -> Arc<dyn DataNodeClient> {
        let rc = self.route_cache.clone();
        let on_error: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            tracing::debug!("rpc failure -> invalidate route cache for cell {db}");
            rc.lock().unwrap().remove(&db);
        });
        let client: Arc<dyn DataNodeClient> =
            Arc::new(RemoteDataNodeClient::with_hooks(addr, None, Some(on_error)));
        self.clients.lock().unwrap().insert(node_id, client.clone());
        client
    }
}

#[async_trait]
impl DataNodeProvider for RoutingProvider {
    async fn client_for(&self, db: DatabaseId) -> Result<Arc<dyn DataNodeClient>> {
        // 热路径:路由缓存命中(新鲜)直接返回客户端,避免每次查 PostgreSQL。
        // 注意:guard 在语句结束即释放,不能跨 await。
        let cached = self.route_cache.lock().unwrap().get(&db).copied();
        if let Some((node_id, at)) = cached {
            if at.elapsed() < ROUTE_CACHE_TTL {
                if let Some(c) = self.clients.lock().unwrap().get(&node_id) {
                    return Ok(c.clone());
                }
                if let Some(addr) = self.registry.addr(node_id).await {
                    return Ok(self.make_client(db, node_id, addr));
                }
                // 节点不可用:清掉缓存项,走 authority 重新解析。
                self.route_cache.lock().unwrap().remove(&db);
            }
        }

        // authority:databases 表(PG)是 cell → node 的事实来源。
        let record = self.metadata.get_database_by_id(db).await?;
        match record.storage_node_id {
            Some(node_id) => {
                self.route_cache
                    .lock()
                    .unwrap()
                    .insert(db, (node_id, Instant::now()));
                let addr = match self.registry.addr(node_id).await {
                    Some(a) => a,
                    None => {
                        // 节点不可用:失效缓存,从 authority 重新解析后重试一次。
                        // 此时请求尚未发出,重试是安全的。
                        self.route_cache.lock().unwrap().remove(&db);
                        let rec2 = self.metadata.get_database_by_id(db).await?;
                        let node2 = rec2.storage_node_id.ok_or_else(|| {
                            CombeeError::Internal("database has no storage node assigned".into())
                        })?;
                        self.route_cache
                            .lock()
                            .unwrap()
                            .insert(db, (node2, Instant::now()));
                        self.registry.addr(node2).await.ok_or_else(|| {
                            CombeeError::Internal(format!("data node {node2} unavailable"))
                        })?
                    }
                };
                Ok(self.make_client(db, node_id, addr))
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
            .await
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
