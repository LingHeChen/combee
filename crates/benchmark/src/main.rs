//! Combee benchmark suite(对照设计文档第 22 节性能目标)。
//!
//! 直接调用 DataNode(不含 HTTP 与公网 RTT),测量:
//! - KV hot GET(全部缓存命中,纯内存)/ cold GET(缓存未命中,读 SQLite)
//! - SET(fast / normal / strict 三种 durability)
//! - Simple SQL(SELECT 与 INSERT)
//! - many-db(20,000 个逻辑 Cell,验证活跃连接上限)
//! - cache miss 梯度与 mixed workload(`--mixed`)
//! - Hot Cell contention(1 Cell × 并发度,per-db 锁瓶颈分析,`--contention`)
//! - end-to-end(client → HTTP → API Server → RPC → Data Node,`--e2e`)
//!
//! 运行:
//! ```text
//! cargo run --release -p combee-benchmark                    # 默认性能基准(含 mixed)
//! cargo run --release -p combee-benchmark -- --mixed         # 仅 miss 梯度 + mixed workload
//! cargo run --release -p combee-benchmark -- --contention    # 热点 Cell 并发瓶颈
//! cargo run --release -p combee-benchmark -- --e2e --url http://127.0.0.1:8080   # 端到端
//! cargo run --release -p combee-benchmark -- --capacity      # 容量基准(默认 10k/100k/1M × 32/100/500/1k/5k)
//! cargo run --release -p combee-benchmark -- --capacity --metadata postgres --total 1M --active 32,500,5000
//! ```
//! `--metadata in-memory|postgres`(默认 in-memory);postgres 时读取
//! `COMBEE_DATABASE_URL`(默认 postgres://combee:combee@localhost:5432/combee)。

mod capacity;
mod contention;
mod e2e;
mod mixed;
mod proc;

use std::sync::Arc;
use std::time::{Duration, Instant};

use combee_common::DatabaseId;
use combee_common::config::KvDurability;
use combee_common::protocol::{KvSetItem, SqlRequest};
use combee_data_node::{DataNode, DataNodeConfig};
use combee_metadata::{InMemoryStore, MetadataStore, PostgresStore};
use rand::Rng;
use tokio::task::JoinSet;

const HOT_KEYS: usize = 20_000;
const HOT_READS: usize = 50_000;
const COLD_KEYS: usize = 10_000;
const SET_OPS: usize = 20_000;
const STRICT_OPS: usize = 5_000;
const SQL_OPS: usize = 50_000;
const MANY_DBS: usize = 20_000;
const MANY_TOUCHES: usize = 200;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rt = tokio::runtime::Runtime::new().expect("runtime");

    if has_flag(&args, "--capacity") || has_flag(&args, "-c") {
        let totals = parse_sizes(&args, "--total", &[10_000, 100_000, 1_000_000]);
        let actives = parse_sizes(&args, "--active", &[32, 100, 500, 1_000, 5_000]);
        let samples = parse_usize(&args, "--samples", 2_000);
        let (metadata, meta_label) = build_metadata(&args, &rt);
        rt.block_on(capacity::run_capacity(&totals, &actives, samples, metadata));
        let _ = meta_label;
        return;
    }

    if has_flag(&args, "--mixed") {
        rt.block_on(mixed::run_mixed());
        return;
    }

    if has_flag(&args, "--contention") {
        rt.block_on(contention::run_contention());
        return;
    }

    if has_flag(&args, "--e2e") {
        let url = arg_value(&args, "--url").unwrap_or("http://127.0.0.1:8080");
        rt.block_on(e2e::run_e2e(url));
        return;
    }

    println!("Combee benchmark (single-threaded client, in-process DataNode)");
    println!("{}", "-".repeat(78));
    rt.block_on(async {
        bench_hot_get().await;
        bench_cold_get().await;
        bench_set(KvDurability::Fast, "SET (fast,  no fsync)").await;
        bench_set(KvDurability::Normal, "SET (normal, WAL fsync)").await;
        bench_set(KvDurability::Strict, "SET (strict, FULL fsync)").await;
        bench_simple_sql().await;
        bench_many_dbs().await;
        mixed::run_mixed().await;
    });
    println!("{}", "-".repeat(78));
    println!(
        "目标(设计文档 §22,不含公网 RTT):hot GET p50<1ms p99<5ms;fast SET p99<5ms;strict SET p99<20ms;cold GET<20ms;SQL p99<20ms"
    );
}

