//! Public API 契约测试(设计文档 P2 §9 / artifacts/engineering/API.md)。
//!
//! 覆盖:request-id 透传与生成、稳定错误 code、Idempotency-Key 幂等创建、
//! /openapi.json 可访问且无内部端点泄漏。

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt;

use common::{create_db, send, test_app};

#[path = "common/mod.rs"]
mod common;

/// 目的:每个响应回显 x-request-id(透传优先,缺失生成);错误响应带稳定 code。
#[tokio::test]
async fn request_id_echo_and_error_code() {
    let (app, _client, _dir) = test_app(16);

    // 透传:请求带 x-request-id → 响应回显
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(Method::GET)
                .uri("/v1/databases")
                .header("x-request-id", "req-abc-123")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.headers()["x-request-id"], "req-abc-123", "透传回显");

    // 缺失 → 生成
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(Method::GET)
                .uri("/v1/databases")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let rid = resp.headers()["x-request-id"].to_str().unwrap().to_string();
    assert!(!rid.is_empty());

    // 错误响应:404 带稳定 code
    let id = create_db(&app).await;
    // 访问不存在的 Cell → 404 + code
    let (status, body) = send(
        &app,
        Method::POST,
        "/v1/databases/00000000-0000-0000-0000-000000000000/sql",
        Some(json!({"sql": "SELECT 1"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "database_not_found", "稳定错误码: {body}");

    // SQL 语法错误 → 400 + code=sql
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "THIS IS NOT SQL"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "sql", "SQL 错误码: {body}");
}

/// 目的:Idempotency-Key —— 同 key 重试返回同一 Cell;不同 key 各自创建。
#[tokio::test]
async fn idempotency_key_cell_creation() {
    let (app, _client, _dir) = test_app(16);

    let create = |key: String| {
        let app = app.clone();
        async move {
            app.clone()
                .oneshot(
                    axum::http::Request::builder()
                        .method(Method::POST)
                        .uri("/v1/databases")
                        .header("idempotency-key", key)
                        .body(axum::body::Body::from("{}".to_string()))
                        .unwrap(),
                )
                .await
                .unwrap()
        }
    };

    let r1 = create("idem-1".to_string()).await;
    let s1 = r1.status();
    let body1: Value =
        serde_json::from_slice(&axum::body::to_bytes(r1.into_body(), 1024).await.unwrap()).unwrap();
    assert_eq!(s1, StatusCode::CREATED);
    let id1 = body1["id"].as_str().unwrap().to_string();

    // 同 key 重试 → 200 + 同一 id
    let r2 = create("idem-1".to_string()).await;
    let s2 = r2.status();
    let body2: Value =
        serde_json::from_slice(&axum::body::to_bytes(r2.into_body(), 1024).await.unwrap()).unwrap();
    assert_eq!(s2, StatusCode::OK, "同 key 重试幂等");
    assert_eq!(body2["id"].as_str().unwrap(), id1, "返回首次创建的 Cell");

    // 不同 key → 新 Cell
    let r3 = create("idem-2".to_string()).await;
    assert_eq!(r3.status(), StatusCode::CREATED);
}

/// 目的:/openapi.json 可访问,包含核心路径,不含 internal/admin/rpc。
#[tokio::test]
async fn openapi_served_without_internal_leak() {
    let (app, _client, _dir) = test_app(16);
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(Method::GET)
                .uri("/openapi.json")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 10 * 1024 * 1024)
        .await
        .unwrap();
    let doc: Value = serde_json::from_slice(&bytes).unwrap();
    let paths = doc["paths"].as_object().unwrap();
    assert!(paths.contains_key("/v1/databases"), "核心路径存在");
    assert!(paths.contains_key("/v1/credits/redeem"));
    for p in paths.keys() {
        assert!(
            !p.starts_with("/internal") && !p.starts_with("/admin") && !p.starts_with("/rpc"),
            "内部端点泄漏: {p}"
        );
    }
}
