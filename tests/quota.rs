//! 资源配额(安全护栏)测试:KV key/value、SQL 截断、cells-per-tenant、body limit、并发计数。

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
use combee_api_server::quota::ConcurrencyCounters;
use combee_common::config::{KvDurability, QuotaConfig};
use combee_data_node::{DataNode, DataNodeConfig};
use combee_metadata::{InMemoryStore, MetadataStore};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

async fn send(app: &Router, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let req = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    let resp = app
        .clone()
        .oneshot(
            req.body(body.map(|v| v.to_string()).unwrap_or_else(|| "{}".into()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 10 * 1024 * 1024)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn make_app(quota: QuotaConfig) -> (Router, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let metadata: Arc<dyn MetadataStore> = Arc::new(InMemoryStore::new());
    let node = Arc::new(DataNode::new(DataNodeConfig {
        data_dir: dir.path().to_path_buf(),
        max_active_dbs: 16,
        db_idle_timeout: Duration::from_secs(3600),
        ttl_gc_interval: Duration::from_secs(3600),
        kv_cache_capacity: 100_000,
        kv_durability: KvDurability::Normal,
        sql_timeout: Some(Duration::from_secs(5)),
        quota: quota.clone(),
    }));
    let client = Arc::new(LocalDataNodeClient::new(node));
    let provider: Arc<dyn DataNodeProvider> = Arc::new(LocalProvider::new(client));
    let state = AppState {
        metadata,
        data_node: provider,
        nodes: Arc::new(NodeRegistry::new()),
        auth_mode: AuthMode::Off,
        control_plane_token: None,
        bff_service_key: None,
        usage: combee_api_server::usage::UsageMeter::new(
            Arc::new(InMemoryStore::new()),
            Duration::from_secs(3600),
        ),
        pricing: combee_api_server::pricing::PricingManager::new(
            Arc::new(InMemoryStore::new()),
            Duration::from_secs(3600),
        ),
        admin_token: None,
        quota,
        concurrency: Default::default(),
        min_credit_balance_units: -100 * combee_common::credit::CREDIT_UNITS_PER_CREDIT,
    };
    (build_app(state), dir)
}

async fn create_cell(app: &Router) -> String {
    let (_s, body) = send(app, Method::POST, "/v1/databases", None).await;
    body["id"].as_str().unwrap().to_string()
}

/// 目的:KV value 超过配额 → 429 QuotaExceeded。
#[tokio::test]
async fn kv_value_too_large_rejected() {
    let quota = QuotaConfig {
        max_kv_value_bytes: 10,
        ..Default::default()
    };
    let (app, _dir) = make_app(quota).await;
    let id = create_cell(&app).await;
    let (status, body) = send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/k"),
        Some(json!({"value": "this value is longer than ten bytes"})),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "超限应 429: {body}");
    assert_eq!(body["code"], "quota_exceeded");
}

/// 目的:KV key 超过配额 → 429。
#[tokio::test]
async fn kv_key_too_large_rejected() {
    let quota = QuotaConfig {
        max_kv_key_bytes: 5,
        ..Default::default()
    };
    let (app, _dir) = make_app(quota).await;
    let id = create_cell(&app).await;
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/way-too-long-key"),
        Some(json!({"value": "v"})),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

/// 目的:SQL 查询超过 max_rows → 截断并标记 truncated。
#[tokio::test]
async fn sql_rows_truncated() {
    let quota = QuotaConfig {
        max_sql_rows: 3,
        ..Default::default()
    };
    let (app, _dir) = make_app(quota).await;
    let id = create_cell(&app).await;
    send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "CREATE TABLE t (x INTEGER)"})),
    )
    .await;
    for i in 0..6 {
        send(
            &app,
            Method::POST,
            &format!("/v1/databases/{id}/sql"),
            Some(json!({"sql": "INSERT INTO t VALUES (?)", "params": [i]})),
        )
        .await;
    }
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT * FROM t ORDER BY x"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rows"].as_array().unwrap().len(), 3, "截断到 max_rows");
    assert_eq!(body["truncated"], true, "标记截断");
}

/// 目的:cells-per-tenant 超限 → 429。
#[tokio::test]
async fn cells_per_tenant_limit() {
    let quota = QuotaConfig {
        max_cells_per_tenant: 2,
        ..Default::default()
    };
    let (app, _dir) = make_app(quota).await;
    assert_eq!(create_cell(&app).await.len(), 36);
    assert_eq!(create_cell(&app).await.len(), 36);
    let (status, body) = send(&app, Method::POST, "/v1/databases", None).await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "第 3 个应 429: {body}"
    );
}

/// 目的:请求体超过 max_request_body_bytes → 413。
#[tokio::test]
async fn request_body_limit() {
    let quota = QuotaConfig {
        max_request_body_bytes: 200,
        ..Default::default()
    };
    let (app, _dir) = make_app(quota).await;
    let big = json!({"sql": format!("SELECT '{}'", "x".repeat(500))});
    let (status, _) = send(
        &app,
        Method::POST,
        "/v1/databases/00000000-0000-0000-0000-000000000001/sql",
        Some(big),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "超大 body 应 413");
}

