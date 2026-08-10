//! Usage Metering 集成测试(设计文档 P0 §4)。
//!
//! 覆盖:请求计数按操作类型区分(SQL/KV read/write)、flush 后查询 API 返回、
//! summary 聚合与 storage bytes、timeseries 按 interval 合并、租户隔离。

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
use combee_api_server::usage::UsageMeter;
use combee_common::config::KvDurability;
use combee_common::usage::UsageMetric;
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
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn make_app() -> (Router, Arc<UsageMeter>, Arc<dyn MetadataStore>, TempDir) {
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
        quota: Default::default(),
    }));
    let client = Arc::new(LocalDataNodeClient::new(node));
    let provider: Arc<dyn DataNodeProvider> = Arc::new(LocalProvider::new(client));
    let meter = UsageMeter::new(metadata.clone(), Duration::from_secs(3600));
    let state = AppState {
        metadata: metadata.clone(),
        data_node: provider,
        nodes: Arc::new(NodeRegistry::new()),
        auth_mode: AuthMode::Off,
        control_plane_token: None,
        usage: meter.clone(),
        pricing: combee_api_server::pricing::PricingManager::new(
            metadata.clone(),
            std::time::Duration::from_secs(3600),
        ),
        admin_token: None,
        admin_api_key: None,
        quota: Default::default(),
        concurrency: Default::default(),
    };
    (build_app(state), meter, metadata, dir)
}

/// 目的:操作计数按类型区分,flush 后 usage API 返回正确数字;storage bytes > 0。
#[tokio::test]
async fn usage_tracks_ops_by_type_and_storage() {
    let (app, meter, _md, _dir) = make_app().await;

    let (_s, body) = send(&app, Method::POST, "/v1/databases", None).await;
    let id = body["id"].as_str().unwrap().to_string();

    // SQL 写(INSERT)+ SQL 读(SELECT)
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "CREATE TABLE t (x INTEGER)"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "INSERT INTO t VALUES (1)"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/v1/databases/{id}/sql"),
        Some(json!({"sql": "SELECT * FROM t"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // KV 写 + 读
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/v1/databases/{id}/kv/k1"),
        Some(json!({"value": "v1"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/v1/databases/{id}/kv/k1"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // flush 到 metadata
    let flushed = meter.flush_once().await.unwrap();
    assert!(flushed >= 6, "至少 6 个聚合键被 flush,实际 {flushed}");

    // 单 Cell usage
    let (status, body) = send(&app, Method::GET, &format!("/v1/cells/{id}/usage"), None).await;
    assert_eq!(status, StatusCode::OK, "cell usage: {body}");
    let ops = &body["operations"];
    assert!(
        ops["sql_writes"].as_u64().unwrap() >= 2,
        "CREATE+INSERT: {body}"
    );
    assert!(ops["sql_reads"].as_u64().unwrap() >= 1);
    assert!(ops["kv_writes"].as_u64().unwrap() >= 1);
    assert!(ops["kv_reads"].as_u64().unwrap() >= 1);
    // create 请求记在租户级(None cell);cell usage 只含 3 SQL + 2 KV = 5
    assert!(body["request_count"].as_u64().unwrap() >= 5);
    assert!(body["bytes_in"].as_u64().unwrap() > 0);
    assert!(body["bytes_out"].as_u64().unwrap() > 0);
    assert!(
        body["current_storage_bytes"].as_u64().unwrap() > 0,
        "落盘后有存储字节"
    );
}

/// 目的:租户级 summary 跨 Cell 聚合;timeseries 按 interval 合并。
#[tokio::test]
async fn usage_summary_and_timeseries() {
    let (app, meter, _md, _dir) = make_app().await;

    let (_s, b1) = send(&app, Method::POST, "/v1/databases", None).await;
    let id1 = b1["id"].as_str().unwrap().to_string();
    let (_s, b2) = send(&app, Method::POST, "/v1/databases", None).await;
    let id2 = b2["id"].as_str().unwrap().to_string();

    for id in [&id1, &id2] {
        send(
            &app,
            Method::PUT,
            &format!("/v1/databases/{id}/kv/x"),
            Some(json!({"value": "1"})),
        )
        .await;
    }
    meter.flush_once().await.unwrap();

    // summary:两个 Cell 的请求都计入
    let (status, body) = send(&app, Method::GET, "/v1/usage/summary", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["request_count"].as_u64().unwrap() >= 4,
        "2 次 create + 2 次 kv set: {body}"
    );
    assert!(body["operations"]["kv_writes"].as_u64().unwrap() >= 2);
    assert!(body["current_storage_bytes"].as_u64().unwrap() > 0);

    // timeseries:按分钟聚合 kv_write
    let (status, body) = send(
        &app,
        Method::GET,
        "/v1/usage/timeseries?metric=kv_write&interval=minute",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let arr = body.as_array().unwrap();
    assert!(!arr.is_empty());
    assert!(arr[0]["value"].as_u64().unwrap() >= 2, "{body}");

    // 非法 metric / interval → 400
    let (status, _) = send(
        &app,
        Method::GET,
        "/v1/usage/timeseries?metric=nope&interval=minute",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(
        &app,
        Method::GET,
        "/v1/usage/timeseries?metric=kv_read&interval=week",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// 目的:Usage 查询不产生 double-count —— 同一 key 的多次 flush 累加而不是覆盖。
#[tokio::test]
async fn usage_flush_accumulates_without_double_count_on_retry() {
    let (_app, meter, metadata, _dir) = make_app().await;
    let tenant = combee_metadata::DEFAULT_TENANT;
    let cell = combee_common::DatabaseId::new();
    let bucket = combee_common::usage::bucket_start(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
    );
    // 模拟两个窗口的计数
    meter.record(tenant, Some(cell), UsageMetric::KvRead, 5);
    meter.flush_once().await.unwrap();
    meter.record(tenant, Some(cell), UsageMetric::KvRead, 7);
    meter.flush_once().await.unwrap();

    let buckets = metadata
        .query_usage(
            tenant,
            Some(cell),
            Some(UsageMetric::KvRead),
            bucket,
            bucket,
        )
        .await
        .unwrap();
    assert_eq!(buckets.len(), 1);
    assert_eq!(buckets[0].value, 12, "两次 flush 累加,不重复不丢失");
}

/// 目的:并发记录计数准确(内存聚合线程安全)。
#[tokio::test]
async fn usage_concurrent_records_are_accurate() {
    let (_app, meter, metadata, _dir) = make_app().await;
    let tenant = combee_metadata::DEFAULT_TENANT;
    let cell = combee_common::DatabaseId::new();

    let mut handles = Vec::new();
    for _ in 0..8 {
        let meter = meter.clone();
        handles.push(tokio::spawn(async move {
            for _ in 0..250 {
                meter.record(tenant, Some(cell), UsageMetric::KvRead, 1);
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    meter.flush_once().await.unwrap();

    let bucket = combee_common::usage::bucket_start(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
    );
    let buckets = metadata
        .query_usage(
            tenant,
            Some(cell),
            Some(UsageMetric::KvRead),
            bucket,
            bucket,
        )
        .await
        .unwrap();
    assert_eq!(buckets[0].value, 8 * 250, "8 × 250 次计数全部记录");
}
