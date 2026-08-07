//! 集成测试共享工具(测试专用,非库代码)。
#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use combee_api_server::AppState;
use combee_api_server::app::build_app;
use combee_api_server::client::{DataNodeProvider, LocalDataNodeClient, LocalProvider};
use combee_api_server::nodes::NodeRegistry;
use combee_common::config::KvDurability;
use combee_data_node::{DataNode, DataNodeConfig};
use combee_metadata::{DEFAULT_TENANT, InMemoryStore, MetadataStore};
use serde_json::Value;
use tower::ServiceExt;

/// 构建测试 app。`max_active` 控制 Data Node 的并发连接上限。
pub fn test_app(max_active: usize) -> (Router, Arc<LocalDataNodeClient>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let metadata: Arc<dyn MetadataStore> = Arc::new(InMemoryStore::new());
    let node = Arc::new(DataNode::new(DataNodeConfig {
        data_dir: dir.path().to_path_buf(),
        max_active_dbs: max_active,
        db_idle_timeout: Duration::from_secs(3600),
        ttl_gc_interval: Duration::from_secs(3600),
        kv_cache_capacity: 100_000,
        kv_durability: KvDurability::Normal,
        sql_timeout: Some(std::time::Duration::from_secs(5)),
    }));
    let client = Arc::new(LocalDataNodeClient::new(node));
    let provider: Arc<dyn DataNodeProvider> = Arc::new(LocalProvider::new(client.clone()));
    let state = AppState {
        metadata,
        data_node: provider,
        nodes: Arc::new(NodeRegistry::new()),
        auth_mode: combee_api_server::auth::AuthMode::Off,
        control_plane_token: None,
    };
    (build_app(state), client, dir)
}

/// 带认证的测试 app。
pub async fn test_app_with_keys(keys: &[&str]) -> (Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let metadata: Arc<dyn MetadataStore> = Arc::new(InMemoryStore::new());
    // Key 模式下需先在元数据中预置 key 哈希(明文仅存在于调用方)
    for k in keys {
        metadata
            .create_api_key(DEFAULT_TENANT, combee_common::api_key::hash(k))
            .await
            .unwrap();
    }
    let node = Arc::new(DataNode::new(DataNodeConfig {
        data_dir: dir.path().to_path_buf(),
        max_active_dbs: 16,
        db_idle_timeout: Duration::from_secs(3600),
        ttl_gc_interval: Duration::from_secs(3600),
        kv_cache_capacity: 100_000,
        kv_durability: KvDurability::Normal,
        sql_timeout: Some(std::time::Duration::from_secs(5)),
    }));
    let client = Arc::new(LocalDataNodeClient::new(node));
    let provider: Arc<dyn DataNodeProvider> = Arc::new(LocalProvider::new(client));
    let state = AppState {
        metadata,
        data_node: provider,
        nodes: Arc::new(NodeRegistry::new()),
        auth_mode: combee_api_server::auth::AuthMode::Key,
        control_plane_token: None,
    };
    (build_app(state), dir)
}

/// 发送一个 HTTP 请求并解析响应。非 JSON body 宽容为 `Value::Null`
/// (axum 的路径参数解析失败等错误路径返回纯文本 body)。
pub async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    api_key: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(k) = api_key {
        builder = builder.header("x-api-key", k);
    }
    let req = match body {
        Some(b) => builder
            .header("content-type", "application/json")
            .body(Body::from(b.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

/// 创建一个数据库并返回其 id。
pub async fn create_db(app: &Router) -> String {
    let (status, body) = send(app, Method::POST, "/v1/databases", None, None).await;
    assert_eq!(status, StatusCode::CREATED, "create failed: {body}");
    body["id"].as_str().unwrap().to_string()
}
