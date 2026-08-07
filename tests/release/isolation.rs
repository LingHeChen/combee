//! Release Gate:Cell / Tenant Isolation(P0 门槛)。
//!
//! 现状说明:当前所有 Cell 属于单一默认租户(dev 模式),隔离模型为
//! **按 UUID 隔离**(不知道 Cell id 就无法访问)。本测试如实验证:
//! - 猜测 UUID → 拒绝;互不可见;
//! - SQL 无法突破 Cell 文件边界(__sys / ATTACH / VACUUM INTO 均拒绝);
//! - 同租户下"拿到 id 即可操作"是已知单租户模型(记录为 HIGH 隔离缺口)。

#[path = "../common/mod.rs"]
mod common;

use axum::http::{Method, StatusCode};
use common::{create_db, send, test_app};
use serde_json::json;

/// 猜测 UUID / 不存在的 Cell → 404,且无法跨 Cell 读取。
#[tokio::test]
async fn guessing_cell_id_is_rejected() {
    let (app, _, _dir) = test_app(16);

    // 随机/伪造 UUID
    for ghost in [
        "00000000-0000-0000-0000-000000000000",
        "ffffffff-ffff-ffff-ffff-ffffffffffff",
    ] {
        for (method, uri, body) in [
            (Method::GET, format!("/v1/databases/{ghost}/kv/k"), None),
            (
                Method::POST,
                format!("/v1/databases/{ghost}/sql"),
                Some(json!({"sql": "SELECT 1"})),
            ),
            (Method::DELETE, format!("/v1/databases/{ghost}"), None),
        ] {
            let (status, _) = send(&app, method, &uri, body, None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        }
    }

    // 合法但随机的 UUID(极不可能已创建)→ 404
    let random = combee_common::DatabaseId::new().to_string();
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{random}/sql"),
        Some(json!({"sql": "SELECT 1"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// 两个 Cell 同名数据互不可见(kv + sql),删除/restore/replication 均需 Cell id。
#[tokio::test]
async fn cells_are_isolated_by_id() {
    let (app, _, _dir) = test_app(16);
    let a = create_db(&app).await;
    let b = create_db(&app).await;

    // A 写 secret
    send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{a}/kv/secret"),
        Some(json!({"value": "a-secret"})),
        None,
    )
    .await;
    send(
        &app,
        Method::POST,
        &format!("/v1/databases/{a}/sql"),
        Some(json!({"sql": "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)"})),
        None,
    )
    .await;
    send(
        &app,
        Method::POST,
        &format!("/v1/databases/{a}/sql"),
        Some(json!({"sql": "INSERT INTO users (name) VALUES ('alice')"})),
        None,
    )
    .await;

    // B 写同名但内容不同
    send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{b}/kv/secret"),
        Some(json!({"value": "b-secret"})),
        None,
    )
    .await;
    send(
        &app,
        Method::POST,
        &format!("/v1/databases/{b}/sql"),
        Some(json!({"sql": "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)"})),
        None,
    )
    .await;
    send(
        &app,
        Method::POST,
        &format!("/v1/databases/{b}/sql"),
        Some(json!({"sql": "INSERT INTO users (name) VALUES ('bob')"})),
        None,
    )
    .await;

    // 各自读回自己的数据,互不干扰
    let (_, body) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{a}/kv/secret"),
        None,
        None,
    )
    .await;
    assert_eq!(body["value"], "a-secret");
    let (_, body) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{b}/kv/secret"),
        None,
        None,
    )
    .await;
    assert_eq!(body["value"], "b-secret");
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{a}/sql"),
        Some(json!({"sql": "SELECT name FROM users"})),
        None,
    )
    .await;
    assert_eq!(body["rows"], json!([["alice"]]));
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{b}/sql"),
        Some(json!({"sql": "SELECT name FROM users"})),
        None,
    )
    .await;
    assert_eq!(body["rows"], json!([["bob"]]));
}

/// SQL 无法突破 Cell 文件边界:__sys 表 / ATTACH / VACUUM INTO 全部拒绝。
#[tokio::test]
async fn sql_cannot_escape_filesystem() {
    let (app, _, _dir) = test_app(16);
    let id = create_db(&app).await;

    for bad in [
        "SELECT * FROM __sys_kv",
        "INSERT INTO __sys_meta VALUES ('x','y')",
        "ATTACH DATABASE '/tmp/esc.db' AS x",
        "DETACH x",
        "VACUUM INTO '/tmp/esc.sqlite'",
        "VACUUM",
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
    // 多语句注入
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT 1; ATTACH DATABASE '/tmp/esc.db' AS x"})),
        None,
    )
    .await;
    assert!(status.is_client_error());
}