/// 按 `--metadata` 参数构造 MetadataStore。
fn build_metadata(
    args: &[String],
    rt: &tokio::runtime::Runtime,
) -> (Arc<dyn MetadataStore>, &'static str) {
    match arg_value(args, "--metadata").unwrap_or("in-memory") {
        "postgres" | "postgresql" => {
            let url = std::env::var("COMBEE_DATABASE_URL")
                .unwrap_or_else(|_| "postgres://combee:combee@localhost:5432/combee".into());
            let store = rt
                .block_on(PostgresStore::connect(&url))
                .unwrap_or_else(|e| panic!("postgres metadata: {e}"));
            (Arc::new(store), "postgres")
        }
        _ => (Arc::new(InMemoryStore::new()), "in-memory"),
    }
}

// ---- 工具 ----

fn bench_node(data_dir: &std::path::Path, max_active: usize, durability: KvDurability) -> DataNode {
    DataNode::new(DataNodeConfig {
        data_dir: data_dir.to_path_buf(),
        max_active_dbs: max_active,
        db_idle_timeout: Duration::from_secs(300),
        ttl_gc_interval: Duration::from_secs(60),
        kv_cache_capacity: 1_000_000,
        kv_durability: durability,
        sql_timeout: Some(std::time::Duration::from_secs(30)),
    })
}

/// 重新统计:给定样本,输出各百分位与吞吐。
fn report(label: &str, samples: &[Duration]) {
    let mut ns: Vec<u64> = samples.iter().map(|d| d.as_nanos() as u64).collect();
    ns.sort_unstable();
    let n = ns.len();
    if n == 0 {
        println!("{label}: no samples");
        return;
    }
    let pct = |p: f64| ns[((n as f64 * p).floor() as usize).min(n - 1)];
    let mean = ns.iter().sum::<u64>() / n as u64;
    println!(
        "{:<26} n={:<7} p50={:>8.2}µs p90={:>8.2}µs p99={:>9.2}µs max={:>9.2}µs mean={:>8.2}µs",
        label,
        n,
        pct(0.50) as f64 / 1e3,
        pct(0.90) as f64 / 1e3,
        pct(0.99) as f64 / 1e3,
        pct(1.00) as f64 / 1e3,
        mean as f64 / 1e3,
    );
}

// ---- 场景 ----

/// KV hot GET:全部缓存命中,纯内存路径。
async fn bench_hot_get() {
    let dir = tempfile::tempdir().unwrap();
    let node = bench_node(dir.path(), 16, KvDurability::Fast);
    let db = DatabaseId::new();

    // 预写数据(注意 SET 会同时更新缓存)
    let items: Vec<KvSetItem> = (0..HOT_KEYS)
        .map(|i| KvSetItem {
            key: format!("hot:{i}"),
            value: "value".into(),
            ttl_seconds: None,
        })
        .collect();
    node.kv_mset(db, items, 0).await.unwrap();

    let mut rng = rand::thread_rng();
    let mut samples = Vec::with_capacity(HOT_READS);
    for _ in 0..HOT_READS {
        let key = format!("hot:{}", rng.gen_range(0..HOT_KEYS));
        let t0 = Instant::now();
        let e = node.kv_get(db, key).await.unwrap().expect("key exists");
        samples.push(t0.elapsed());
        debug_assert_eq!(e.value, "value");
    }
    let (hits, misses) = node.cache_stats();
    report("KV hot GET (cache hit)", &samples);
    println!(
        "    cache: hits={hits} misses={misses} (hit rate={:.1}%)",
        hits as f64 * 100.0 / (hits + misses) as f64
    );
}

/// KV cold GET:缓存为空(用独立读实例),每个 key 首次访问读 SQLite。
async fn bench_cold_get() {
    let dir = tempfile::tempdir().unwrap();
    let db = DatabaseId::new();

    // 写实例:落盘后退出
    {
        let w = bench_node(dir.path(), 16, KvDurability::Fast);
        for i in 0..COLD_KEYS {
            w.kv_set(
                db,
                format!("cold:{i}"),
                "value".into(),
                None,
                false,
                false,
                0,
            )
            .await
            .unwrap();
        }
        w.shutdown().await;
    }

    // 读实例:缓存为空,全部 miss
    let r = bench_node(dir.path(), 16, KvDurability::Fast);
    let mut samples = Vec::with_capacity(COLD_KEYS);
    for i in 0..COLD_KEYS {
        let key = format!("cold:{i}");
        let t0 = Instant::now();
        let e = r.kv_get(db, key).await.unwrap().expect("key exists");
        samples.push(t0.elapsed());
        debug_assert_eq!(e.value, "value");
    }
    report("KV cold GET (SQLite)", &samples);
}