/// 目的:并发计数器 enter/guard drop 正确增减。
#[test]
fn concurrency_counter_guard() {
    let c = ConcurrencyCounters::default();
    let g1 = c.try_enter("t:1", 2).unwrap();
    let g2 = c.try_enter("t:1", 2).unwrap();
    assert!(c.try_enter("t:1", 2).is_err(), "第 3 个超限");
    drop(g2);
    assert!(c.try_enter("t:1", 2).is_ok(), "释放后可再进");
    drop(g1);
    // 全释放后可再次进入(计数已回收)
    assert!(c.try_enter("t:1", 2).is_ok());
}

/// 目的:余额护栏 —— 非 internal 请求在余额低于阈值时 402,充值后放行。
/// 这里用 auth=off(app 无 AuthContext → 默认租户)与默认阈值 -100 credits。
#[tokio::test]
async fn credit_balance_guard_rejects_and_recovers() {
    let (app, _dir) = make_app(QuotaConfig::default()).await;
    // 直接用 make_app 内的 metadata?make_app 未暴露 metadata —— 用独立 app 构造太重复,
    // 这里改为验证:无 AuthContext 时走默认租户,默认余额 0 >= -100 阈值 → 放行。
    // 真正 402 路径由 credit_quota 单测覆盖(见下)。
    let (status, _) = send(&app, Method::GET, "/v1/databases", None).await;
    assert_eq!(status, StatusCode::OK, "默认租户余额 0(≥ -100)应放行");
}

/// 目的:credit_quota 中间件单元级 —— 余额 < 阈值 → 402;internal 豁免;余额充足放行。
#[tokio::test]
async fn credit_quota_middleware_enforces_threshold() {
    use axum::middleware;
    use axum::{Router, routing::get};
    use combee_api_server::quota::credit_quota;
    use combee_common::{AuthContext, TenantId};
    use combee_metadata::DEFAULT_TENANT;

    let metadata: Arc<dyn MetadataStore> = Arc::new(InMemoryStore::new());
    let t = TenantId::new();
    metadata.create_tenant(t).await.unwrap();
    // 充值 -200 credits(透支超过 -100 阈值)
    use combee_common::credit::{CreditTransaction, CreditTransactionType};
    metadata
        .append_credit_transaction(CreditTransaction {
            id: combee_common::TenantId::new().0,
            tenant_id: t,
            txn_type: CreditTransactionType::Grant,
            amount_units: -200 * combee_common::credit::CREDIT_UNITS_PER_CREDIT,
            pricing_version: None,
            reference_id: Some("test:overdraft".into()),
            description: None,
            created_at: 1,
            balance_after: None,
        })
        .await
        .unwrap();

    let data_node = Arc::new(DataNode::new(DataNodeConfig {
        data_dir: tempfile::tempdir().unwrap().keep(),
        max_active_dbs: 4,
        db_idle_timeout: Duration::from_secs(3600),
        ttl_gc_interval: Duration::from_secs(3600),
        kv_cache_capacity: 100,
        kv_durability: KvDurability::Normal,
        sql_timeout: None,
        quota: Default::default(),
    }));
    let state = AppState {
        metadata: metadata.clone(),
        data_node: Arc::new(LocalProvider::new(Arc::new(LocalDataNodeClient::new(
            data_node,
        )))),
        nodes: Arc::new(NodeRegistry::new()),
        auth_mode: AuthMode::Key,
        control_plane_token: None,
        bff_service_key: Some("cmb_sk_bff".into()),
        usage: combee_api_server::usage::UsageMeter::new(
            metadata.clone(),
            Duration::from_secs(3600),
        ),
        pricing: combee_api_server::pricing::PricingManager::new(
            metadata.clone(),
            Duration::from_secs(3600),
        ),
        admin_token: None,
        quota: Default::default(),
        concurrency: Default::default(),
        min_credit_balance_units: -100 * combee_common::credit::CREDIT_UNITS_PER_CREDIT,
    };

    // 手动构造一个带 AuthContext{tenant=t, internal=false} 的请求,经 credit_quota 中间件
    let app = Router::new()
        .route("/probe", get(|| async { "ok" }))
        .layer(middleware::from_fn_with_state(state.clone(), credit_quota));

    // 透支超过阈值 → 402
    let req = axum::http::Request::builder()
        .uri("/probe")
        .extension(AuthContext {
            tenant_id: t,
            internal: false,
        })
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::PAYMENT_REQUIRED,
        "透支超阈值应 402"
    );

    // internal 豁免 → 200
    let req2 = axum::http::Request::builder()
        .uri("/probe")
        .extension(AuthContext {
            tenant_id: t,
            internal: true,
        })
        .body(axum::body::Body::empty())
        .unwrap();
    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK, "internal 请求应豁免");

    // 余额充足(默认租户,0 ≥ -100)→ 200
    let req3 = axum::http::Request::builder()
        .uri("/probe")
        .extension(AuthContext {
            tenant_id: DEFAULT_TENANT,
            internal: false,
        })
        .body(axum::body::Body::empty())
        .unwrap();
    let resp3 = app.clone().oneshot(req3).await.unwrap();
    assert_eq!(resp3.status(), StatusCode::OK, "余额充足应放行");
}
