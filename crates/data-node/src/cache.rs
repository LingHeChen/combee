//! 全局共享 KV Memory Cache(设计文档第 11、12 节)。
//!
//! - 整个 Data Node 共享一个 moka 缓存,key 为 `(database_id, key)`,
//!   而不是每个 Cell 一个独立 HashMap —— 冷 Cell 不占内存;
//! - 缓存只保存"最近读过的值"快照(读填充),所有写操作先落 SQLite
//!   再失效/更新缓存(write-invalidate / write-update);
//! - 一致性由调用方保证:所有缓存访问都发生在 `ActiveDbManager` 的
//!   per-db 串行临界区内(同一 Cell 的操作串行,不同 Cell 并行),
//!   因此不存在"读到旧值并覆盖新值"的填充竞态;
//! - 条目带绝对过期时间 `expires_at`,读取时惰性检查,过期即失效。

use std::sync::atomic::{AtomicU64, Ordering};

use combee_common::DatabaseId;
use combee_common::protocol::KvEntry;

use crate::ttl;

/// 缓存键:逻辑数据库 + KV key。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub database_id: DatabaseId,
    pub key: String,
}

/// 缓存值:值与绝对过期时间(与 `__sys_kv.expires_at` 同语义)。
#[derive(Debug, Clone)]
pub struct CachedValue {
    pub value: String,
    pub expires_at: Option<i64>,
}

