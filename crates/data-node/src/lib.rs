//! Data Node:真正承载 SQLite 与 KV 的数据面。
//!
//! 核心思想:**一个 SQLite 文件 ≠ 一个常驻进程/连接**。
//! [`manager::ActiveDbManager`] 按需打开连接、限制活跃连接上限、空闲后休眠回收;
//! SQL 与 KV 共享同一个 SQLite 文件(见 `__sys_kv` 内部表);
//! KV 走 **Memory Serving Layer + Durable Storage Layer**:
//! 热 key 由全局共享缓存([`cache::KvCache`])服务,冷 key 落到 SQLite;
//! TTL 采用 lazy expiration + 后台 GC 两层策略。
//!
//! 缓存一致性模型(read-through fill + write-update/write-invalidate):
//! - **写操作**(SET/MSET/DEL/INCR/EXPIRE)在 per-db 串行临界区内先落 SQLite
//!   (权威)再更新/失效缓存;写 ACK 前缓存必已更新 → read-your-writes;
//! - **读操作**(GET/MGET/TTL/EXISTS)命中缓存时走**无锁快路径**直接返回
//!   (缓存条目是不可变已提交快照,读无需串行化);miss 才进 per-db 锁读 SQLite
//!   并填充。并发读-写交错可线性化:读要么先于写提交(读到旧值,写未 ACK),
//!   要么后于写提交(读到新值)。

pub mod agent;
pub mod backup;
pub mod cache;
pub mod kv;
pub mod manager;
pub mod server;
pub mod sql;
pub mod storage;
pub mod ttl;

use std::sync::Arc;

use combee_common::protocol::{KvEntry, KvSetItem, SqlRequest, SqlResult, TransactionRequest};
use combee_common::rpc::BackupInfo;
use combee_common::{CombeeError, DatabaseId, Result};
use object_store::ObjectStore;
use object_store::path::Path as ObjPath;

use cache::KvCache;
pub use manager::{ActiveDbManager, DataNodeConfig, LockStats};

/// Data Node 对外服务入口:所有方法都是异步的,
/// 内部通过 `ActiveDbManager` 把 SQLite 阻塞操作放到 blocking worker 执行。
pub struct DataNode {
    manager: Arc<ActiveDbManager>,
    cache: Arc<KvCache>,
    /// 单条 SQL 执行超时。
    sql_timeout: Option<std::time::Duration>,
    /// 对象存储(备份/恢复);未启用时为 None。
    store: Option<Arc<dyn ObjectStore>>,
    /// per-cell generation(fencing):failover 时经 fence_cell 递增;写校验。
    generations: std::sync::Mutex<std::collections::HashMap<DatabaseId, i64>>,
    _maintenance: tokio::task::JoinHandle<()>,
}

impl DataNode {
    pub fn new(config: DataNodeConfig) -> Self {
        let cache = Arc::new(KvCache::new(config.kv_cache_capacity.max(1) as u64));
        let sql_timeout = config.sql_timeout;
        let manager = Arc::new(ActiveDbManager::new(config));
        let maintenance = manager.spawn_maintenance();
        Self {
            manager,
            cache,
            sql_timeout,
            store: None,
            generations: std::sync::Mutex::new(std::collections::HashMap::new()),
            _maintenance: maintenance,
        }
    }

    /// generation fencing:设置/清除该 Cell 的 generation。
    /// failover 后调用,使旧主的写请求(generation 不匹配)被拒绝。
    pub fn fence_cell(&self, db: DatabaseId, generation: i64) {
        self.generations.lock().unwrap().insert(db, generation);
    }

    /// SQL 执行超时保护:超时中断 SQLite 执行并返回错误,防止无限递归/大查询占满 CPU。
    async fn timeout_sql<T>(
        &self,
        fut: impl std::future::Future<Output = Result<T>>,
        db: DatabaseId,
    ) -> Result<T> {
        match self.sql_timeout {
            Some(d) => match tokio::time::timeout(d, fut).await {
                Ok(r) => r,
                Err(_) => {
                    self.manager.interrupt(db);
                    Err(CombeeError::Sql(format!(
                        "query timed out after {:?} (interrupted)",
                        d
                    )))
                }
            },
            None => fut.await,
        }
    }

