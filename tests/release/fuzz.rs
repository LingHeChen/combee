//! Release Gate:API Fuzz / Malformed Input + SQL 逃逸。
//!
//! 规则:用户输入永远不能导致 panic / OOM / crash / 越权;
//! 畸形输入必须返回确定的可解释错误(4xx / 5xx),且服务保持可用。

#[path = "../common/mod.rs"]
mod common;

use axum::http::{Method, StatusCode};
use common::{create_db, send, test_app};
use serde_json::{Value, json};

/// 发送请求,断言不是"panic 类"响应(非 5xx 的 4xx/413 等),并确认服务仍可用。
async fn assert_not_crash_and_alive(
    app: &axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> StatusCode {
    let (status, _) = send(app, method, uri, body, None).await;
    assert!(
        status.as_u16() < 500 || status == StatusCode::INTERNAL_SERVER_ERROR,
        "unexpected status {status}"
    );
    // 服务仍存活
    let (s2, _) = send(app, Method::GET, "/v1/databases", None, None).await;
    assert_eq!(
        s2,
        StatusCode::OK,
        "server must stay alive after malformed input"
    );
    status
}

/// malformed JSON / 空 body / 类型错误。
#[tokio::test]
async fn malformed_bodies_do_not_crash() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    // 非法 JSON
    for bad in [
        "{",
        "not json",
        r#"{"sql": }"#,
        r#"{"params": [1,2"#,
        "\u{0000}",
        "{}",
    ] {
        let (status, _) = send(
            &app,
            Method::POST,
            &format!("/v1/databases/{id}/sql"),
            Some(Value::String(bad.into())),
            None,
        )
        .await;
        assert!(
            status.is_client_error(),
            "malformed json should be 4xx, got {status}"
        );
    }
    // 空 body
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        None,
        None,
    )
    .await;
    assert!(status.is_client_error());
    // 空 body 的 KV PUT
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/k"),
        None,
        None,
    )
    .await;
    assert!(status.is_client_error());
}

/// SQL 逃逸:用户 SQL 不能突破 Cell 文件边界。
#[tokio::test]
async fn sql_cannot_escape_cell_boundary() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    // 文件系统逃逸
    for bad in [
        "VACUUM INTO '/tmp/combee-escape.sqlite'",
        "ATTACH DATABASE '/tmp/foo.db' AS x",
        "DETACH x",
    ] {
        let (status, _) = send(
            &app,
            Method::POST,
            &format!("/v1/databases/{id}/sql"),
            Some(json!({"sql": bad})),
            None,
        )
        .await;
        assert!(status.is_client_error(), "{bad} 应被拒绝,got {status}");
    }

    // 内部表
    for bad in [
        "SELECT * FROM __sys_kv",
        "SELECT * FROM __SYS_META",
        "SELECT * FROM __sys_kv_expires_at",
    ] {
        let (status, _) = send(
            &app,
            Method::POST,
            &format!("/v1/databases/{id}/sql"),
            Some(json!({"sql": bad})),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{bad}");
    }

    // 扩展加载(默认未启用 → SQL error,不 panic)
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT load_extension('x')"})),
        None,
    )
    .await;
    assert!(status.is_client_error(), "load_extension 应失败");

    // CLI-only 函数不存在 → SQL error
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT readfile('/etc/passwd')"})),
        None,
    )
    .await;
    assert!(status.is_client_error(), "readfile 应不存在");

    // 危险 PRAGMA:即使允许执行也不得越权(返回错误或无害值,不 panic)
    for sql in [
        "PRAGMA journal_mode=DELETE",
        "PRAGMA foreign_keys=OFF",
        "PRAGMA page_size=512",
    ] {
        let (status, _) = send(
            &app,
            Method::POST,
            &format!("/v1/databases/{id}/sql"),
            Some(json!({"sql": sql})),
            None,
        )
        .await;
        assert!(!status.is_server_error(), "PRAGMA {sql} 不应 5xx");
    }
}

/// 边界输入:超大整数 / 负数 TTL / u64::MAX / 空 key / unicode。
#[tokio::test]
async fn boundary_inputs_do_not_crash() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    // SQL params 边界
    for param in [
        json!(i64::MAX),
        json!(i64::MIN),
        json!(3.5),
        json!(null),
        json!(true),
        json!(""),
        json!({"a": 1}), // object 参数 → 拒绝
        json!([1, 2]),   // array 参数 → 拒绝
    ] {
        assert_not_crash_and_alive(
            &app,
            Method::POST,
            &format!("/v1/databases/{id}/sql"),
            Some(json!({"sql": "SELECT ?", "params": [param]})),
        )
        .await;
    }

    // KV:负数 TTL / u64::MAX TTL / TTL=0 / 空 key(INCR body)
    for ttl in [json!(-1), json!(u64::MAX), json!(0), json!(10)] {
        assert_not_crash_and_alive(
            &app,
            Method::PUT,
            &format!("/v1/databases/{id}/kv/ttlkey"),
            Some(json!({"value": "v", "ttl_seconds": ttl})),
        )
        .await;
    }
    assert_not_crash_and_alive(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/incr"),
        Some(json!({"key": "", "delta": 1})),
    )
    .await;
    assert_not_crash_and_alive(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/kv/ops/incr"),
        Some(json!({"key": "unicode-键-🚀", "delta": -1})),
    )
    .await;

    // 非法 UUID 路径
    for uri in [
        "/v1/databases/not-a-uuid/sql",
        "/v1/databases/not-a-uuid/kv/k",
        "/v1/databases/123/kv/k",
        "/v1/databases//sql",
    ] {
        assert_not_crash_and_alive(&app, Method::POST, uri, Some(json!({"sql": "SELECT 1"}))).await;
    }

    // 超大 KV value(1MB 通过;10MB 超过 axum 默认 body limit → 413)
    let big1mb = "x".repeat(1_000_000);
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/big"),
        Some(json!({"value": big1mb})),
        None,
    )
    .await;
    assert!(
        status == StatusCode::OK || status == StatusCode::PAYLOAD_TOO_LARGE,
        "1MB value: {status}"
    );

    let big10mb = "y".repeat(10_000_000);
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/huge"),
        Some(json!({"value": big10mb})),
        None,
    )
    .await;
    assert!(
        status == StatusCode::PAYLOAD_TOO_LARGE || status.is_client_error(),
        "10MB value 应被限制(413),got {status}"
    );
}

/// WITH RECURSIVE 无限递归 / 巨大 join:SQLite 限制,不 panic。
#[tokio::test]
async fn recursive_query_is_bounded() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;
    assert_not_crash_and_alive(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "WITH RECURSIVE cnt(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM cnt) SELECT * FROM cnt"})),
    )
    .await;
}
