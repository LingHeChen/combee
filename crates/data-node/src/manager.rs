//! Active Database Manager:连接的生命周期管理。
//!
//! 目标(设计文档第 15 节):
//! ```text
//! 100000 logical databases ≠ 100000 open SQLite connections
//! ```
//!
//! - 逻辑 Cell 可以非常多,但打开的 SQLite 连接有上限(`max_active`),超出按 LRU 逐出;
//! - 空闲超过 `db_idle_timeout` 的连接被休眠(checkpoint + 关闭),需要时再激活;
//! - 同一 Cell 内的操作通过 per-db 锁串行化(为未来 Actor 模型预留),不同 Cell 并行;
//! - 所有 SQLite 阻塞操作都在 `spawn_blocking` 中执行,不阻塞 async 运行时。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use combee_common::config::{Config as CommonConfig, KvDurability};
use combee_common::{CombeeError, DatabaseId, Result};
use rusqlite::Connection;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::{storage, ttl};

#[derive(Debug, Clone)]
pub struct DataNodeConfig {
    pub data_dir: PathBuf,
    pub max_active_dbs: usize,
    pub db_idle_timeout: Duration,
    pub ttl_gc_interval: Duration,
    /// 共享 KV 缓存条目上限。
    pub kv_cache_capacity: usize,
    /// KV 写入持久化强度。
    pub kv_durability: KvDurability,
    /// 资源配额(安全护栏;0 = 不限)。
    pub quota: combee_common::config::QuotaConfig,
    /// 单条 SQL 执行超时;超时中断执行并返回错误。None = 不限。
    pub sql_timeout: Option<Duration>,
}

impl DataNodeConfig {
    pub fn from_common(cfg: &CommonConfig) -> Self {
        Self {
            data_dir: cfg.data_dir.clone(),
            max_active_dbs: cfg.max_active_dbs,
            db_idle_timeout: cfg.db_idle_timeout,
            ttl_gc_interval: cfg.ttl_gc_interval,
            kv_cache_capacity: cfg.kv_cache_capacity,
            kv_durability: cfg.kv_durability,
            sql_timeout: (cfg.sql_timeout_secs > 0)
                .then(|| Duration::from_secs(cfg.sql_timeout_secs)),
            quota: cfg.quota.clone(),
        }
    }
}

struct Entry {
    conn: Connection,
    last_used: Instant,
}

struct Inner {
    /// 打开的连接集合(由 blocking worker 独占访问)。
    conns: Mutex<HashMap<DatabaseId, Entry>>,
    /// 每 Cell 的 SQLite 中断句柄(SQL 执行超时用)。
    interrupts: Mutex<HashMap<DatabaseId, rusqlite::InterruptHandle>>,
    /// per-db 串行化信号量;条目在删除数据库时清理(V0 规模下内存可接受)。
    db_locks: Mutex<HashMap<DatabaseId, Arc<tokio::sync::Mutex<()>>>>,
    /// 完整性校验失败的 Cell(只读保护:拒绝写操作,见 DataNode::check_writable)。
    readonly: Mutex<HashSet<DatabaseId>>,
}

pub struct ActiveDbManager {
    data_dir: PathBuf,
    max_active: usize,
    idle_timeout: Duration,
    ttl_gc_interval: Duration,
    kv_durability: KvDurability,
    inner: Arc<Inner>,
    // ---- per-db 锁的可观测性(热点 Cell 并发 benchmark 用) ----
    lock_wait_ns: AtomicU64,
    lock_wait_samples: AtomicU64,
    lock_wait_max_ns: AtomicU64,
    lock_queue: AtomicU64,
    lock_queue_max: AtomicU64,
}

/// per-db 锁等待统计(累计;`reset_lock_stats` 后用于增量窗口)。
#[derive(Debug, Clone, Copy, Default)]
pub struct LockStats {
    /// 累计锁等待时间(ns)。
    pub total_wait_ns: u64,
    /// 获取锁的次数。
    pub samples: u64,
    /// 单次最长等待(ns)。
    pub max_wait_ns: u64,
    /// 峰值排队深度(同一时刻等待同一把锁的最大请求数)。
    pub max_queue_depth: u64,
}

impl LockStats {
    pub fn avg_wait_ns(&self) -> u64 {
        self.total_wait_ns.checked_div(self.samples).unwrap_or(0)
    }
}