    /// 写前校验 generation(与本地 fencing 状态一致才放行)。
    fn check_generation(&self, db: DatabaseId, generation: i64) -> Result<()> {
        let cur = self
            .generations
            .lock()
            .unwrap()
            .get(&db)
            .copied()
            .unwrap_or(0);
        if generation != cur {
            return Err(CombeeError::Forbidden(format!(
                "cell {db} fenced: write generation {generation} != current {cur}"
            )));
        }
        Ok(())
    }

    /// 启用对象存储备份(生产:MinIO/S3;测试:LocalFileSystem/InMemory)。
    pub fn with_object_store(mut self, store: Arc<dyn ObjectStore>) -> Self {
        self.store = Some(store);
        self
    }

    // ---- 备份 / 恢复 ----

    /// 备份:per-db 锁内 `VACUUM INTO` 生成一致性快照,上传对象存储。
    pub async fn backup(&self, db: DatabaseId) -> Result<BackupInfo> {
        let store = self
            .store
            .clone()
            .ok_or_else(|| CombeeError::Internal("object storage not configured".into()))?;
        // 锁内快照(与写操作串行,快照一致)
        let tmp = self
            .manager
            .with_conn(db, move |conn| {
                let tmp = backup::temp_snapshot_path(db);
                let path_sql = tmp.display().to_string().replace("'", "''");
                conn.execute_batch(&format!("VACUUM INTO '{}'", path_sql))
                    .map_err(sql_err)?;
                Ok(tmp)
            })
            .await?;
        // 锁外上传
        let info = backup::upload_snapshot(&store, db, &tmp).await?;
        let _ = tokio::fs::remove_file(&tmp).await;
        tracing::info!(%db, key = %info.key, size = info.size_bytes, "backup uploaded");
        Ok(info)
    }

    /// 恢复:优先取最新 WAL 增量备份(主库 + WAL 重放),否则全量快照;
    /// 指定 `version` 时按对象 key 恢复。原子替换本地文件并清空该 db 缓存。
    pub async fn restore(&self, db: DatabaseId, version: Option<String>) -> Result<()> {
        let store = self
            .store
            .clone()
            .ok_or_else(|| CombeeError::Internal("object storage not configured".into()))?;
        let dest = storage::db_path(&self.manager.data_dir(), db);
        // 关闭该 Cell 的连接(释放文件句柄),再替换文件
        self.manager.close_database(db).await?;

        if let Some(v) = version {
            // 指定版本:按对象 key 恢复(旧全量快照或增量 snapshot)
            backup::download_snapshot(&store, &ObjPath::from(v.clone()), &dest).await?;
            let _ = tokio::fs::remove_file(format!("{}-wal", dest.display())).await;
            tracing::info!(%db, key = %v, "restored from specified backup");
        } else if backup::restore_from_incr(&store, db, &dest).await? {
            tracing::info!(%db, "restored from latest incremental backup");
        } else {
            let metas = backup::list_snapshots(&store, db).await?;
            let latest = metas
                .last()
                .ok_or_else(|| CombeeError::Internal(format!("no backups found for {db}")))?;
            backup::download_snapshot(&store, &latest.location, &dest).await?;
            let _ = tokio::fs::remove_file(format!("{}-wal", dest.display())).await;
            tracing::info!(%db, key = %latest.location, "restored from full snapshot");
        }
        self.cache.clear_database(db);
        Ok(())
    }

