//! Control-plane 认证测试:COMBEE_CONTROL_PLANE_TOKEN 保护 /internal/* 与 data-node /rpc/*。
//!
//! 规则:
//! 1. 携带租户 `x-api-key` 的请求**永远**不能调用内部接口(即使 dev 模式、即使 token 正确);
//! 2. 配置 token 时,必须提供 `Authorization: Bearer <token>` 或 `x-control-token: <token>`;
//! 3. 未配置 token(dev):无 x-api-key 即放行。
//! 4. public `/v1/*` 路由不受 internal_auth 影响。

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::{Method, StatusCode};
use combee_api_server::AppState;
use combee_api_server::app::build_app;
use combee_api_server::auth::AuthMode;
use combee_api_server::client::LocalDataNodeClient;
use combee_api_server::client::{DataNodeProvider, LocalProvider};
use combee_api_server::nodes::NodeRegistry;
use combee_common::config::KvDurability;
use combee_data_node::{DataNode, DataNodeConfig};
use combee_metadata::{DEFAULT_TENANT, InMemoryStore, MetadataStore};
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;

const TOKEN: &str = "ctl-very-secret-token";

async fn make_app(control_token: Option<&str>) -> (Router, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let metadata: Arc<dyn MetadataStore> = Arc::new(InMemoryStore::new());
    // 预置一个租户 key(key 模式下 public 接口可用)
    metadata
        .create_api_key(
            DEFAULT_TENANT,
            combee_common::api_key::hash("cmb_sk_test"),
            "default",
        )
        .await
        .unwrap();

    let node = Arc::new(DataNode::new(DataNodeConfig {
        data_dir: dir.path().to_path_buf(),
        max_active_dbs: 16,
        db_idle_timeout: Duration::from_secs(3600),
        ttl_gc_interval: Duration::from_secs(3600),
        kv_cache_capacity: 100_000,
        kv_durability: KvDurability::Normal,
        sql_timeout: Some(Duration::from_secs(5)),
        quota: Default::default(),
    }));
    let client = Arc::new(LocalDataNodeClient::new(node));
    let provider: Arc<dyn DataNodeProvider> = Arc::new(LocalProvider::new(client));
    let usage_meter = combee_api_server::usage::UsageMeter::new(
        metadata.clone(),
        std::time::Duration::from_secs(3600),
    );
    let pricing_meter = combee_api_server::pricing::PricingManager::new(
        metadata.clone(),
        std::time::Duration::from_secs(3600),
    );
    let state = AppState {
        metadata,
        data_node: provider,
        nodes: Arc::new(NodeRegistry::new()),
        auth_mode: AuthMode::Key,
        control_plane_token: control_token.map(str::to_string),
        bff_service_key: None,
        usage: usage_meter,
        pricing: pricing_meter,
        admin_token: None,
        quota: Default::default(),
        concurrency: Default::default(),
    };
    (build_app(state), dir)
}

async fn send(app: &Router, method: Method, uri: &str, headers: &[(&str, &str)]) -> StatusCode {
    let mut req = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = app
        .clone()
        .oneshot(req.body("{}".to_string()).unwrap())
        .await
        .unwrap();
    resp.status()
}

/// 目的:dev 模式(未配置 token)—— 无 key 放行,但携带租户 x-api-key 一律 401。
#[tokio::test]
async fn dev_mode_rejects_tenant_key_on_internal() {
    let (app, _dir) = make_app(None).await;

    // 无任何 key/token → 通过中间件(register 空 body → 400 反序列化失败)
    let status = send(&app, Method::POST, "/internal/nodes/register", &[]).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "dev 模式应放行 internal"
    );

    // 带租户 key → 401,即使 dev 模式
    let status = send(
        &app,
        Method::POST,
        "/internal/nodes/register",
        &[("x-api-key", "cmb_sk_test")],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "租户 key 永远不能进 internal"
    );

    // public 路由不受影响:带租户 key 正常
    let status = send(
        &app,
        Method::GET,
        "/v1/databases",
        &[("x-api-key", "cmb_sk_test")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// 目的:配置 token 后 —— 无/错 token 401;正确 token 放行;租户 key 即使带对 token 也 401。
#[tokio::test]
async fn token_protected_internal_endpoints() {
    let (app, _dir) = make_app(Some(TOKEN)).await;

    // 无 token → 401
    let status = send(&app, Method::POST, "/internal/nodes/register", &[]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 错误 token → 401
    for header in [
        ("authorization", "Bearer wrong"),
        ("x-control-token", "wrong"),
    ] {
        let status = send(&app, Method::POST, "/internal/nodes/register", &[header]).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{header:?} 错误 token 应 401"
        );
    }

    // 正确 token(Bearer 与 x-control-token 均可)→ 通过中间件(400 说明放行)
    for header in [
        ("authorization", "Bearer ctl-very-secret-token"),
        ("x-control-token", "ctl-very-secret-token"),
    ] {
        let status = send(&app, Method::POST, "/internal/nodes/register", &[header]).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{header:?} 正确 token 应放行"
        );
    }

    // 租户 key + 正确 token → 仍然 401(x-api-key 优先拒绝)
    let status = send(
        &app,
        Method::POST,
        "/internal/nodes/register",
        &[
            ("x-api-key", "cmb_sk_test"),
            ("x-control-token", "ctl-very-secret-token"),
        ],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "租户 key + token 也必须拒绝"
    );

    // 错误的租户 key 调 public → 401(public 认证逻辑不变)
    let status = send(
        &app,
        Method::GET,
        "/v1/databases",
        &[("x-api-key", "cmb_sk_wrong")],
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// 目的:unregister / heartbeat / list 等其余 internal 端点同样受保护。
#[tokio::test]
async fn all_internal_endpoints_protected() {
    let (app, _dir) = make_app(Some(TOKEN)).await;

    for uri in [
        "/internal/nodes/heartbeat",
        "/internal/nodes/unregister",
        "/internal/nodes",
        "/internal/nodes/00000000-0000-0000-0000-000000000000/replicas",
    ] {
        let is_get = uri == "/internal/nodes";
        let method = || if is_get { Method::GET } else { Method::POST };
        let status = send(&app, method(), uri, &[]).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri} 无 token 应 401");
        let status = send(&app, method(), uri, &[("x-control-token", TOKEN)]).await;
        assert_ne!(
            status,
            StatusCode::UNAUTHORIZED,
            "{uri} 正确 token 不应 401"
        );
    }
}

// 让 Value 不被视为未使用(发送辅助返回状态码,保留导入语义)
#[allow(dead_code)]
fn _keep(_: Value) {}