/// 全局共享 KV 缓存。
pub struct KvCache {
    inner: moka::sync::Cache<CacheKey, CachedValue>,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl KvCache {
    /// `max_entries` 为共享内存预算(条目数)。
    pub fn new(max_entries: u64) -> Self {
        Self {
            inner: moka::sync::Cache::new(max_entries.max(1)),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    /// 查询缓存。命中且未过期时返回;过期条目立即失效并视为未命中。
    pub fn get(&self, db: DatabaseId, key: &str, now: i64) -> Option<KvEntry> {
        let cache_key = CacheKey {
            database_id: db,
            key: key.to_string(),
        };
        match self.inner.get(&cache_key) {
            Some(v) if !ttl::is_expired(v.expires_at, now) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(KvEntry {
                    value: v.value,
                    ttl_seconds: ttl::ttl_or_remaining(v.expires_at, now),
                })
            }
            Some(_) => {
                // 已过期:移除,交给 SQLite(权威)重新读取
                self.inner.invalidate(&cache_key);
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// 写入缓存(SET 成功后的 write-update)。
    pub fn put(&self, db: DatabaseId, key: &str, value: &str, expires_at: Option<i64>) {
        self.inner.insert(
            CacheKey {
                database_id: db,
                key: key.to_string(),
            },
            CachedValue {
                value: value.to_string(),
                expires_at,
            },
        );
    }

    /// 失效缓存(SET NX 未写入、DEL / INCR / EXPIRE 等写操作后的 write-invalidate)。
    pub fn invalidate(&self, db: DatabaseId, key: &str) {
        self.inner.invalidate(&CacheKey {
            database_id: db,
            key: key.to_string(),
        });
    }

    /// 清空某个数据库的全部缓存条目(删除数据库时调用,防止 id 复用后的脏读)。
    pub fn clear_database(&self, db: DatabaseId) {
        // moka 的 iter() 产出 (Arc<CacheKey>, CachedValue)
        let keys: Vec<_> = self
            .inner
            .iter()
            .filter(|(k, _)| k.database_id == db)
            .map(|(k, _)| k)
            .collect();
        for k in keys {
            self.inner.invalidate(&*k);
        }
    }

    /// 当前缓存条目数(moka 异步近似的计数)。
    pub fn len(&self) -> u64 {
        self.inner.entry_count()
    }

    /// 缓存是否为空。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 命中/未命中统计(hits, misses)。
    pub fn stats(&self) -> (u64, u64) {
        (
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> DatabaseId {
        DatabaseId::new()
    }

    const NOW: i64 = 1_000_000;

    #[test]
    fn hit_miss_and_lazy_expiry() {
        let cache = KvCache::new(100);
        let d = db();

        assert!(cache.get(d, "k", NOW).is_none(), "empty cache -> miss");
        cache.put(d, "k", "v", Some(NOW + 100));
        let e = cache.get(d, "k", NOW).unwrap();
        assert_eq!(e.value, "v");
        assert_eq!(e.ttl_seconds, Some(100));

        // 过期条目:立即失效并视为 miss
        assert!(cache.get(d, "k", NOW + 200).is_none());
        assert!(
            cache.get(d, "k", NOW + 300).is_none(),
            "expired entry removed"
        );

        // 持久 key 永不过期
        cache.put(d, "p", "x", None);
        assert!(cache.get(d, "p", NOW + 1_000_000).is_some());

        let (hits, misses) = cache.stats();
        assert_eq!(hits, 2);
        assert_eq!(misses, 3);
    }

    #[test]
    fn key_scoped_to_database() {
        let cache = KvCache::new(100);
        let a = db();
        let b = db();
        cache.put(a, "k", "a-value", None);
        assert!(
            cache.get(b, "k", NOW).is_none(),
            "db B must not see db A's key"
        );
        assert_eq!(cache.get(a, "k", NOW).unwrap().value, "a-value");
    }

    #[test]
    fn invalidate_removes_entry() {
        let cache = KvCache::new(100);
        let d = db();
        cache.put(d, "k", "v", None);
        assert!(cache.get(d, "k", NOW).is_some());
        cache.invalidate(d, "k");
        assert!(cache.get(d, "k", NOW).is_none());
    }

    // ---- DataNode 层:缓存一致性(read-through fill + write-invalidate/update) ----

    use crate::DataNode;
    use crate::manager::DataNodeConfig;
    use combee_common::config::KvDurability;
    use std::path::Path;

    fn node(dir: &Path, cache_capacity: u64) -> DataNode {
        DataNode::new(DataNodeConfig {
            data_dir: dir.to_path_buf(),
            max_active_dbs: 8,
            db_idle_timeout: std::time::Duration::from_secs(3600),
            ttl_gc_interval: std::time::Duration::from_secs(3600),
            kv_cache_capacity: cache_capacity as usize,
            kv_durability: KvDurability::Normal,
            sql_timeout: Some(std::time::Duration::from_secs(30)),
        })
    }

    #[tokio::test]
    async fn set_then_get_is_cache_hit() {
        let dir = tempfile::tempdir().unwrap();
        let n = node(dir.path(), 1000);
        let d = db();

        n.kv_set(d, "k".into(), "v".into(), None, false, false, 0)
            .await
            .unwrap();
        assert_eq!(n.kv_get(d, "k".into()).await.unwrap().unwrap().value, "v");

        let (hits, misses) = n.cache_stats();
        assert_eq!(
            hits, 1,
            "get after set should be a cache hit (write-update)"
        );
        assert_eq!(misses, 0);
    }

    #[tokio::test]
    async fn miss_fills_cache_then_hits() {
        let dir = tempfile::tempdir().unwrap();
        let d = db();
        {
            // 实例 1 写入数据后退出(数据落盘)
            let n = node(dir.path(), 1000);
            n.kv_set(d, "k".into(), "v2".into(), None, false, false, 0)
                .await
                .unwrap();
            n.shutdown().await;
        }
        // 实例 2 缓存为空:第一次 get 是 miss,从 SQLite 读回并填充;第二次是 hit
        let n = node(dir.path(), 1000);
        assert_eq!(n.kv_get(d, "k".into()).await.unwrap().unwrap().value, "v2");
        let (hits, misses) = n.cache_stats();
        assert_eq!(
            (hits, misses),
            (0, 1),
            "first read after cold start must be a miss"
        );

        assert_eq!(n.kv_get(d, "k".into()).await.unwrap().unwrap().value, "v2");
        let (hits, misses) = n.cache_stats();
        assert_eq!((hits, misses), (1, 1), "second read must hit the cache");
    }

    #[tokio::test]
    async fn overwrite_and_delete_invalidate_cache() {
        let dir = tempfile::tempdir().unwrap();
        let n = node(dir.path(), 1000);
        let d = db();

        n.kv_set(d, "k".into(), "v1".into(), None, false, false, 0)
            .await
            .unwrap();
        assert_eq!(n.kv_get(d, "k".into()).await.unwrap().unwrap().value, "v1");

        // 覆盖 → 缓存更新,读到新值
        n.kv_set(d, "k".into(), "v2".into(), None, false, false, 0)
            .await
            .unwrap();
        assert_eq!(n.kv_get(d, "k".into()).await.unwrap().unwrap().value, "v2");

        // 删除 → 缓存失效,读到 None
        assert!(n.kv_del(d, "k".into(), 0).await.unwrap());
        assert!(n.kv_get(d, "k".into()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn incr_and_expire_stay_consistent_with_cache() {
        let dir = tempfile::tempdir().unwrap();
        let n = node(dir.path(), 1000);
        let d = db();

        n.kv_set(d, "c".into(), "5".into(), Some(100), false, false, 0)
            .await
            .unwrap();
        // 预热缓存
        let e = n.kv_get(d, "c".into()).await.unwrap().unwrap();
        assert_eq!(e.value, "5");
        assert!(e.ttl_seconds.is_some());

        // INCR 后缓存失效,get 读到新值且 TTL 保留
        assert_eq!(n.kv_incr(d, "c".into(), 3, None, 0).await.unwrap(), 8);
        let e = n.kv_get(d, "c".into()).await.unwrap().unwrap();
        assert_eq!(e.value, "8", "cache must reflect INCR result");
        assert!(e.ttl_seconds.is_some(), "INCR keeps original TTL");

        // EXPIRE 后缓存失效,TTL 更新
        assert!(n.kv_expire(d, "c".into(), Some(50), 0).await.unwrap());
        let e = n.kv_get(d, "c".into()).await.unwrap().unwrap();
        assert_eq!(e.value, "8");
        let ttl = e.ttl_seconds.unwrap();
        assert!(
            (1..=50).contains(&ttl),
            "expire must refresh cached ttl, got {ttl}"
        );
    }

    #[tokio::test]
    async fn expired_cache_entry_falls_back_to_sqlite() {
        let dir = tempfile::tempdir().unwrap();
        let n = node(dir.path(), 1000);
        let d = db();

        // TTL=0:写入即过期;get 先查缓存(miss,因条目过期),再读 SQLite(亦 None)
        n.kv_set(d, "k".into(), "v".into(), Some(0), false, false, 0)
            .await
            .unwrap();
        assert!(n.kv_get(d, "k".into()).await.unwrap().is_none());
        assert!(n.kv_get(d, "k".into()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn ttl_seconds_decrease_across_cache_hits() {
        let dir = tempfile::tempdir().unwrap();
        let n = node(dir.path(), 1000);
        let d = db();

        n.kv_set(d, "t".into(), "v".into(), Some(100), false, false, 0)
            .await
            .unwrap();
        let t1 = n
            .kv_get(d, "t".into())
            .await
            .unwrap()
            .unwrap()
            .ttl_seconds
            .unwrap();
        // 两次都应是缓存 hit(第二次 ttl 递减,证明基于绝对 expires_at 计算)
        let t2 = n
            .kv_get(d, "t".into())
            .await
            .unwrap()
            .unwrap()
            .ttl_seconds
            .unwrap();
        assert!((1..=100).contains(&t1) && (1..=100).contains(&t2));
        assert!(t2 <= t1, "ttl must not increase: {t1} -> {t2}");
        let (hits, _) = n.cache_stats();
        assert_eq!(hits, 2);
    }

    #[tokio::test]
    async fn cache_isolation_between_databases() {
        let dir = tempfile::tempdir().unwrap();
        let n = node(dir.path(), 1000);
        let a = db();
        let b = db();

        n.kv_set(a, "k".into(), "a".into(), None, false, false, 0)
            .await
            .unwrap();
        n.kv_set(b, "k".into(), "b".into(), None, false, false, 0)
            .await
            .unwrap();
        assert_eq!(n.kv_get(a, "k".into()).await.unwrap().unwrap().value, "a");
        assert_eq!(n.kv_get(b, "k".into()).await.unwrap().unwrap().value, "b");
        // 两次读都是各自 db 的缓存 hit(b 的写入没有污染 a 的条目)
        let (hits, misses) = n.cache_stats();
        assert_eq!((hits, misses), (2, 0));
    }

    #[tokio::test]
    async fn eviction_does_not_break_correctness() {
        let dir = tempfile::tempdir().unwrap();
        let n = node(dir.path(), 2); // 容量 2
        let d = db();

        // 写 4 个 key,逐个读两次(缓存容量不足,必然发生驱逐)
        for i in 0..4 {
            let key = format!("k{i}");
            n.kv_set(d, key.clone(), format!("v{i}"), None, false, false, 0)
                .await
                .unwrap();
        }
        for i in 0..4 {
            let key = format!("k{i}");
            let e = n.kv_get(d, key.clone()).await.unwrap().unwrap();
            assert_eq!(
                e.value,
                format!("v{i}"),
                "evicted entries must fall back to SQLite"
            );
            let e = n.kv_get(d, key).await.unwrap().unwrap();
            assert_eq!(e.value, format!("v{i}"));
        }
    }

    #[tokio::test]
    async fn cache_survives_shutdown_but_data_reads_from_sqlite() {
        let dir = tempfile::tempdir().unwrap();
        let d = db();
        {
            let n = node(dir.path(), 1000);
            n.kv_set(d, "k".into(), "persisted".into(), None, false, false, 0)
                .await
                .unwrap();
            n.shutdown().await;
        }
        // 新实例:缓存为空,数据从 SQLite 读回
        let n = node(dir.path(), 1000);
        let e = n.kv_get(d, "k".into()).await.unwrap().unwrap();
        assert_eq!(
            e.value, "persisted",
            "data survives restart (cache is memory-only)"
        );
        let (hits, misses) = n.cache_stats();
        assert_eq!((hits, misses), (0, 1), "fresh instance must start cold");
    }

    #[tokio::test]
    async fn delete_database_clears_its_cache_entries() {
        let dir = tempfile::tempdir().unwrap();
        let n = node(dir.path(), 1000);
        let d = db();

        n.kv_set(d, "k".into(), "v".into(), None, false, false, 0)
            .await
            .unwrap();
        // get 命中 set 更新的缓存,证明条目已存在
        assert_eq!(n.kv_get(d, "k".into()).await.unwrap().unwrap().value, "v");
        let (hits, _) = n.cache_stats();
        assert!(hits >= 1);

        n.delete_database(d).await.unwrap();
        // moka entry_count 是异步近似的计数,轮询等待清零
        for _ in 0..50 {
            if n.cache_len() == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(
            n.cache_len(),
            0,
            "cache entries for deleted db must be cleared"
        );
    }

    #[tokio::test]
    async fn cache_hits_take_no_per_db_lock() {
        // 无锁快路径的直接证据:命中 GET 不产生任何锁等待样本
        let dir = tempfile::tempdir().unwrap();
        let n = node(dir.path(), 1000);
        let d = db();

        n.kv_set(d, "k".into(), "v".into(), None, false, false, 0)
            .await
            .unwrap();
        n.kv_get(d, "k".into()).await.unwrap().unwrap(); // 预热
        n.reset_lock_stats();

        for _ in 0..100 {
            n.kv_get(d, "k".into()).await.unwrap().unwrap();
        }
        let stats = n.lock_stats();
        assert_eq!(stats.samples, 0, "cache hits must NOT take the per-db lock");
        let (hits, _) = n.cache_stats();
        assert!(hits >= 100, "all fast-path reads should hit the cache");
    }

    #[tokio::test]
    async fn concurrent_reads_and_writes_stay_linearizable() {
        // 热点 Cell 并发读-写:读到的值必须恒为某个已提交值(v1/v2),
        // 无撕裂、无中间值、无脏读。
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let n = Arc::new(node(dir.path(), 1000));
        let d = db();

        let mut handles = Vec::new();
        for w in 0..4 {
            let n = n.clone();
            handles.push(tokio::spawn(async move {
                for i in 0..2_000 {
                    let v = if (i + w) % 2 == 0 { "v1" } else { "v2" };
                    n.kv_set(d, "k".into(), v.into(), None, false, false, 0)
                        .await
                        .unwrap();
                }
            }));
        }
        for _ in 0..4 {
            let n = n.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..2_000 {
                    if let Some(e) = n.kv_get(d, "k".into()).await.unwrap() {
                        assert!(
                            e.value == "v1" || e.value == "v2",
                            "corrupted value observed: {:?}",
                            e.value
                        );
                    }
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        // 所有写线程结束后,值必然存在且合法
        let e = n.kv_get(d, "k".into()).await.unwrap().unwrap();
        assert!(e.value == "v1" || e.value == "v2");
        n.shutdown().await;
    }
}