/// SET 基准:随机 key 覆盖写,不同 durability。
async fn bench_set(durability: KvDurability, label: &str) {
    let dir = tempfile::tempdir().unwrap();
    let node = bench_node(dir.path(), 16, durability);
    let db = DatabaseId::new();

    let ops = if durability == KvDurability::Strict {
        STRICT_OPS
    } else {
        SET_OPS
    };
    let mut rng = rand::thread_rng();
    let mut samples = Vec::with_capacity(ops);
    for _ in 0..ops {
        let key = format!("set:{}", rng.gen_range(0..10_000));
        let t0 = Instant::now();
        node.kv_set(db, key, "value-value-value".into(), None, false, false, 0)
            .await
            .unwrap();
        samples.push(t0.elapsed());
    }
    report(label, &samples);
}

/// Simple SQL:CREATE TABLE 预热,然后 SELECT 与带参数 INSERT。
async fn bench_simple_sql() {
    let dir = tempfile::tempdir().unwrap();
    let node = bench_node(dir.path(), 16, KvDurability::Fast);
    let db = DatabaseId::new();

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

    let mut samples = Vec::with_capacity(SQL_OPS);
    let mut rng = rand::thread_rng();
    for i in 0..SQL_OPS {
        let t0 = Instant::now();
        if i % 5 == 0 {
            node.execute_sql(
                db,
                SqlRequest {
                    sql: "INSERT INTO items (name, score) VALUES (?, ?)".into(),
                    params: vec![
                        serde_json::json!("item"),
                        serde_json::json!(rng.gen_range(0.0..100.0)),
                    ],
                },
                0,
            )
            .await
            .unwrap();
        } else {
            node.execute_sql(
                db,
                SqlRequest {
                    sql: "SELECT 1".into(),
                    params: vec![],
                },
                0,
            )
            .await
            .unwrap();
        }
        samples.push(t0.elapsed());
    }
    report("Simple SQL", &samples);
}

/// many-db:创建 20,000 个逻辑 Cell(无 IO),随机访问少量,
/// 验证活跃连接数被限制在 max_active。
async fn bench_many_dbs() {
    let dir = tempfile::tempdir().unwrap();
    let max_active = 32;
    let node = std::sync::Arc::new(bench_node(dir.path(), max_active, KvDurability::Fast));

    let t0 = Instant::now();
    let dbs: Vec<DatabaseId> = (0..MANY_DBS).map(|_| DatabaseId::new()).collect();
    println!(
        "{:<26} created {MANY_DBS} logical cells in {:.2}ms (no disk IO, lazy)",
        "many-db creation",
        t0.elapsed().as_secs_f64() * 1e3
    );

    // 随机访问 200 个 db
    let mut rng = rand::thread_rng();
    let mut samples = Vec::with_capacity(MANY_TOUCHES);
    for _ in 0..MANY_TOUCHES {
        let db = dbs[rng.gen_range(0..dbs.len())];
        let t0 = Instant::now();
        node.execute_sql(
            db,
            SqlRequest {
                sql: "SELECT 1".into(),
                params: vec![],
            },
            0,
        )
        .await
        .unwrap();
        samples.push(t0.elapsed());
    }
    report("random db touch", &samples);
    println!(
        "    active sqlite connections after touches: {} (cap {max_active})",
        node.active_count()
    );

    // 并发随机访问:确认并发下连接数仍被限制
    let mut set = JoinSet::new();
    for _ in 0..64 {
        let node = node.clone();
        let db = dbs[rng.gen_range(0..dbs.len())];
        set.spawn(async move {
            node.execute_sql(
                db,
                SqlRequest {
                    sql: "SELECT 1".into(),
                    params: vec![],
                },
                0,
            )
            .await
            .unwrap();
        });
    }
    while set.join_next().await.is_some() {}
    println!(
        "    active sqlite connections after 64 concurrent touches: {} (cap {max_active})",
        node.active_count()
    );
}

// ---- CLI 辅助 ----

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

/// 取 `--name value` 形式参数的值。
fn arg_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
}

/// 解析 `--total 10k,100000,1M` 形式的尺寸列表,支持 `k` / `M` 后缀。
fn parse_sizes(args: &[String], name: &str, default: &[usize]) -> Vec<usize> {
    match arg_value(args, name) {
        Some(v) => v
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| parse_size(s.trim()))
            .collect(),
        None => default.to_vec(),
    }
}

fn parse_size(s: &str) -> usize {
    let lower = s.to_ascii_lowercase();
    let (num, mult) = if let Some(n) = lower.strip_suffix('k') {
        (n, 1_000)
    } else if let Some(n) = lower.strip_suffix('m') {
        (n, 1_000_000)
    } else {
        (lower.as_str(), 1)
    };
    num.trim().parse::<usize>().expect("invalid size") * mult
}

fn parse_usize(args: &[String], name: &str, default: usize) -> usize {
    arg_value(args, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
