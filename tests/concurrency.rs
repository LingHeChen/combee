//! 并发正确性集成测试(HTTP 栈)。
//!
//! 验证 Active DB Manager 的 per-db 串行化与跨 db 并行能力:
//! - 同一 Cell 的并发 INCR 必须原子(最终值恰好等于请求数);
//! - 同一 Cell 的并发 SET 必须 last-writer-wins(值不撕裂、不报错);
//! - 不同 Cell 的并发 SQL 写入互不干扰。
//!
//! 每个测试的目的与预期结果见 `artifacts/engineering/TESTING.md`。

mod common;

use axum::http::{Method, StatusCode};
use common::{create_db, send, test_app};
use serde_json::json;

/// 目的:20 个并发 INCR 打到同一个 key,最终值必须恰好为 20
/// (证明 per-db 锁保证读改写原子,无丢失更新)。
#[tokio::test]
async fn concurrent_incr_is_atomic_over_http() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    let mut handles = Vec::new();
    for _ in 0..20 {
        let app = app.clone();
        let id = id.clone();
        handles.push(tokio::spawn(async move {
            let (status, body) = send(
                &app,
                Method::POST,
                &format!("/v1/databases/{id}/kv/ops/incr"),
                Some(json!({"key": "c", "delta": 1})),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "incr failed: {body}");
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    // delta=0 仅用于读取当前值
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/incr"),
        Some(json!({"key": "c", "delta": 0})),
        None,
    )
    .await;
    assert_eq!(
        body["value"], 20,
        "all 20 concurrent INCRs must land exactly"
    );
}

/// 目的:10 个并发 SET 写同一 key,最终值必须是其中之一(完整值,不撕裂),
/// 且不产生任何错误。
#[tokio::test]
async fn concurrent_set_same_key_last_writer_wins() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    let mut handles = Vec::new();
    for i in 0..10 {
        let app = app.clone();
        let id = id.clone();
        handles.push(tokio::spawn(async move {
            let value = format!("value-{i}-with-long-suffix");
            let (status, _) = send(
                &app,
                Method::PUT,
                &format!("/v1/databases/{id}/kv/shared"),
                Some(json!({"value": value})),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK);
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let (_, body) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id}/kv/shared"),
        None,
        None,
    )
    .await;
    assert_eq!(body["exists"], true);
    let v = body["value"].as_str().unwrap();
    assert!(
        v.starts_with("value-"),
        "value must be one of the written ones, got {v}"
    );
}

/// 目的:10 个不同的 Cell 并发执行建表+写入+查询,互不干扰
/// (证明不同 Cell 的操作真正并行,且数据隔离)。
#[tokio::test]
async fn concurrent_writes_to_different_dbs_do_not_interfere() {
    let (app, _, _dir) = test_app(16);

    let mut handles = Vec::new();
    for i in 0..10 {
        let app = app.clone();
        handles.push(tokio::spawn(async move {
            let id = create_db(&app).await;
            let (status, _) = send(
                &app,
                Method::POST,
                &format!("/v1/databases/{id}/sql"),
                Some(json!({"sql": "CREATE TABLE t (v INTEGER)"})),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "create table in {id}");
            let (status, _) = send(
                &app,
                Method::POST,
                &format!("/v1/databases/{id}/sql"),
                Some(json!({"sql": "INSERT INTO t VALUES (?)", "params": [i]})),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            let (status, body) = send(
                &app,
                Method::POST,
                &format!("/v1/databases/{id}/sql"),
                Some(json!({"sql": "SELECT v FROM t"})),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                body["rows"],
                json!([[i]]),
                "db {id} must contain only its own data"
            );
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}
