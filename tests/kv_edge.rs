//! KV 边界与错误路径集成测试(HTTP 栈)。
//!
//! 覆盖:空 key 拒绝、不存在的 db 全 404、unicode 与大 value 往返、
//! 端点保留名作为普通 key、INCR 带 TTL。
//! 每个测试的目的与预期结果见 `docs/TESTING.md`。

mod common;

use axum::http::{Method, StatusCode};
use common::{create_db, send, test_app};
use serde_json::json;

/// 目的:空 key 必须被拒绝(400),包括 INCR 的 body key 与 MSET 的 item key。
#[tokio::test]
async fn empty_key_rejected() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/incr"),
        Some(json!({"key": "", "delta": 1})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "empty incr key");

    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/mset"),
        Some(json!({"items": [{"key": "", "value": "x"}]})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "empty mset key");
}

/// 目的:对不存在的数据库执行任何 KV 操作都必须返回 404(租户隔离 + 存在性校验)。
#[tokio::test]
async fn missing_database_returns_404_for_kv() {
    let (app, _, _dir) = test_app(16);
    let ghost = "00000000-0000-0000-0000-000000000000";

    let cases: [(Method, String, Option<serde_json::Value>); 4] = [
        (Method::GET, format!("/v1/databases/{ghost}/kv/k"), None),
        (
            Method::PUT,
            format!("/v1/databases/{ghost}/kv/k"),
            Some(json!({"value": "x"})),
        ),
        (Method::DELETE, format!("/v1/databases/{ghost}/kv/k"), None),
        (
            Method::POST,
            format!("/v1/databases/{ghost}/kv/ops/incr"),
            Some(json!({"key": "k"})),
        ),
    ];
    for (method, uri, body) in cases {
        let (status, _) = send(&app, method, &uri, body, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "expected 404 for {uri}");
    }
}

/// 目的:unicode key/value 与 100KB 大 value 的完整往返
/// (UTF-8 编解码与较大 payload 不出错)。
#[tokio::test]
async fn unicode_and_large_values_roundtrip() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    let unicode_key = "会话:用户:42";
    let unicode_value = "你好,Combee!🚀";
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/{unicode_key}"),
        Some(json!({"value": unicode_value})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id}/kv/{unicode_key}"),
        None,
        None,
    )
    .await;
    assert_eq!(body["value"], unicode_value);

    let big = "x".repeat(100_000);
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/big"),
        Some(json!({"value": big})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id}/kv/big"),
        None,
        None,
    )
    .await;
    assert_eq!(body["value"].as_str().unwrap().len(), 100_000);
}

/// 目的:与 KV 操作端点同名的字符串("exists"/"mget"/"ttl"/"expire"/"incr")
/// 仍可作为普通 key 读写(GET/PUT 走参数路径,静态路径仅拦截 POST)。
#[tokio::test]
async fn reserved_endpoint_words_can_be_used_as_keys() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    for key in ["exists", "mget", "ttl", "expire", "incr"] {
        let (status, body) = send(
            &app,
            Method::PUT,
            &format!("/v1/databases/{id}/kv/{key}"),
            Some(json!({"value": key})),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "PUT {key}: {body}");
        let (_, body) = send(
            &app,
            Method::GET,
            &format!("/v1/databases/{id}/kv/{key}"),
            None,
            None,
        )
        .await;
        assert_eq!(body["exists"], true, "GET {key}");
        assert_eq!(body["value"], key);
    }
}

/// 目的:INCR 首次携带 TTL,写入后 GET 应能读到剩余秒数。
#[tokio::test]
async fn incr_with_ttl_over_http() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/incr"),
        Some(json!({"key": "c", "delta": 5, "ttl_seconds": 100})),
        None,
    )
    .await;
    assert_eq!(body["value"], 5);

    let (_, body) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id}/kv/c"),
        None,
        None,
    )
    .await;
    let ttl = body["ttl_seconds"].as_i64().unwrap();
    assert!(
        (1..=100).contains(&ttl),
        "incr with ttl should leave remaining ttl, got {ttl}"
    );
}