impl ActiveDbManager {
    pub fn new(config: DataNodeConfig) -> Self {
        std::fs::create_dir_all(&config.data_dir).expect("create data dir");
        Self {
            data_dir: config.data_dir,
            max_active: config.max_active_dbs.max(1),
            idle_timeout: config.db_idle_timeout,
            ttl_gc_interval: config.ttl_gc_interval,
            kv_durability: config.kv_durability,
            inner: Arc::new(Inner {
                conns: Mutex::new(HashMap::new()),
                db_locks: Mutex::new(HashMap::new()),
                interrupts: Mutex::new(HashMap::new()),
                readonly: Mutex::new(HashSet::new()),
            }),
            lock_wait_ns: AtomicU64::new(0),
            lock_wait_samples: AtomicU64::new(0),
            lock_wait_max_ns: AtomicU64::new(0),
            lock_queue: AtomicU64::new(0),
            lock_queue_max: AtomicU64::new(0),
        }
    }

    fn db_lock(&self, db: DatabaseId) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.inner.db_locks.lock().unwrap();
        locks
            .entry(db)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    /// 获取 per-db 锁,并记录等待时间与排队深度(可观测性)。
    /// 返回 `OwnedMutexGuard`,持有 Arc 所有权,生命周期与 &self 解耦。
    async fn acquire_lock(&self, db: DatabaseId) -> tokio::sync::OwnedMutexGuard<()> {
        let queue = self.lock_queue.fetch_add(1, Ordering::Relaxed) + 1;
        self.lock_queue_max.fetch_max(queue, Ordering::Relaxed);
        let db_lock = self.db_lock(db);
        let t0 = Instant::now();
        let guard = db_lock.lock_owned().await;
        let waited = t0.elapsed().as_nanos() as u64;
        self.lock_queue.fetch_sub(1, Ordering::Relaxed);
        self.lock_wait_ns.fetch_add(waited, Ordering::Relaxed);
        self.lock_wait_samples.fetch_add(1, Ordering::Relaxed);
        self.lock_wait_max_ns.fetch_max(waited, Ordering::Relaxed);
        guard
    }

    /// 锁等待统计快照。
    pub fn lock_stats(&self) -> LockStats {
        LockStats {
            total_wait_ns: self.lock_wait_ns.load(Ordering::Relaxed),
            samples: self.lock_wait_samples.load(Ordering::Relaxed),
            max_wait_ns: self.lock_wait_max_ns.load(Ordering::Relaxed),
            max_queue_depth: self.lock_queue_max.load(Ordering::Relaxed),
        }
    }

    /// 标记 Cell 为只读(完整性校验失败;写操作将被拒绝,同时日志告警)。
    pub fn mark_readonly(&self, db: DatabaseId) {
        let mut set = self.inner.readonly.lock().unwrap();
        if set.insert(db) {
            tracing::error!(
                service = "combee-data-node",
                event = "cell.readonly",
                cell_id = %db,
                "integrity check failed, cell entered read-only protection mode"
            );
        }
    }

    /// Cell 是否处于只读保护模式。
    pub fn is_readonly(&self, db: DatabaseId) -> bool {
        self.inner.readonly.lock().unwrap().contains(&db)
    }

    /// 清零锁统计(供 benchmark 按窗口测量)。
    pub fn reset_lock_stats(&self) {
        self.lock_wait_ns.store(0, Ordering::Relaxed);
        self.lock_wait_samples.store(0, Ordering::Relaxed);
        self.lock_wait_max_ns.store(0, Ordering::Relaxed);
        self.lock_queue.store(0, Ordering::Relaxed);
        self.lock_queue_max.store(0, Ordering::Relaxed);
    }

