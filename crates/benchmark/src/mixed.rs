//! Cache miss / mixed workload benchmark。
//!
//! - **miss 梯度**:同一批数据下,控制读请求中"从未读过的冷 key"比例
//!   (0% / 25% / 50% / 75% / 100%),展示延迟与 cache hit rate 随 miss 率的变化曲线;
//! - **mixed workload**:贴近真实场景的混合负载 —— 热读(hot GET)+ 写(SET)+
//!   冷读(cold GET)+ 带 TTL 的读(已过期,miss),报告 p50/p95/p99 与 hit rate。
//!
//! 注意:SET 是 write-update(写时即更新缓存),因此**预写数据必须用独立写实例**,
//! 读实例的缓存才为空 —— 否则"从未读过的 key"在 SET 时已进缓存,冷读不会 miss。

use std::time::{Duration, Instant};

use combee_common::DatabaseId;
use combee_common::config::KvDurability;
use combee_data_node::{DataNode, DataNodeConfig};
use rand::Rng;

use crate::report;

const HOT_KEYS: usize = 20_000;
/// 冷 key 池:每个只读一次 → 每次都是 miss(池必须大于采样总数)。
const COLD_KEYS: usize = 100_000;
const GRADIENT_SAMPLES: usize = 10_000;
const MIXED_SAMPLES: usize = 20_000;

pub async fn run_mixed() {
    bench_miss_gradient().await;
    bench_mixed_workload().await;
}

fn node(dir: &std::path::Path) -> DataNode {
    DataNode::new(DataNodeConfig {
        data_dir: dir.to_path_buf(),
        max_active_dbs: 16,
        db_idle_timeout: Duration::from_secs(300),
        ttl_gc_interval: Duration::from_secs(60),
        kv_cache_capacity: 500_000,
        kv_durability: KvDurability::Fast,
        sql_timeout: Some(std::time::Duration::from_secs(30)),
        quota: Default::default(),
    })
}

/// 用写实例预置数据(落盘),返回读实例。读实例缓存为空。
async fn warm_store(dir: &std::path::Path, db: DatabaseId, with_expired: bool) -> DataNode {
    let w = node(dir);
    for i in 0..HOT_KEYS {
        w.kv_set(db, format!("hot:{i}"), "v".into(), None, false, false, 0)
            .await
            .unwrap();
    }
    for i in 0..COLD_KEYS {
        w.kv_set(db, format!("cold:{i}"), "v".into(), None, false, false, 0)
            .await
            .unwrap();
    }
    if with_expired {
        // TTL=0:写入即过期,读必 miss(lazy expiration)
        for i in 0..2_000 {
            w.kv_set(db, format!("exp:{i}"), "v".into(), Some(0), false, false, 0)
                .await
                .unwrap();
        }
    }
    w.shutdown().await;
    // 读实例:缓存为空
    node(dir)
}

/// cache miss 梯度:0% / 25% / 50% / 75% / 100% 冷读比例。
async fn bench_miss_gradient() {
    let dir = tempfile::tempdir().unwrap();
    let db = DatabaseId::new();
    let n = warm_store(dir.path(), db, false).await;

    // 预热 hot 缓存(此时 hot 全 hit)
    for i in 0..HOT_KEYS {
        n.kv_get(db, format!("hot:{i}")).await.unwrap().unwrap();
    }

    let mut rng = rand::thread_rng();
    // 基线:预热阶段(hot 首次读)的 miss 不计入统计
    let base = n.cache_stats();
    let mut prev = base;
    let mut cold_offset = 0usize;
    println!("\n== cache miss gradient(冷读 = 每个只读一次的 key,必然 miss)==");
    for &frac in &[0.0, 0.25, 0.5, 0.75, 1.0] {
        let cold_target = (GRADIENT_SAMPLES as f64 * frac) as usize;
        let mut samples = Vec::with_capacity(GRADIENT_SAMPLES);
        for i in 0..GRADIENT_SAMPLES {
            // 每档使用互不重叠的冷 key 区间,保证冷读必然 miss
            let key = if i < cold_target {
                format!("cold:{}", cold_offset + i)
            } else {
                format!("hot:{}", rng.gen_range(0..HOT_KEYS))
            };
            let t0 = Instant::now();
            n.kv_get(db, key).await.unwrap().unwrap();
            samples.push(t0.elapsed());
        }
        cold_offset += cold_target;
        report(&format!("GET miss {:.0}%", frac * 100.0), &samples);
        let (hits, misses) = n.cache_stats();
        let (dh, dm) = (hits - prev.0, misses - prev.1);
        prev = (hits, misses);
        let total = dh + dm;
        println!(
            "    cache: +{dh} hits / +{dm} misses (hit rate={:.1}%)",
            dh as f64 * 100.0 / total as f64
        );
    }
    n.shutdown().await;
}

/// mixed workload:60% 热读 + 20% 写 + 10% 冷读(新 key)+ 10% 过期 key 读。
async fn bench_mixed_workload() {
    let dir = tempfile::tempdir().unwrap();
    let db = DatabaseId::new();
    let n = warm_store(dir.path(), db, true).await;

    // 预热 hot 缓存
    for i in 0..HOT_KEYS {
        n.kv_get(db, format!("hot:{i}")).await.unwrap().unwrap();
    }

    // 基线:预热阶段的 miss 不计入统计
    let base = n.cache_stats();
    let mut rng = rand::thread_rng();
    let mut samples = Vec::with_capacity(MIXED_SAMPLES);
    let mut cold_seen = 0usize;
    for _ in 0..MIXED_SAMPLES {
        let t0 = Instant::now();
        let roll = rng.gen_range(0..100);
        if roll < 60 {
            // 60% hot GET(hit)
            let key = format!("hot:{}", rng.gen_range(0..HOT_KEYS));
            n.kv_get(db, key).await.unwrap().unwrap();
        } else if roll < 80 {
            // 20% SET(写 + 更新缓存)
            let key = format!("hot:{}", rng.gen_range(0..HOT_KEYS));
            n.kv_set(db, key, "v2".into(), None, false, false, 0)
                .await
                .unwrap();
        } else if roll < 90 {
            // 10% cold GET(从未读过的 key → miss;冷池 100k > 采样 20k,够用)
            let key = format!("cold:{cold_seen}");
            cold_seen += 1;
            n.kv_get(db, key).await.unwrap().unwrap();
        } else {
            // 10% 过期 key 读(miss)
            let key = format!("exp:{}", rng.gen_range(0..2_000));
            n.kv_get(db, key).await.unwrap(); // None
        }
        samples.push(t0.elapsed());
    }
    println!("\n== mixed workload(60% hot GET / 20% SET / 10% cold GET / 10% expired GET)==");
    report("mixed workload", &samples);
    let (hits, misses) = n.cache_stats();
    let (dh, dm) = (hits - base.0, misses - base.1);
    let total = dh + dm;
    println!(
        "    cache(采样增量): +{dh} hits / +{dm} misses (hit rate={:.1}%)",
        dh as f64 * 100.0 / total as f64
    );
    n.shutdown().await;
}
