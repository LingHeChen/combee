//! End-to-end benchmark:client → HTTP → API Server → RPC → Data Node。
//!
//! 通过真实 HTTP 打 API Server(独立进程部署时含内部 RPC 一跳),
//! 测量 hot GET / SET / SQL 在并发 1/8/32 下的 p50/p95/p99 与吞吐。
//!
//! 运行:`combee-benchmark --e2e --url http://127.0.0.1:8080`

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde_json::{Value, json};
use tokio::task::JoinSet;

const KEYS: usize = 1_000;
const PHASE_DURATION: Duration = Duration::from_secs(2);
const SAMPLES_PER_WORKER: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Get,
    Set,
    Sql,
}

impl Op {
    fn label(self) -> &'static str {
        match self {
            Op::Get => "GET (cache hit)",
            Op::Set => "SET",
            Op::Sql => "SQL SELECT",
        }
    }
}

#[derive(Debug, Clone)]
pub struct E2ERow {
    pub op: &'static str,
    pub concurrency: usize,
    pub throughput: f64,
    pub p50_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
}

pub async fn run_e2e(url: &str) {
    let base = url.trim_end_matches('/').to_string();
    let http = Arc::new(
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest"),
    );

    // 创建数据库
    let resp = http
        .post(format!("{base}/v1/databases"))
        .send()
        .await
        .expect("create db");
    assert!(
        resp.status().is_success(),
        "create db failed: {}",
        resp.status()
    );
    let db: Value = resp.json().await.expect("create db json");
    let db_id = db["id"].as_str().expect("db id").to_string();
    println!("e2e benchmark: API {base}, db {db_id}, {PHASE_DURATION:?}/phase");

    // 预热:写 1000 keys + SQL 表,并读一遍填充缓存
    for i in 0..KEYS {
        put_kv(&http, &base, &db_id, &format!("k:{i}"), "v").await;
    }
    for i in 0..KEYS {
        get_kv(&http, &base, &db_id, &format!("k:{i}")).await;
    }
    post_sql(
        &http,
        &base,
        &db_id,
        "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT)",
    )
    .await;

    println!();
    let mut rows = Vec::new();
    for &op in &[Op::Get, Op::Set, Op::Sql] {
        for &conc in &[1, 8, 32] {
            let row = run_phase(&http, &base, &db_id, op, conc).await;
            println!("  op={:<15} conc={:<3} done", op.label(), conc);
            rows.push(row);
        }
    }
    write_outputs(&rows);
}

async fn run_phase(
    http: &Arc<reqwest::Client>,
    base: &str,
    db_id: &str,
    op: Op,
    conc: usize,
) -> E2ERow {
    let wall0 = Instant::now();
    let deadline = wall0 + PHASE_DURATION;

    let mut set = JoinSet::new();
    for wid in 0..conc {
        let http = http.clone();
        let base = base.to_string();
        let db_id = db_id.to_string();
        set.spawn(async move {
            let mut rng = StdRng::seed_from_u64(wid as u64 ^ 0xA0761D6478BD642F);
            let mut samples: Vec<Duration> = Vec::with_capacity(SAMPLES_PER_WORKER);
            let mut ops = 0u64;
            while Instant::now() < deadline {
                let key = format!("k:{}", rng.gen_range(0..KEYS));
                let t0 = Instant::now();
                match op {
                    Op::Get => get_kv(&http, &base, &db_id, &key).await,
                    Op::Set => put_kv(&http, &base, &db_id, &key, "v").await,
                    Op::Sql => {
                        post_sql(&http, &base, &db_id, "SELECT id, name FROM items").await;
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
    let mut all = Vec::new();
    while let Some(res) = set.join_next().await {
        let (ops, samples) = res.expect("worker");
        total_ops += ops;
        all.extend(samples);
    }
    let wall = wall0.elapsed().as_secs_f64();
    let (p50, p95, p99) = percentiles(&all);
    E2ERow {
        op: op.label(),
        concurrency: conc,
        throughput: total_ops as f64 / wall,
        p50_us: p50,
        p95_us: p95,
        p99_us: p99,
    }
}

// ---- HTTP helpers ----

async fn get_kv(http: &reqwest::Client, base: &str, db_id: &str, key: &str) {
    let resp = http
        .get(format!("{base}/v1/databases/{db_id}/kv/{key}"))
        .send()
        .await
        .expect("get");
    assert_eq!(resp.status().as_u16(), 200, "GET {key}: {}", resp.status());
}

async fn put_kv(http: &reqwest::Client, base: &str, db_id: &str, key: &str, value: &str) {
    let resp = http
        .put(format!("{base}/v1/databases/{db_id}/kv/{key}"))
        .json(&json!({ "value": value }))
        .send()
        .await
        .expect("put");
    assert_eq!(resp.status().as_u16(), 200, "PUT {key}: {}", resp.status());
}

async fn post_sql(http: &reqwest::Client, base: &str, db_id: &str, sql: &str) {
    let resp = http
        .post(format!("{base}/v1/databases/{db_id}/sql"))
        .json(&json!({ "sql": sql }))
        .send()
        .await
        .expect("sql");
    assert_eq!(resp.status().as_u16(), 200, "SQL {sql}: {}", resp.status());
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

fn write_outputs(rows: &[E2ERow]) {
    let csv_path = Path::new("e2e.csv");
    let md_path = Path::new("e2e.md");

    let mut csv = String::from("operation,concurrency,throughput_ops,p50_us,p95_us,p99_us\n");
    for r in rows {
        csv.push_str(&format!(
            "{},{},{:.0},{:.2},{:.2},{:.2}\n",
            r.op, r.concurrency, r.throughput, r.p50_us, r.p95_us, r.p99_us
        ));
    }
    std::fs::write(csv_path, &csv).expect("write e2e.csv");

    let mut md = String::from(
        "| operation | concurrency | throughput (ops/s) | p50 (µs) | p95 (µs) | p99 (µs) |\n|---|---|---:|---:|---:|---:|\n",
    );
    for r in rows {
        md.push_str(&format!(
            "| {} | {} | {:.0} | {:.2} | {:.2} | {:.2} |\n",
            r.op, r.concurrency, r.throughput, r.p50_us, r.p95_us, r.p99_us
        ));
    }
    std::fs::write(md_path, &md).expect("write e2e.md");

    println!(
        "\n结果:{} / {}(与工作目录)",
        csv_path.display(),
        md_path.display()
    );
    println!();
    println!("{md}");
}
