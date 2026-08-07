//! Capacity benchmark:自动扫描 total Cells × active Cells 的组合,
//! 每个组合测量:
//! - 资源:RSS(kB)、CPU 使用率(%)、fd 数(需 Linux /proc,否则为 "-")
//! - 延迟:KV hot GET 的 p50 / p95 / p99(µs)
//! - cache hit rate 与活跃连接数
//!
//! 输出:stdout 打印 Markdown 表格,并写入 `capacity.csv` / `capacity.md`。

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use combee_common::DatabaseId;
use combee_common::config::KvDurability;
use combee_data_node::{DataNode, DataNodeConfig};
use combee_metadata::{DEFAULT_TENANT, MetadataStore};
use rand::Rng;

use crate::output_path;
use crate::proc;

/// 与 active 规模匹配的连接上限(active=5000 时需 ≥5000)。
const MAX_ACTIVE_DBS: usize = 10_000;
/// 共享缓存条目上限(容纳所有 active cell 的 key)。
const CACHE_CAPACITY: usize = 200_000;

#[derive(Debug, Clone)]
pub struct CapacityRow {
    pub total_cells: usize,
    pub active_cells: usize,
    pub rss_kb: Option<u64>,
    pub cpu_pct: Option<f64>,
    pub fds: Option<u64>,
    pub p50_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
    pub hit_rate: f64,
    pub active_conns: usize,
}

pub async fn run_capacity(
    totals: &[usize],
    actives: &[usize],
    samples: usize,
    metadata: Arc<dyn MetadataStore>,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    println!(
        "Capacity benchmark: total Cells {totals:?} × active Cells {actives:?}, {samples} hot GET samples/phase"
    );
    println!("(RSS/CPU/fd 来自 /proc,需 Linux;非 Linux 显示 -)");
    println!();

    let mut rows = Vec::new();
    let mut cell_pool: Vec<DatabaseId> = Vec::new();
    let mut meta_created = 0usize;

    for &total in totals {
        // 惰性扩池到当前 total,并把新增 Cell 的目录记录写入 metadata
        let new_ids: Vec<DatabaseId> = (cell_pool.len()..total)
            .map(|_| DatabaseId::new())
            .collect();
        if !new_ids.is_empty() {
            let t0 = Instant::now();
            metadata
                .create_databases_batch(DEFAULT_TENANT, &new_ids)
                .await
                .expect("metadata batch create");
            let elapsed = t0.elapsed();
            meta_created += new_ids.len();
            println!(
                "  metadata: created {} records (total {meta_created}) in {:.1}ms",
                new_ids.len(),
                elapsed.as_secs_f64() * 1e3
            );
        }
        cell_pool.extend(new_ids);

        for &active in actives {
            if active > total {
                println!("  skip: active={active} > total={total}");
                continue;
            }
            let row = run_phase(dir.path(), &cell_pool[..total], active, samples).await;
            rows.push(row);
        }
    }

    write_outputs(&rows);
}

