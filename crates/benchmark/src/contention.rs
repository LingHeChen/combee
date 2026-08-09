//! Hot Cell contention benchmark:1 个 Cell 被不同并发度请求打,
//! 验证 per-db 串行化(一致性保证)在热点场景下是否成为瓶颈。
//!
//! 维度:
//! - concurrency: 1 / 8 / 32 / 128 / 512
//! - operations:   GET(缓存命中)/ SET(SQLite 写)/ mixed SQL+KV
//!
//! 每档测量(固定时长窗口):
//! - throughput(ops/s)
//! - 端到端延迟 p50 / p95 / p99
//! - per-db 锁等待 avg / max(来自 [`combee_data_node::LockStats`])
//! - 峰值队列深度(同一时刻等待同一把锁的最大请求数)
//!
//! 输出:`contention.csv` / `contention.md`(与工作目录)。

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::output_path;
use combee_common::DatabaseId;
use combee_common::config::KvDurability;
use combee_common::protocol::SqlRequest;
use combee_data_node::{DataNode, DataNodeConfig, LockStats};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tokio::task::JoinSet;

/// 每档固定测量时长。
const PHASE_DURATION: Duration = Duration::from_secs(2);
/// 每 worker 收集的延迟样本上限(用于 p50/p95/p99)。
const SAMPLES_PER_WORKER: usize = 2_000;
/// 单个 Cell 内的 key 数。
const KEYS: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Get,
    Set,
    Mixed,
}