    /// WAL 增量备份:per-db 锁内拷贝"主库 + 当前 WAL"(与写串行,保证对齐),
    /// 上传为一轮增量备份。周期调用即缩短 RPO(恢复 = 主库 + WAL 重放)。
    pub async fn incremental_backup(&self, db: DatabaseId) -> Result<BackupInfo> {
        let store = self
            .store
            .clone()
            .ok_or_else(|| CombeeError::Internal("object storage not configured".into()))?;
        let snap_path = storage::db_path(&self.manager.data_dir(), db);
        let wal_path = std::path::PathBuf::from(format!("{}-wal", snap_path.display()));

        // 锁内拷贝(与写串行,主库 + WAL 属于同一时刻的一致状态)
        let (snap_tmp, wal_tmp) = self
            .manager
            .with_conn(db, move |_conn| {
                let dir =
                    std::env::temp_dir().join(format!("combee-incr-{}", uuid::Uuid::new_v4()));
                std::fs::create_dir_all(&dir)
                    .map_err(|e| CombeeError::Internal(format!("create tmp dir: {e}")))?;
                let s = dir.join("snap.sqlite");
                std::fs::copy(&snap_path, &s)
                    .map_err(|e| CombeeError::Internal(format!("copy db: {e}")))?;
                let w = dir.join("snap-wal");
                if wal_path.exists() {
                    std::fs::copy(&wal_path, &w)
                        .map_err(|e| CombeeError::Internal(format!("copy wal: {e}")))?;
                }
                Ok((s, w))
            })
            .await?;

        // 锁外上传
        let snap_bytes = tokio::fs::read(&snap_tmp)
            .await
            .map_err(|e| CombeeError::Internal(format!("read snap tmp: {e}")))?;
        let wal_bytes = if wal_tmp.exists() {
            Some(
                tokio::fs::read(&wal_tmp)
                    .await
                    .map_err(|e| CombeeError::Internal(format!("read wal tmp: {e}")))?,
            )
        } else {
            None
        };
        let info = backup::upload_incr(&store, db, snap_bytes, wal_bytes).await?;
        let _ = tokio::fs::remove_dir_all(snap_tmp.parent().expect("tmp dir")).await;
        tracing::info!(%db, key = %info.key, size = info.size_bytes, "incremental backup uploaded");
        Ok(info)
    }

    /// 对象存储是否已配置。
    pub fn store_enabled(&self) -> bool {
        self.store.is_some()
    }

