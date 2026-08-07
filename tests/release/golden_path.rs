//! Release Gate:Golden Path E2E —— 真实小型 Web App workload。
//!
//! SQL(users / posts)+ KV(session / page-cache / pageviews counter),
//! 精确校验 SQL / KV / TTL / counter,并验证 Data Node 重启后数据保持。

#[path = "../common/mod.rs"]
mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::http::{Method, StatusCode};
use combee_common::DatabaseId;
use combee_common::config::KvDurability;
use combee_data_node::{DataNode, DataNodeConfig};
use common::{create_db, send, test_app};
use serde_json::json;

/// HTTP 层真实 workload:users + posts + session + cache + counter。
#[tokio::test]
async fn golden_path_web_app_workload() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    // ---- SQL ----
    for sql in [
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT UNIQUE)",
        "CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT, body TEXT)",
        "CREATE INDEX idx_posts_user ON posts(user_id)",
    ] {
        let (status, _) = send(
            &app,
            Method::POST,
            &format!("/v1/databases/{id}/sql"),
            Some(json!({"sql": sql})),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{sql}");
    }
    // 一个用户 + 100 篇 post(参数绑定)
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "INSERT INTO users (name, email) VALUES (?, ?)", "params": ["alice", "alice@example.com"]})),
        None,
    )
    .await;
    assert_eq!(body["rows_affected"], 1);
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/transaction"),
        Some(json!({"statements": [
            {"sql": "INSERT INTO posts (user_id, title) VALUES (1, 'post-1')"},
            {"sql": "INSERT INTO posts (user_id, title) VALUES (1, 'post-2')"},
        ]})),
        None,
    )
    .await;
    assert_eq!(body.as_array().unwrap().len(), 2);

    // ---- KV ----
    send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/session:user:1"),
        Some(json!({"value": "token-abc", "ttl_seconds": 600})),
        None,
    )
    .await;
    send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/cache:homepage"),
        Some(json!({"value": "<html>cached</html>", "ttl_seconds": 300})),
        None,
    )
    .await;
    send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/incr"),
        Some(json!({"key": "pageviews", "delta": 1})),
        None,
    )
    .await;
    send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/incr"),
        Some(json!({"key": "pageviews", "delta": 1})),
        None,
    )
    .await;

    // ---- 精确校验 ----
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT u.name, COUNT(p.id) AS n FROM users u LEFT JOIN posts p ON p.user_id = u.id GROUP BY u.id"})),
        None,
    )
    .await;
    assert_eq!(body["rows"], json!([["alice", 2]]));

    let (_, body) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id}/kv/session:user:1"),
        None,
        None,
    )
    .await;
    assert_eq!(body["value"], "token-abc");
    let ttl = body["ttl_seconds"].as_i64().unwrap();
    assert!((1..=600).contains(&ttl), "session ttl in range, got {ttl}");

    let (_, body) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id}/kv/cache:homepage"),
        None,
        None,
    )
    .await;
    assert_eq!(body["value"], "<html>cached</html>");

    let (st, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/incr"),
        Some(json!({"key": "pageviews", "delta": 0})),
        None,
    )
    .await;
    eprintln!("INCR delta=0 -> status={st} body={body}");
    assert_eq!(body["value"], 2, "counter 精确为 2");

    // ---- 更新/删除 ----
    send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "UPDATE users SET name = ? WHERE id = ?", "params": ["alice2", 1]})),
        None,
    )
    .await;
    send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "DELETE FROM posts WHERE id = 1"})),
        None,
    )
    .await;
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT name FROM users WHERE id = 1"})),
        None,
    )
    .await;
    assert_eq!(body["rows"], json!([["alice2"]]));
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT COUNT(*) AS n FROM posts"})),
        None,
    )
    .await;
    assert_eq!(body["rows"], json!([[1]]));
}

/// 重启持久性:DataNode 写数据 → shutdown(checkpoint)→ 新实例同数据目录 → 全部读回。
#[tokio::test]
async fn restart_preserves_web_app_state() {
    let dir = tempfile::tempdir().unwrap();
    let db = DatabaseId::new();
    {
        let n = DataNode::new(DataNodeConfig {
            data_dir: dir.path().to_path_buf(),
            max_active_dbs: 8,
            db_idle_timeout: Duration::from_secs(3600),
            ttl_gc_interval: Duration::from_secs(3600),
            kv_cache_capacity: 10_000,
            kv_durability: KvDurability::Normal,
            sql_timeout: Some(std::time::Duration::from_secs(30)),
        });
        n.execute_sql(
            db,
            combee_common::protocol::SqlRequest {
                sql: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)".into(),
                params: vec![],
            },
            0,
        )
        .await
        .unwrap();
        n.execute_sql(
            db,
            combee_common::protocol::SqlRequest {
                sql: "INSERT INTO users (name) VALUES ('alice')".into(),
                params: vec![],
            },
            0,
        )
        .await
        .unwrap();
        n.kv_set(
            db,
            "session:user:1".into(),
            "token-abc".into(),
            Some(600),
            false,
            false,
            0,
        )
        .await
        .unwrap();
        n.kv_incr(db, "pageviews".into(), 5, None, 0).await.unwrap();
        n.shutdown().await;
    }
    // 模拟重启:全新 DataNode 实例,同一数据目录(连接/缓存全空)
    let n = DataNode::new(DataNodeConfig {
        data_dir: dir.path().to_path_buf(),
        max_active_dbs: 8,
        db_idle_timeout: Duration::from_secs(3600),
        ttl_gc_interval: Duration::from_secs(3600),
        kv_cache_capacity: 10_000,
        kv_durability: KvDurability::Normal,
        sql_timeout: Some(std::time::Duration::from_secs(30)),
    });
    let r = n
        .execute_sql(
            db,
            combee_common::protocol::SqlRequest {
                sql: "SELECT name FROM users".into(),
                params: vec![],
            },
            0,
        )
        .await
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![serde_json::json!("alice")]],
        "SQL 重启后仍在"
    );
    let e = n
        .kv_get(db, "session:user:1".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(e.value, "token-abc", "KV 重启后仍在");
    let ttl = e.ttl_seconds.unwrap();
    assert!((1..=600).contains(&ttl), "TTL 重启后仍有效,got {ttl}");
    let v = n.kv_incr(db, "pageviews".into(), 0, None, 0).await.unwrap();
    assert_eq!(v, 5, "counter 重启后精确");
    let _ = Arc::new(());
}
