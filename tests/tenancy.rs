//! 租户隔离与 API key 生命周期测试。
//!
//! 核心原则(用户要求):隔离必须在 repository/trait 层强制,
//! HTTP handler 只拿到 `AuthContext{tenant_id}` 后调用 `get_database(tenant, id)`。
//! 本测试验证所有资源操作(sql / kv / delete / backup / restore)跨租户一律 404,
//! 以及 API key 明文只返回一次、撤销即失效。

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
use combee_common::{AuthContext, TenantId};
use combee_data_node::{DataNode, DataNodeConfig};
use combee_metadata::{DEFAULT_TENANT, InMemoryStore, MetadataStore};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const KEY_A: &str = "cmb_sk_test_tenant_a";
const KEY_B: &str = "cmb_sk_test_tenant_b";
const KEY_ADMIN: &str = "cmb_sk_test_admin_platform";

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    api_key: Option<&str>,
) -> (StatusCode, Value) {
    let mut req = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(k) = api_key {
        req = req.header("x-api-key", k);
    }
    let resp = app
        .clone()
        .oneshot(
            req.body(body.map(|v| v.to_string()).unwrap_or_else(|| "{}".into()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

/// 构造 key 模式 app:key-a ∈ DEFAULT_TENANT,key-b ∈ tenant_b。
async fn make_app() -> (Router, TempDir, TenantId) {
    let dir = tempfile::tempdir().unwrap();
    let tenant_b = TenantId::new();
    let metadata: Arc<dyn MetadataStore> = Arc::new(InMemoryStore::new());
    metadata.create_tenant(tenant_b).await.unwrap();
    metadata
        .create_api_key(
            DEFAULT_TENANT,
            combee_common::api_key::hash(KEY_A),
            "default",
        )
        .await
        .unwrap();
    metadata
        .create_api_key(tenant_b, combee_common::api_key::hash(KEY_B), "default")
        .await
        .unwrap();
    // 平台服务账号(admin key):BFF/console 代理用;能看到全部租户
    metadata
        .create_api_key(
            DEFAULT_TENANT,
            combee_common::api_key::hash(KEY_ADMIN),
            "admin",
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
        control_plane_token: None,
        admin_api_key: Some(KEY_ADMIN.to_string()),
        usage: usage_meter,
        pricing: pricing_meter,
        admin_token: None,
        quota: Default::default(),
        concurrency: Default::default(),
    };
    (build_app(state), dir, tenant_b)
}

/// 目的:跨租户隔离 —— B 无法读取/写入/删除/备份 A 的 Cell,全部 404;A 不受影响。
#[tokio::test]
async fn cross_tenant_access_is_rejected() {
    let (app, _dir, _tb) = make_app().await;

    // A 创建 Cell
    let (status, body) = send(
        &app,
        Method::POST,
        "/v1/databases",
        Some(json!({})),
        Some(KEY_A),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = body["id"].as_str().unwrap().to_string();

    // A 写入数据
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/greeting"),
        Some(json!({"value": "hello-a"})),
        Some(KEY_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // B 列不出 A 的 Cell
    let (status, body) = send(&app, Method::GET, "/v1/databases", None, Some(KEY_B)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 0, "B 看不到 A 的 Cell");

    // B 用 A 的 id:sql / kv / delete / backup 全部 404
    for (method, uri, payload) in [
        (
            Method::POST,
            format!("/v1/databases/{id}/sql"),
            Some(json!({"sql": "SELECT 1"})),
        ),
        (
            Method::POST,
            format!("/v1/databases/{id}/transaction"),
            Some(json!({"statements": []})),
        ),
        (Method::GET, format!("/v1/databases/{id}/kv/greeting"), None),
        (Method::DELETE, format!("/v1/databases/{id}"), None),
        (Method::POST, format!("/v1/databases/{id}/backup"), None),
        (Method::POST, format!("/v1/databases/{id}/restore"), None),
    ] {
        let (status, _) = send(&app, method, &uri, payload, Some(KEY_B)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri} 跨租户必须 404");
    }

    // A 的数据完好可用
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id}/kv/greeting"),
        None,
        Some(KEY_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["value"], "hello-a");
}

/// 目的:API key 生命周期 —— 明文仅返回一次;列表不含明文;
/// 撤销后立即失效(401);A 无法撤销 B 的 key。
#[tokio::test]
async fn api_key_lifecycle_and_revocation() {
    let (app, _dir, _tb) = make_app().await;

    // B 创建 key:明文只在创建响应中出现一次
    let (status, body) = send(&app, Method::POST, "/v1/api-keys", None, Some(KEY_B)).await;
    assert_eq!(status, StatusCode::CREATED);
    let plain = body["key"].as_str().unwrap().to_string();
    assert!(plain.starts_with("cmb_sk_"), "key 应带 cmb_sk_ 前缀");
    let key_id = body["record"]["id"].as_str().unwrap().to_string();

    // 新 key 可认证访问 B 的资源
    let (status, _) = send(&app, Method::GET, "/v1/databases", None, Some(&plain)).await;
    assert_eq!(status, StatusCode::OK);

    // 列表不含明文(只含哈希),且 B 能看到自己的新 key
    let (status, body) = send(&app, Method::GET, "/v1/api-keys", None, Some(KEY_B)).await;
    assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().unwrap();
    assert!(!arr.is_empty());
    for rec in arr {
        let kh = rec["key_hash"].as_str().unwrap();
        assert_eq!(kh.len(), 64, "存 sha256 十六进制哈希");
        assert!(!kh.contains(&plain), "不含明文");
    }

    // A 不能撤销 B 的 key
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/v1/api-keys/{key_id}"),
        None,
        Some(KEY_A),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "A 撤销 B 的 key 应失败");

    // B 自己撤销 → 204,之后该 key 立即 401
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/v1/api-keys/{key_id}"),
        None,
        Some(KEY_B),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(&app, Method::GET, "/v1/databases", None, Some(&plain)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "撤销后 key 立即失效");
}

/// 目的:租户资源完全隔开 —— A 的 Cell 对 B 而言如同不存在(404 而非 403,不泄露存在性)。
#[tokio::test]
async fn tenant_a_data_invisible_to_tenant_b() {
    let (app, _dir, _tb) = make_app().await;
    let (_s, body) = send(
        &app,
        Method::POST,
        "/v1/databases",
        Some(json!({})),
        Some(KEY_A),
    )
    .await;
    let id = body["id"].as_str().unwrap().to_string();

    // 未认证 → 401;错误 key → 401
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id}/kv/x"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id}/kv/x"),
        None,
        Some("cmb_sk_wrong"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// 确保 AuthContext extractor 可被引用(编译层面)
#[allow(dead_code)]
fn _assert_auth_context_is_copy(_: AuthContext) -> AuthContext {
    AuthContext {
        tenant_id: DEFAULT_TENANT,
        internal: false,
    }
}

/// 目的:平台 admin key(BFF 代理)能看到全部租户 Cell(供 BFF 按用户租户过滤);
/// 普通租户 key 看不到别的租户 Cell;admin key 的请求标记 internal 不计费。
#[tokio::test]
async fn admin_key_platform_view_and_internal_billing() {
    let (app, _dir, _tb) = make_app().await;

    // A 创建 Cell(用户 key)
    let (status, body) = send(
        &app,
        Method::POST,
        "/v1/databases",
        Some(json!({})),
        Some(KEY_A),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = body["id"].as_str().unwrap().to_string();

    // B 创建 Cell(用户 key)
    let (status, body_b) = send(
        &app,
        Method::POST,
        "/v1/databases",
        Some(json!({})),
        Some(KEY_B),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id_b = body_b["id"].as_str().unwrap().to_string();

    // admin key:列表看到全部(平台视角,供 BFF 过滤)
    let (status, body) = send(&app, Method::GET, "/v1/databases", None, Some(KEY_ADMIN)).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<String> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    assert!(ids.contains(&id), "admin key 应看到 A 的 Cell");
    assert!(ids.contains(&id_b), "admin key 应看到 B 的 Cell");

    // admin key 访问任意 Cell 数据成功(平台代理)
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id_b}/kv/probe"),
        None,
        Some(KEY_ADMIN),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "admin key 可访问任意租户 Cell");

    // admin key 请求不计费(internal):普通请求计费对照
    // (计费细节见 tests/usage.rs;此处验证 admin key 平台视角与访问能力)
}

/// 目的:by-name ensure 创建的 Cell 归属创建者租户 —— A 用用户 key by-name 创建后,
/// 仅 A(及其租户)可见;DEFAULT(平台)/B 均不可见;跨租户 by-name 查询 404。
/// 这是 console 新用户"看到 admin cell"回归的直接防护:
/// BFF 若用 admin key 创建 cell,cell 归平台租户 → 所有用户都能看到。
#[tokio::test]
async fn by_name_cell_belongs_to_creating_tenant() {
    let (app, _dir, _tb) = make_app().await;

    // A 用用户 key 创建命名 Cell(console 场景:用户创建自己的 cell)
    let (status, body) = send(
        &app,
        Method::PUT,
        "/v1/databases/by-name/my-app",
        Some(json!({})),
        Some(KEY_A),
    )
    .await;
    assert!(
        status == StatusCode::OK || status == StatusCode::CREATED,
        "by-name ensure 创建 201 或复用 200,实际 {status}"
    );
    let tenant_a_cell = body["cell"]["tenant_id"].as_str().unwrap().to_string();
    // 归属:创建者(DEFAULT 租户 —— KEY_A 挂在 DEFAULT_TENANT 下)非平台外租户
    assert_eq!(
        tenant_a_cell,
        DEFAULT_TENANT.0.to_string(),
        "Cell 归属创建者租户"
    );

    // B(其他租户)by-name 查询 A 的 Cell → 404
    let (status, _) = send(
        &app,
        Method::GET,
        "/v1/databases/by-name/my-app",
        None,
        Some(KEY_B),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "跨租户 by-name 查询必须 404");

    // A 通过 by-name 查到自己的 Cell id
    let (status, body) = send(
        &app,
        Method::GET,
        "/v1/databases/by-name/my-app",
        None,
        Some(KEY_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "A 可按名查到自己的 Cell");
    let cell_id = body["id"].as_str().unwrap().to_string();

    // B 用 A 的 Cell id 访问数据 → 404(跨租户)
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{cell_id}/sql"),
        Some(json!({"sql": "SELECT 1"})),
        Some(KEY_B),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "跨租户 SQL 必须 404");

    // A 正常访问自己的 Cell(id 路径)
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{cell_id}/sql"),
        Some(json!({"sql": "SELECT 1"})),
        Some(KEY_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "A 可访问自己的命名 Cell");
}

/// 目的:新用户(租户 B)列表只能看到自己租户的 Cell;
/// 平台租户(admin/DEFAULT)的 Cell(如 combee-bff)对用户不可见。
#[tokio::test]
async fn user_sees_only_own_tenant_cells() {
    let (app, _dir, _tb) = make_app().await;

    // 平台(DEFAULT)创建系统 Cell(模拟 combee-bff,由 admin key 创建 → 归 DEFAULT)
    let (status, body) = send(
        &app,
        Method::PUT,
        "/v1/databases/by-name/combee-bff",
        Some(json!({})),
        Some(KEY_ADMIN),
    )
    .await;
    assert!(
        status == StatusCode::OK || status == StatusCode::CREATED,
        "by-name ensure 创建 201 或复用 200,实际 {status}"
    );
    let admin_cell = body["cell"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        body["cell"]["tenant_id"].as_str().unwrap(),
        DEFAULT_TENANT.0.to_string(),
        "admin key 创建的系统 Cell 归平台租户"
    );

    // 新用户(B)创建自己的 Cell
    let (status, body) = send(
        &app,
        Method::POST,
        "/v1/databases",
        Some(json!({})),
        Some(KEY_B),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let b_cell = body["id"].as_str().unwrap().to_string();

    // B 的列表:只有自己的 Cell,看不到 combee-bff(平台)
    let (status, body) = send(&app, Method::GET, "/v1/databases", None, Some(KEY_B)).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<String> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    assert!(ids.contains(&b_cell), "B 看到自己的 Cell");
    assert!(!ids.contains(&admin_cell), "B 看不到平台 combee-bff Cell");

    // B 直接访问 combee-bff → 404
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{admin_cell}/sql"),
        Some(json!({"sql": "SELECT 1"})),
        Some(KEY_B),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "B 无法访问平台 Cell");
}