    /// 在给定 Cell 的连接上执行闭包。闭包内是同步 SQLite 代码,运行于 blocking worker。
    pub async fn with_conn<F, R>(&self, db: DatabaseId, f: F) -> Result<R>
    where
        F: FnOnce(&mut Connection) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let _guard = self.acquire_lock(db).await;
        let data_dir = self.data_dir.clone();
        let max_active = self.max_active;
        let durability = self.kv_durability;
        let inner = self.inner.clone();
        // 继承调用者 span(request_id 等)到阻塞线程:blocking 内日志可关联
        let span = tracing::Span::current();
        let result = tokio::task::spawn_blocking(move || {
            let _span_guard = span.enter();
            let now = Instant::now();
            let mut conns = inner.conns.lock().unwrap();
            let entry = match conns.get_mut(&db) {
                Some(e) => e,
                None => {
                    if conns.len() >= max_active {
                        evict_lru(&mut conns);
                    }
                    let path = storage::db_path(&data_dir, db);
                    let conn = match storage::open(&path, durability) {
                        Ok(c) => c,
                        Err(e) => {
                            // 完整性校验失败(或打开失败):进入只读保护模式并告警,
                            // 而不是静默提供服务(roadmap 4.1:不静默修复)。
                            inner.readonly.lock().unwrap().insert(db);
                            tracing::error!(
                                service = "combee-data-node",
                                event = "cell.readonly",
                                cell_id = %db,
                                error = %e,
                                "cell open failed, entered read-only protection mode"
                            );
                            return Err(e);
                        }
                    };
                    // 格式版本校验(roadmap §12):版本过旧/未知格式明确报错,不静默打开。
                    if let Err(e) = storage::check_format_version(&data_dir, db) {
                        return Err(e);
                    }
                    let interrupt = conn.get_interrupt_handle();
                    inner.interrupts.lock().unwrap().insert(db, interrupt);
                    // manifest:首次打开维护格式版本/时间戳(失败仅告警,不阻塞)。
                    if let Err(e) = storage::write_manifest(&data_dir, db, durability) {
                        tracing::warn!(%db, "write manifest failed: {e}");
                    }
                    debug!(service = "combee-data-node", event = "cell.open", cell_id = %db, active = conns.len() + 1);
                    conns.insert(
                        db,
                        Entry {
                            conn,
                            last_used: now,
                        },
                    );
                    conns.get_mut(&db).expect("just inserted")
                }
            };
            let result = f(&mut entry.conn);
            entry.last_used = Instant::now();
            result
        })
        .await
        .map_err(|e| CombeeError::Internal(format!("data node task panicked: {e}")))??;
        Ok(result)
    }

    /// 删除数据库:回收连接并移除磁盘文件。
    pub async fn delete_database(&self, db: DatabaseId) -> Result<()> {
        let _guard = self.acquire_lock(db).await;
        let data_dir = self.data_dir.clone();
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let mut conns = inner.conns.lock().unwrap();
            if let Some(entry) = conns.remove(&db) {
                let _ = storage::checkpoint(&entry.conn);
            }
            drop(conns);
            storage::remove_files(&data_dir, db)?;
            if let Ok(mut locks) = inner.db_locks.lock() {
                locks.remove(&db);
            }
            Ok::<(), CombeeError>(())
        })
        .await
        .map_err(|e| CombeeError::Internal(format!("data node task panicked: {e}")))??;
        Ok(())
    }

    /// 当前打开的连接数。
    pub fn active_count(&self) -> usize {
        self.inner.conns.lock().unwrap().len()
    }

    /// 中断该 Cell 当前正在执行的 SQL(执行超时保护)。
    /// SQLite 中断只中止当前语句,连接状态保持,可继续使用。
    pub fn interrupt(&self, db: DatabaseId) {
        if let Some(h) = self.inner.interrupts.lock().unwrap().get(&db) {
            h.interrupt();
        }
    }

    /// 当前活跃(有打开连接)的 Cell 列表 —— WAL 增量备份周期遍历用。
    pub fn active_databases(&self) -> Vec<DatabaseId> {
        self.inner.conns.lock().unwrap().keys().copied().collect()
    }

    /// 数据目录(供备份/恢复计算落盘路径)。
    pub fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }

    /// 关闭某个 Cell 的连接并清理 per-db 锁(不删除文件;恢复前调用)。
    pub async fn close_database(&self, db: DatabaseId) -> Result<()> {
        let _guard = self.acquire_lock(db).await;
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let mut conns = inner.conns.lock().unwrap();
            if let Some(entry) = conns.remove(&db) {
                let _ = storage::checkpoint(&entry.conn);
            }
            drop(conns);
            if let Ok(mut locks) = inner.db_locks.lock() {
                locks.remove(&db);
            }
            Ok::<(), CombeeError>(())
        })
        .await
        .map_err(|e| CombeeError::Internal(format!("data node task panicked: {e}")))??;
        Ok(())
    }

    /// 优雅关闭:checkpoint 并关闭所有连接。
    pub fn close_all(&self) {
        let mut conns = self.inner.conns.lock().unwrap();
        for (id, entry) in conns.drain() {
            let _ = storage::checkpoint(&entry.conn);
            info!(service = "combee-data-node", event = "cell.close", cell_id = %id);
        }
    }

    /// 后台维护任务:空闲连接休眠 + TTL 后台 GC。
    pub fn spawn_maintenance(&self) -> JoinHandle<()> {
        let inner = self.inner.clone();
        let idle_timeout = self.idle_timeout;
        let interval = self.ttl_gc_interval;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                let inner = inner.clone();
                let idle_timeout = idle_timeout;
                let _ = tokio::task::spawn_blocking(move || {
                    let now = Instant::now();
                    let mut conns = match inner.conns.lock() {
                        Ok(g) => g,
                        Err(_) => return, // mutex poisoned,保底继续跑
                    };
                    // 1) 空闲休眠
                    let idle: Vec<DatabaseId> = conns
                        .iter()
                        .filter(|(_, e)| now.duration_since(e.last_used) > idle_timeout)
                        .map(|(id, _)| *id)
                        .collect();
                    for id in idle {
                        if let Some(entry) = conns.remove(&id) {
                            let _ = storage::checkpoint(&entry.conn);
                            debug!(service = "combee-data-node", event = "cell.sleep", cell_id = %id);
                        }
                    }
                    // 2) TTL GC(仅针对仍活跃的连接)
                    for entry in conns.values_mut() {
                        if let Err(e) = ttl::gc_expired(&entry.conn, 1000) {
                            warn!("ttl gc failed: {e}");
                        }
                    }
                })
                .await;
            }
        })
    }
}