async fn run_phase(dir: &Path, cells: &[DatabaseId], active: usize, samples: usize) -> CapacityRow {
    let total = cells.len();
    // 新 DataNode:缓存与连接清零,复用同一 data_dir(SQLite 文件跨阶段复用)
    let node = DataNode::new(DataNodeConfig {
        data_dir: dir.to_path_buf(),
        max_active_dbs: MAX_ACTIVE_DBS,
        db_idle_timeout: std::time::Duration::from_secs(300),
        ttl_gc_interval: std::time::Duration::from_secs(60),
        kv_cache_capacity: CACHE_CAPACITY,
        kv_durability: KvDurability::Fast,
        sql_timeout: Some(std::time::Duration::from_secs(30)),
    });

    let active_cells = &cells[..active];
    let keys: Vec<String> = (0..active).map(|i| format!("k{i}")).collect();

    // 预热:每个 active cell 写入并读一次(创建 SQLite 文件 + 填充缓存)
    for (i, db) in active_cells.iter().enumerate() {
        node.kv_set(*db, keys[i].clone(), "v".into(), None, false, false, 0)
            .await
            .unwrap_or_else(|e| panic!("warmup set {db}: {e}"));
    }
    for (i, db) in active_cells.iter().enumerate() {
        node.kv_get(*db, keys[i].clone())
            .await
            .expect("warmup get")
            .expect("key exists");
    }

    // 采样前资源基线
    let before = proc::read_stats();
    let wall0 = Instant::now();

    // 采样:随机 active cell + 随机 key 的 hot GET(全部缓存命中)
    let mut rng = rand::thread_rng();
    let mut lat = Vec::with_capacity(samples);
    for _ in 0..samples {
        let idx = rng.gen_range(0..active);
        let db = active_cells[idx];
        let key = keys[idx].clone();
        let t0 = Instant::now();
        let entry = node.kv_get(db, key).await.expect("get").expect("hot key");
        lat.push(t0.elapsed());
        debug_assert_eq!(entry.value, "v");
    }
    let wall = wall0.elapsed();
    let after = proc::read_stats();

    let (hits, misses) = node.cache_stats();
    let hit_rate = if hits + misses > 0 {
        hits as f64 * 100.0 / (hits + misses) as f64
    } else {
        0.0
    };

    let (p50, p95, p99) = percentiles(&lat);
    let cpu_pct = proc::cpu_percent(&before, &after, wall.as_secs_f64());
    let conns = node.active_count();

    node.shutdown().await; // 关闭连接,释放 fd 供下一阶段

    let row = CapacityRow {
        total_cells: total,
        active_cells: active,
        rss_kb: after.rss_kb,
        cpu_pct,
        fds: after.fd_count,
        p50_us: p50,
        p95_us: p95,
        p99_us: p99,
        hit_rate,
        active_conns: conns,
    };
    println!("  total={total:<8} active={active:<5} done");
    row
}

fn percentiles(samples: &[std::time::Duration]) -> (f64, f64, f64) {
    let mut ns: Vec<u64> = samples.iter().map(|d| d.as_nanos() as u64).collect();
    ns.sort_unstable();
    let n = ns.len();
    if n == 0 {
        return (0.0, 0.0, 0.0);
    }
    let pct = |p: f64| ns[((n as f64 * p).floor() as usize).min(n - 1)] as f64 / 1e3;
    (pct(0.50), pct(0.95), pct(0.99))
}

fn write_outputs(rows: &[CapacityRow]) {
    let csv_path = output_path("capacity.csv");
    let md_path = output_path("capacity.md");

    // CSV
    let mut csv = String::from(
        "total_cells,active_cells,rss_kb,cpu_pct,fds,p50_us,p95_us,p99_us,hit_rate,active_conns\n",
    );
    for r in rows {
        csv.push_str(&format!(
            "{},{},{},{},{},{:.2},{:.2},{:.2},{:.2},{}\n",
            r.total_cells,
            r.active_cells,
            fmt_opt(r.rss_kb),
            fmt_opt_f(r.cpu_pct),
            fmt_opt(r.fds),
            r.p50_us,
            r.p95_us,
            r.p99_us,
            r.hit_rate,
            r.active_conns,
        ));
    }
    std::fs::write(&csv_path, &csv).expect("write capacity.csv");

    // Markdown
    let mut md = String::from(
        "| total_cells | active_cells | RSS (MB) | CPU % | fd | p50 (µs) | p95 (µs) | p99 (µs) | cache hit % | active conns |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n",
    );
    for r in rows {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {} |\n",
            r.total_cells,
            r.active_cells,
            fmt_opt_mb(r.rss_kb),
            fmt_opt_f(r.cpu_pct),
            fmt_opt(r.fds),
            r.p50_us,
            r.p95_us,
            r.p99_us,
            r.hit_rate,
            r.active_conns,
        ));
    }
    std::fs::write(&md_path, &md).expect("write capacity.md");

    // stdout 打印 Markdown 表格
    println!(
        "\n结果:{} / {}(与工作目录)",
        csv_path.display(),
        md_path.display()
    );
    println!();
    println!("{md}");
}

fn fmt_opt(v: Option<u64>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "-".into())
}

fn fmt_opt_f(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.1}")).unwrap_or_else(|| "-".into())
}

fn fmt_opt_mb(v: Option<u64>) -> String {
    v.map(|kb| format!("{:.1}", kb as f64 / 1024.0))
        .unwrap_or_else(|| "-".into())
}