impl Op {
    fn label(self) -> &'static str {
        match self {
            Op::Get => "GET (cache hit)",
            Op::Set => "SET (sqlite write)",
            Op::Mixed => "mixed SQL/KV",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContentionRow {
    pub op: &'static str,
    pub concurrency: usize,
    pub throughput: f64,
    pub p50_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
    pub lock_avg_ns: u64,
    pub lock_max_ns: u64,
    pub queue_max: u64,
}

pub async fn run_contention() {
    let dir = tempfile::tempdir().expect("tempdir");
    let node = Arc::new(DataNode::new(DataNodeConfig {
        data_dir: dir.path().to_path_buf(),
        max_active_dbs: 16,
        db_idle_timeout: Duration::from_secs(300),
        ttl_gc_interval: Duration::from_secs(60),
        kv_cache_capacity: 100_000,
        kv_durability: KvDurability::Fast,
        sql_timeout: Some(std::time::Duration::from_secs(30)),
        quota: Default::default(),
    }));
    let db = DatabaseId::new();

    // 准备:hot keys + SQL 表,并预热缓存
    for i in 0..KEYS {
        node.kv_set(db, format!("k:{i}"), "v".into(), None, false, false, 0)
            .await
            .unwrap();
    }
    node.execute_sql(
        db,
        SqlRequest {
            sql: "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, score REAL)".into(),
            params: vec![],
        },
        0,
    )
    .await
    .unwrap();
    for i in 0..KEYS {
        node.kv_get(db, format!("k:{i}")).await.unwrap().unwrap();
    }

    println!(
        "Hot Cell contention: 1 Cell × concurrency [1, 8, 32, 128, 512], {PHASE_DURATION:?}/phase"
    );
    println!();

    let mut rows = Vec::new();
    for &op in &[Op::Get, Op::Set, Op::Mixed] {
        for &conc in &[1, 8, 32, 128, 512] {
            let row = run_phase(&node, db, op, conc).await;
            println!("  op={:<18} conc={:<4} done", op.label(), conc);
            rows.push(row);
        }
    }

    write_outputs(&rows);
}

async fn run_phase(node: &Arc<DataNode>, db: DatabaseId, op: Op, conc: usize) -> ContentionRow {
    node.reset_lock_stats();
    let wall0 = Instant::now();
    let deadline = wall0 + PHASE_DURATION;

    let mut set = JoinSet::new();
    for wid in 0..conc {
        let node = node.clone();
        set.spawn(async move {
            // thread_rng 是 !Send,worker 用独立播种的 StdRng
            let mut rng = StdRng::seed_from_u64(wid as u64 ^ 0x9E3779B97F4A7C15);
            let mut samples: Vec<Duration> = Vec::with_capacity(SAMPLES_PER_WORKER);
            let mut ops = 0u64;
            while Instant::now() < deadline {
                let t0 = Instant::now();
                match op {
                    Op::Get => {
                        let key = format!("k:{}", rng.gen_range(0..KEYS));
                        node.kv_get(db, key).await.unwrap().unwrap();
                    }
                    Op::Set => {
                        let key = format!("k:{}", rng.gen_range(0..KEYS));
                        node.kv_set(db, key, "v".into(), None, false, false, 0)
                            .await
                            .unwrap();
                    }
                    Op::Mixed => {
                        let roll = ops % 10;
                        if roll < 5 {
                            let key = format!("k:{}", rng.gen_range(0..KEYS));
                            node.kv_get(db, key).await.unwrap().unwrap();
                        } else if roll < 8 {
                            let key = format!("k:{}", rng.gen_range(0..KEYS));
                            node.kv_set(db, key, "v".into(), None, false, false, 0)
                                .await
                                .unwrap();
                        } else {
                            node.execute_sql(
                                db,
                                SqlRequest {
                                    sql: "SELECT id, name FROM items WHERE id = ?".into(),
                                    params: vec![serde_json::json!(rng.gen_range(0..KEYS))],
                                },
                                0,
                            )
                            .await
                            .unwrap();
                        }
                    }
                }
                if samples.len() < SAMPLES_PER_WORKER {
                    samples.push(t0.elapsed());
                }
                ops += 1;
            }
            (ops, samples)
        });
    }

    let mut total_ops = 0u64;
    let mut all_samples = Vec::new();
    while let Some(res) = set.join_next().await {
        let (ops, samples) = res.expect("worker panicked");
        total_ops += ops;
        all_samples.extend(samples);
    }
    let wall = wall0.elapsed().as_secs_f64();
    let throughput = total_ops as f64 / wall;

    let lock: LockStats = node.lock_stats();
    let (p50, p95, p99) = percentiles(&all_samples);

    ContentionRow {
        op: op.label(),
        concurrency: conc,
        throughput,
        p50_us: p50,
        p95_us: p95,
        p99_us: p99,
        lock_avg_ns: lock.avg_wait_ns(),
        lock_max_ns: lock.max_wait_ns,
        queue_max: lock.max_queue_depth,
    }
}

fn percentiles(samples: &[Duration]) -> (f64, f64, f64) {
    let mut ns: Vec<u64> = samples.iter().map(|d| d.as_nanos() as u64).collect();
    ns.sort_unstable();
    let n = ns.len();
    if n == 0 {
        return (0.0, 0.0, 0.0);
    }
    let pct = |p: f64| ns[((n as f64 * p).floor() as usize).min(n - 1)] as f64 / 1e3;
    (pct(0.50), pct(0.95), pct(0.99))
}

fn write_outputs(rows: &[ContentionRow]) {
    let csv_path = output_path("contention.csv");
    let md_path = output_path("contention.md");

    let mut csv = String::from(
        "op,concurrency,throughput_ops,p50_us,p95_us,p99_us,lock_avg_ns,lock_max_ns,queue_max\n",
    );
    for r in rows {
        csv.push_str(&format!(
            "{},{},{:.0},{:.2},{:.2},{:.2},{},{},{}\n",
            r.op,
            r.concurrency,
            r.throughput,
            r.p50_us,
            r.p95_us,
            r.p99_us,
            r.lock_avg_ns,
            r.lock_max_ns,
            r.queue_max,
        ));
    }
    std::fs::write(&csv_path, &csv).expect("write contention.csv");

    let mut md = String::from(
        "| operation | concurrency | throughput (ops/s) | p50 (µs) | p95 (µs) | p99 (µs) | lock avg (µs) | lock max (µs) | queue max |\n|---|---|---:|---:|---:|---:|---:|---:|---:|\n",
    );
    for r in rows {
        md.push_str(&format!(
            "| {} | {} | {:.0} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {} |\n",
            r.op,
            r.concurrency,
            r.throughput,
            r.p50_us,
            r.p95_us,
            r.p99_us,
            r.lock_avg_ns as f64 / 1e3,
            r.lock_max_ns as f64 / 1e3,
            r.queue_max,
        ));
    }
    std::fs::write(&md_path, &md).expect("write contention.md");

    println!(
        "\n结果:{} / {}(与工作目录)",
        csv_path.display(),
        md_path.display()
    );
    println!();
    println!("{md}");
}