/// 逐出最久未使用的连接(LRU)。
fn evict_lru(conns: &mut HashMap<DatabaseId, Entry>) {
    if let Some((&id, _)) = conns.iter().min_by_key(|(_, e)| e.last_used) {
        let entry = conns.remove(&id).expect("entry still present");
        let _ = storage::checkpoint(&entry.conn);
        debug!(service = "combee-data-node", event = "cell.evict", cell_id = %id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv;
    use crate::sql;
    use combee_common::protocol::SqlRequest;
    use std::path::Path;

    fn cfg(dir: &Path) -> DataNodeConfig {
        DataNodeConfig {
            data_dir: dir.to_path_buf(),
            max_active_dbs: 8,
            db_idle_timeout: Duration::from_secs(3600),
            ttl_gc_interval: Duration::from_secs(3600),
            kv_cache_capacity: 100_000,
            kv_durability: KvDurability::Normal,
            sql_timeout: Some(std::time::Duration::from_secs(30)),
            quota: Default::default(),
        }
    }

    #[tokio::test]
    async fn concurrent_incr_same_db_is_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = Arc::new(ActiveDbManager::new(cfg(dir.path())));
        let db = DatabaseId::new();

        let mut handles = Vec::new();
        for _ in 0..50 {
            let mgr = mgr.clone();
            handles.push(tokio::spawn(async move {
                mgr.with_conn(db, |c| kv::incr(c, "counter", 1, None))
                    .await
                    .unwrap()
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        let entry = mgr
            .with_conn(db, |c| kv::get(c, "counter"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            entry.value, "50",
            "50 concurrent INCRs must land exactly 50 times"
        );
    }

    #[tokio::test]
    async fn different_dbs_do_not_interfere() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = Arc::new(ActiveDbManager::new(cfg(dir.path())));
        let d1 = DatabaseId::new();
        let d2 = DatabaseId::new();

        let (r1, r2) = tokio::join!(
            mgr.with_conn(d1, |c| kv::set(c, "k", "d1", None, false, false)),
            mgr.with_conn(d2, |c| kv::set(c, "k", "d2", None, false, false)),
        );
        r1.unwrap();
        r2.unwrap();

        let v1 = mgr
            .with_conn(d1, |c| kv::get(c, "k"))
            .await
            .unwrap()
            .unwrap();
        let v2 = mgr
            .with_conn(d2, |c| kv::get(c, "k"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(v1.value, "d1");
        assert_eq!(v2.value, "d2");
    }

    #[tokio::test]
    async fn lru_evicts_oldest_when_at_capacity() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = cfg(dir.path());
        c.max_active_dbs = 1;
        let mgr = Arc::new(ActiveDbManager::new(c));
        let d1 = DatabaseId::new();
        let d2 = DatabaseId::new();

        mgr.with_conn(d1, |c| kv::set(c, "a", "1", None, false, false))
            .await
            .unwrap();
        mgr.with_conn(d2, |c| kv::set(c, "b", "2", None, false, false))
            .await
            .unwrap();
        assert_eq!(
            mgr.active_count(),
            1,
            "d1 should have been evicted at capacity"
        );

        // 被逐出的 d1 重新打开后数据仍在
        let v = mgr
            .with_conn(d1, |c| kv::get(c, "a"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(v.value, "1");
    }

    #[tokio::test]
    async fn idle_timeout_sleeps_connections() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = Arc::new(ActiveDbManager::new(DataNodeConfig {
            data_dir: dir.path().to_path_buf(),
            max_active_dbs: 8,
            db_idle_timeout: Duration::from_millis(150),
            ttl_gc_interval: Duration::from_millis(50),
            kv_cache_capacity: 100_000,
            kv_durability: KvDurability::Normal,
            sql_timeout: Some(std::time::Duration::from_secs(30)),
            quota: Default::default(),
        }));
        let _maintenance = mgr.spawn_maintenance();

        let db = DatabaseId::new();
        mgr.with_conn(db, |c| kv::set(c, "k", "v", None, false, false))
            .await
            .unwrap();
        assert_eq!(mgr.active_count(), 1);

        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(
            mgr.active_count(),
            0,
            "idle connection should be slept by maintenance"
        );

        // 再次访问自动重新激活
        let v = mgr
            .with_conn(db, |c| kv::get(c, "k"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(v.value, "v");
    }

    #[tokio::test]
    async fn delete_database_removes_connection_and_files() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = ActiveDbManager::new(cfg(dir.path()));
        let db = DatabaseId::new();

        mgr.with_conn(db, |c| kv::set(c, "k", "v", None, false, false))
            .await
            .unwrap();
        assert_eq!(mgr.active_count(), 1);
        let path = storage::db_path(dir.path(), db);
        assert!(path.exists());

        mgr.delete_database(db).await.unwrap();
        assert_eq!(mgr.active_count(), 0);
        assert!(!path.exists(), "database files should be removed");
    }

    #[tokio::test]
    async fn data_persists_across_manager_restart() {
        let dir = tempfile::tempdir().unwrap();
        let db = DatabaseId::new();

        {
            let mgr = ActiveDbManager::new(cfg(dir.path()));
            mgr.with_conn(db, |c| {
                kv::set(c, "k", "v", None, false, false)?;
                sql::execute_sql(
                    c,
                    &SqlRequest {
                        sql: "CREATE TABLE t (x TEXT)".into(),
                        params: vec![],
                    },
                )?;
                Ok(())
            })
            .await
            .unwrap();
            mgr.close_all();
        } // mgr drop

        // 新的 manager 实例打开同一目录:KV 与 SQL 数据都还在
        let mgr = ActiveDbManager::new(cfg(dir.path()));
        mgr.with_conn(db, |c| {
            let e = kv::get(c, "k")?.expect("kv should persist");
            assert_eq!(e.value, "v");
            let r = sql::execute_sql(
                c,
                &SqlRequest {
                    sql: "SELECT x FROM t".into(),
                    params: vec![],
                },
            )?;
            assert_eq!(r.columns, vec!["x".to_string()]);
            assert!(r.rows.is_empty());
            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn lock_stats_track_waiting_and_queue_depth() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = Arc::new(ActiveDbManager::new(cfg(dir.path())));
        let db = DatabaseId::new();

        // 16 个并发写同一 Cell → 必然排队
        let mut handles = Vec::new();
        for _ in 0..16 {
            let mgr = mgr.clone();
            handles.push(tokio::spawn(async move {
                mgr.with_conn(db, |c| kv::set(c, "k", "v", None, false, false))
                    .await
                    .unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        let stats = mgr.lock_stats();
        assert!(stats.samples >= 16, "every op acquires the per-db lock");
        assert!(
            stats.max_queue_depth >= 2,
            "concurrent ops on one cell must queue, got {}",
            stats.max_queue_depth
        );

        // reset 后清零,可作增量窗口
        mgr.reset_lock_stats();
        let stats = mgr.lock_stats();
        assert_eq!(stats.samples, 0);
        assert_eq!(stats.max_queue_depth, 0);
    }
}
