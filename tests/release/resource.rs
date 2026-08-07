//! Release Gate:Resource Exhaustion + Noisy Neighbor。
//!
//! - 资源:大量 Cell、超大 SQL 结果、并发连接上限不泄漏;
//!   ENOSPC 无法在本机可靠模拟 → 记录为 WARN(需容器小文件系统验证);
//! - noisy neighbor:Cell A 重负载下,Cell B 的 p99 退化 < 5x(Alpha 宽松门槛)。

#[path = "../common/mod.rs"]
mod common;

use std::time::{Duration, Instant};

use axum::http::{Method, StatusCode};
use combee_api_server::client::DataNodeClient;
use common::{create_db, send, test_app};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde_json::json;
use tokio::task::JoinSet;

/// 大量 Cell(1000 个)创建与使用正常。
#[tokio::test]
async fn many_cells_work() {
    let (app, _, _dir) = test_app(16);
    let mut ids = Vec::new();
    for _ in 0..1000 {
        ids.push(create_db(&app).await);
    }
    // 抽查 20 个可读写
    for id in ids.iter().take(20) {
        let (s, _) = send(
            &app,
            Method::PUT,
            &format!("/v1/databases/{id}/kv/k"),
            Some(json!({"value": "v"})),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let (s, body) = send(
            &app,
            Method::GET,
            &format!("/v1/databases/{id}/kv/k"),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(body["value"], "v");
    }
}

/// 超大 SQL 结果(100k 行)返回或明确失败,不崩溃;之后服务仍可用。
#[tokio::test]
async fn large_sql_result_is_bounded() {
    let (app, client, _dir) = test_app(2);
    let id = create_db(&app).await;
    send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "CREATE TABLE t (x INTEGER)"})),
        None,
    )
    .await;
    send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/transaction"),
        Some(json!({"statements": [
            {"sql": "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c LIMIT 100000) INSERT INTO t SELECT x FROM c"}
        ]})),
        None,
    )
    .await;
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT * FROM t"})),
        None,
    )
    .await;
    // 100k 行:返回(可能大)或明确失败,但不崩溃
    assert!(
        status.is_success() || status.is_client_error(),
        "unexpected {status}"
    );
    if status.is_success() {
        assert_eq!(body["rows"].as_array().unwrap().len(), 100_000);
    }
    // 服务仍可用
    let (s, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT 1"})),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(client.active_count() <= 2, "连接数不泄漏");
}

/// 并发连接上限:max_active=2 时并发打 3 个 Cell,连接数不超上限。
#[tokio::test]
async fn connection_limit_is_respected_under_load() {
    let (app, client, _dir) = test_app(2);
    let mut dbs = Vec::new();
    for _ in 0..3 {
        dbs.push(create_db(&app).await);
    }
    let mut set = JoinSet::new();
    for _ in 0..20 {
        let app = app.clone();
        let dbs = dbs.clone();
        set.spawn(async move {
            let mut rng = StdRng::from_entropy();
            let idx = rng.gen_range(0..dbs.len());
            send(
                &app,
                Method::POST,
                &format!("/v1/databases/{}/sql", dbs[idx]),
                Some(json!({"sql": "SELECT 1"})),
                None,
            )
            .await;
        });
    }
    while set.join_next().await.is_some() {}
    assert!(
        client.active_count() <= 2,
        "active conns: {}",
        client.active_count()
    );
}

/// Noisy neighbor:Cell A 重负载下,Cell B 的 GET p99 退化 < 5x。
#[tokio::test]
async fn noisy_neighbor_does_not_break_other_cells() {
    let (app, _, _dir) = test_app(64);
    let a = create_db(&app).await;
    let b = create_db(&app).await;

    // B 预热 100 keys
    for i in 0..100 {
        send(
            &app,
            Method::PUT,
            &format!("/v1/databases/{b}/kv/k{i}"),
            Some(json!({"value": "v"})),
            None,
        )
        .await;
    }
    // 测量 B 基线 p99(单 worker 顺序读)
    let mut rng = StdRng::from_entropy();
    let mut base = Vec::new();
    for _ in 0..200 {
        let t0 = Instant::now();
        send(
            &app,
            Method::GET,
            &format!("/v1/databases/{b}/kv/k{}", rng.gen_range(0..100)),
            None,
            None,
        )
        .await;
        base.push(t0.elapsed());
    }
    let base_p99 = percentile(&base);

    // A 重负载:10 个 worker 并发 SET + 复杂 SQL
    let mut set = JoinSet::new();
    for _ in 0..10 {
        let app = app.clone();
        let a = a.clone();
        set.spawn(async move {
            let mut rng = StdRng::from_entropy();
            let deadline = Instant::now() + Duration::from_millis(800);
            while Instant::now() < deadline {
                send(
                    &app,
                    Method::PUT,
                    &format!("/v1/databases/{a}/kv/load:{}", rng.gen_range(0..50)),
                    Some(json!({"value": "x".repeat(1024)})),
                    None,
                )
                .await;
                let _ = send(
                    &app,
                    Method::POST,
                    &format!("/v1/databases/{a}/sql"),
                    Some(json!({"sql": "SELECT count(*) FROM (SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3)"})),
                    None,
                )
                .await;
            }
        });
    }
    // A 负载期间测 B
    let mut under = Vec::new();
    for _ in 0..200 {
        let t0 = Instant::now();
        send(
            &app,
            Method::GET,
            &format!("/v1/databases/{b}/kv/k{}", rng.gen_range(0..100)),
            None,
            None,
        )
        .await;
        under.push(t0.elapsed());
    }
    while set.join_next().await.is_some() {}
    let under_p99 = percentile(&under);
    let ratio = under_p99 / base_p99.max(1.0);
    // Alpha 宽松门槛:< 5x
    assert!(
        ratio < 5.0,
        "B p99 退化 {ratio:.1}x (base {base_p99:.1}µs → under {under_p99:.1}µs) 超过 5x"
    );
}

fn percentile(samples: &[Duration]) -> f64 {
    let mut ns: Vec<u64> = samples.iter().map(|d| d.as_nanos() as u64).collect();
    ns.sort_unstable();
    let n = ns.len();
    ns[(n as f64 * 0.99).floor() as usize] as f64 / 1e3
}