    /// 副本同步:从对象存储拉取主节点的最新 WAL 增量归档并应用到本地(单 replica)。
    /// 返回是否应用了数据(主节点尚无归档时返回 false)。
    pub async fn replicate_from_primary(&self, db: DatabaseId) -> Result<bool> {
        let store = self
            .store
            .clone()
            .ok_or_else(|| CombeeError::Internal("object storage not configured".into()))?;
        let dest = storage::db_path(&self.manager.data_dir(), db);
        self.manager.close_database(db).await?;
        if backup::restore_from_incr(&store, db, &dest).await? {
            self.cache.clear_database(db);
            tracing::debug!(%db, "replica applied from primary archive");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 启动副本同步周期任务:每隔 `interval` 向 API Server 询问本节点作为副本的
    /// Cell 列表,并从对象存储拉取主节点增量应用到本地。
    pub fn spawn_replica_loop(
        self: &Arc<Self>,
        interval: std::time::Duration,
        agent: Arc<crate::agent::NodeAgent>,
    ) -> tokio::task::JoinHandle<()> {
        let this = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                let Some(node_id) = agent.id() else {
                    continue; // 尚未注册
                };
                let url = format!("{}/internal/nodes/{node_id}/replicas", agent.api_url());
                let resp = match reqwest::get(&url).await {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!("replica duty query failed: {e}");
                        continue;
                    }
                };
                let dbs: Vec<DatabaseId> = match resp.json().await {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("replica duty decode failed: {e}");
                        continue;
                    }
                };
                for db in dbs {
                    if let Err(e) = this.replicate_from_primary(db).await {
                        tracing::warn!(%db, "replica sync failed: {e}");
                    }
                }
            }
        })
    }

    /// 启动 WAL 增量备份周期任务:每隔 `interval` 对当前活跃 Cell 做一轮增量备份。
    pub fn spawn_incremental_backup(
        self: &Arc<Self>,
        interval: std::time::Duration,
    ) -> tokio::task::JoinHandle<()> {
        let this = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                let dbs = this.manager.active_databases();
                for db in dbs {
                    if let Err(e) = this.incremental_backup(db).await {
                        tracing::warn!(%db, "incremental backup failed: {e}");
                    }
                }
            }
        })
    }

    /// 执行单条 SQL。
    pub async fn execute_sql(
        &self,
        db: DatabaseId,
        req: SqlRequest,
        generation: i64,
    ) -> Result<SqlResult> {
        self.check_generation(db, generation)?;
        self.timeout_sql(
            self.manager
                .with_conn(db, move |conn| sql::execute_sql(conn, &req)),
            db,
        )
        .await
    }

    /// 在单个 SQLite 事务中原子执行多条语句。
    pub async fn execute_transaction(
        &self,
        db: DatabaseId,
        req: TransactionRequest,
        generation: i64,
    ) -> Result<Vec<SqlResult>> {
        self.check_generation(db, generation)?;
        self.timeout_sql(
            self.manager
                .with_conn(db, move |conn| sql::execute_transaction(conn, &req)),
            db,
        )
        .await
    }

    // ---- KV(缓存一致性:读填充 + 写失效/写更新,per-db 串行保护) ----

    /// GET key;过期视为不存在(lazy expiration)。
    /// **无锁快路径**:缓存命中直接返回(纯内存,不经过 per-db 锁与
    /// `spawn_blocking`,热点 Cell 的读可并行);未命中才进锁读 SQLite 并填充。
    pub async fn kv_get(&self, db: DatabaseId, key: String) -> Result<Option<KvEntry>> {
        let now = ttl::unix_now();
        if let Some(entry) = self.cache.get(db, &key, now) {
            return Ok(Some(entry));
        }
        let cache = self.cache.clone();
        self.manager
            .with_conn(db, move |conn| {
                let now = ttl::unix_now();
                match kv::read_with_expiry(conn, &key)? {
                    Some((value, expires_at)) => {
                        cache.put(db, &key, &value, expires_at);
                        Ok(Some(KvEntry {
                            value,
                            ttl_seconds: ttl::ttl_or_remaining(expires_at, now),
                        }))
                    }
                    None => Ok(None),
                }
            })
            .await
    }

    /// SET key value [NX|XX]。返回是否真正写入。
    /// 写入 SQLite(权威)成功后更新缓存;NX/XX 未写入时不动缓存。
    #[allow(clippy::too_many_arguments)]
    pub async fn kv_set(
        &self,
        db: DatabaseId,
        key: String,
        value: String,
        ttl_seconds: Option<u64>,
        nx: bool,
        xx: bool,
        generation: i64,
    ) -> Result<bool> {
        self.check_generation(db, generation)?;
        let cache = self.cache.clone();
        self.manager
            .with_conn(db, move |conn| {
                let written = kv::set(conn, &key, &value, ttl_seconds, nx, xx)?;
                if written {
                    cache.put(
                        db,
                        &key,
                        &value,
                        ttl::expires_at_from(ttl_seconds, ttl::unix_now()),
                    );
                }
                Ok(written)
            })
            .await
    }

    /// DEL key。返回是否删除了 key。删除成功后失效缓存。
    pub async fn kv_del(&self, db: DatabaseId, key: String, generation: i64) -> Result<bool> {
        self.check_generation(db, generation)?;
        let cache = self.cache.clone();
        self.manager
            .with_conn(db, move |conn| {
                let deleted = kv::del(conn, &key)?;
                if deleted {
                    cache.invalidate(db, &key);
                }
                Ok(deleted)
            })
            .await
    }

    /// EXISTS key(忽略已过期)。缓存命中走无锁快路径返回 true;未命中进锁查 SQLite。
    pub async fn kv_exists(&self, db: DatabaseId, key: String) -> Result<bool> {
        let now = ttl::unix_now();
        if self.cache.get(db, &key, now).is_some() {
            return Ok(true);
        }
        self.manager
            .with_conn(db, move |conn| kv::exists(conn, &key))
            .await
    }

    /// 批量读取(MGET):先无锁扫缓存(命中直接填),未命中的 key 进锁读 SQLite 并填充。
    /// 顺序与请求 keys 一致。
    pub async fn kv_mget(&self, db: DatabaseId, keys: Vec<String>) -> Result<Vec<Option<String>>> {
        // 无锁快路径:命中即填;未命中收集待落库
        let now = ttl::unix_now();
        let mut out: Vec<Option<String>> = Vec::with_capacity(keys.len());
        let mut miss: Vec<usize> = Vec::new();
        for (i, k) in keys.iter().enumerate() {
            match self.cache.get(db, k, now) {
                Some(entry) => out.push(Some(entry.value)),
                None => {
                    out.push(None);
                    miss.push(i);
                }
            }
        }
        if miss.is_empty() {
            return Ok(out);
        }
        let cache = self.cache.clone();
        self.manager
            .with_conn(db, move |conn| {
                for i in miss {
                    if let Some((value, expires_at)) = kv::read_with_expiry(conn, &keys[i])? {
                        cache.put(db, &keys[i], &value, expires_at);
                        out[i] = Some(value);
                    }
                }
                Ok(out)
            })
            .await
    }

    /// 批量写入(MSET):逐项写 SQLite 并更新缓存。
    pub async fn kv_mset(
        &self,
        db: DatabaseId,
        items: Vec<KvSetItem>,
        generation: i64,
    ) -> Result<()> {
        self.check_generation(db, generation)?;
        let cache = self.cache.clone();
        self.manager
            .with_conn(db, move |conn| {
                for it in &items {
                    let written = kv::set(conn, &it.key, &it.value, it.ttl_seconds, false, false)?;
                    debug_assert!(written);
                    if written {
                        cache.put(
                            db,
                            &it.key,
                            &it.value,
                            ttl::expires_at_from(it.ttl_seconds, ttl::unix_now()),
                        );
                    }
                }
                Ok(())
            })
            .await
    }

    /// TTL key:返回 `Some(-1)` 表示持久 key,`Some(n)` 表示剩余秒数,`None` 表示不存在。
    /// 缓存命中走无锁快路径由条目计算;未命中进锁查 SQLite。
    pub async fn kv_ttl(&self, db: DatabaseId, key: String) -> Result<Option<i64>> {
        let now = ttl::unix_now();
        if let Some(entry) = self.cache.get(db, &key, now) {
            return Ok(entry.ttl_seconds);
        }
        self.manager
            .with_conn(db, move |conn| kv::ttl(conn, &key))
            .await
    }

    /// EXPIRE key ttl / PERSIST key(ttl 为 None)。返回 key 是否存在。成功后失效缓存。
    pub async fn kv_expire(
        &self,
        db: DatabaseId,
        key: String,
        ttl_seconds: Option<u64>,
        generation: i64,
    ) -> Result<bool> {
        self.check_generation(db, generation)?;
        let cache = self.cache.clone();
        self.manager
            .with_conn(db, move |conn| {
                let updated = kv::expire(conn, &key, ttl_seconds)?;
                if updated {
                    cache.invalidate(db, &key);
                }
                Ok(updated)
            })
            .await
    }

    /// INCR / DECR key。返回新值;值非整数时报错。写后失效缓存。
    pub async fn kv_incr(
        &self,
        db: DatabaseId,
        key: String,
        delta: i64,
        ttl_seconds: Option<u64>,
        generation: i64,
    ) -> Result<i64> {
        self.check_generation(db, generation)?;
        let cache = self.cache.clone();
        self.manager
            .with_conn(db, move |conn| {
                let value = kv::incr(conn, &key, delta, ttl_seconds)?;
                cache.invalidate(db, &key);
                Ok(value)
            })
            .await
    }

    // ---- 生命周期 ----

    /// 删除数据库:回收连接、移除磁盘文件,并清空该 db 的缓存条目。
    /// 目录记录由 API Server 在 Metadata 中删除。
    pub async fn delete_database(&self, db: DatabaseId) -> Result<()> {
        self.manager.delete_database(db).await?;
        self.cache.clear_database(db);
        Ok(())
    }

    /// 当前打开的 SQLite 连接数。
    pub fn active_count(&self) -> usize {
        self.manager.active_count()
    }

    /// 缓存命中/未命中统计。
    pub fn cache_stats(&self) -> (u64, u64) {
        self.cache.stats()
    }

    /// 缓存条目数。
    pub fn cache_len(&self) -> u64 {
        self.cache.len()
    }

    /// per-db 锁等待统计(热点 Cell 并发分析)。
    pub fn lock_stats(&self) -> manager::LockStats {
        self.manager.lock_stats()
    }

    /// 清零锁统计(供 benchmark 按窗口测量)。
    pub fn reset_lock_stats(&self) {
        self.manager.reset_lock_stats();
    }

    /// 优雅关闭:checkpoint 并关闭所有连接。
    pub async fn shutdown(&self) {
        self.manager.close_all();
    }
}

/// 便捷错误转换辅助。
pub(crate) fn sql_err(e: rusqlite::Error) -> CombeeError {
    CombeeError::Sql(e.to_string())
}
