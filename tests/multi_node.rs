//! 多 Data Node 集成测试:registration / placement / 路由与数据隔离。
//!
//! 起两个真实的 Data Node HTTP 服务 + NodeRegistry + RoutingProvider,验证:
//! 1. create database 按 round-robin 放置到不同节点(metadata.storage_node_id);
//! 2. RoutingProvider 按 Cell 路由到对应节点的 RPC 客户端,跨节点数据隔离;
//! 3. Data Node agent 注册/心跳/注销全链路。

use std::sync::Arc;
use std::time::Duration;

use combee_api_server::AppState;
use combee_api_server::app::build_app;
use combee_api_server::client::{
    DataNodeClient, DataNodeProvider, RemoteDataNodeClient, RoutingProvider,
};
use combee_api_server::nodes::NodeRegistry;
use combee_common::config::KvDurability;
use combee_common::protocol::KvSetRequest;
use combee_common::{DatabaseId, NodeId};
use combee_data_node::agent::NodeAgent;
use combee_data_node::server;
use combee_data_node::{DataNode, DataNodeConfig};
use combee_metadata::{DEFAULT_TENANT, InMemoryStore, MetadataStore};
use serde_json::Value;
use tower::ServiceExt;

/// 起一个真实 Data Node HTTP 服务,返回 base URL。
async fn spawn_data_node() -> (String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let node = Arc::new(DataNode::new(DataNodeConfig {
        data_dir: dir.path().to_path_buf(),
        max_active_dbs: 16,
        db_idle_timeout: Duration::from_secs(3600),
        ttl_gc_interval: Duration::from_secs(3600),
        kv_cache_capacity: 100_000,
        kv_durability: KvDurability::Normal,
        sql_timeout: Some(std::time::Duration::from_secs(30)),
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, server::router(node, None))
            .await
            .unwrap();
    });
    (format!("http://{addr}"), dir)
}

fn set_req(value: &str) -> KvSetRequest {
    KvSetRequest {
        value: value.into(),
        ttl_seconds: None,
        nx: false,
        xx: false,
    }
}

/// 1) create database 按 round-robin 放置到不同节点。
#[tokio::test]
async fn create_database_round_robin_placement() {
    let (url_a, _) = spawn_data_node().await;
    let (url_b, _) = spawn_data_node().await;

    let registry = Arc::new(NodeRegistry::new());
    let node_a = registry.register(url_a, 10);
    let node_b = registry.register(url_b, 10);

    let metadata: Arc<dyn MetadataStore> = Arc::new(InMemoryStore::new());
    let provider: Arc<dyn DataNodeProvider> = Arc::new(RoutingProvider::new(
        registry.clone(),
        metadata.clone(),
        None,
    ));
    let app = build_app(AppState {
        metadata: metadata.clone(),
        data_node: provider,
        nodes: registry.clone(),
        auth_mode: combee_api_server::auth::AuthMode::Off,
        control_plane_token: None,
        usage: combee_api_server::usage::UsageMeter::new(
            metadata.clone(),
            std::time::Duration::from_secs(3600),
        ),
        pricing: combee_api_server::pricing::PricingManager::new(
            metadata.clone(),
            std::time::Duration::from_secs(3600),
        ),
        admin_token: None,
    });

    // 创建两个 db → 应分别落到节点 A / B(round-robin)
    let id1 = create_db(&app).await;
    let id2 = create_db(&app).await;
    let r1 = metadata
        .get_database(DEFAULT_TENANT, id1.parse().unwrap())
        .await
        .unwrap();
    let r2 = metadata
        .get_database(DEFAULT_TENANT, id2.parse().unwrap())
        .await
        .unwrap();
    // round-robin 起始节点取决于注册表迭代顺序(随机),断言落在不同节点即可
    assert!(r1.storage_node_id.is_some() && r2.storage_node_id.is_some());
    assert_ne!(
        r1.storage_node_id, r2.storage_node_id,
        "round-robin 应分布到两个节点"
    );
    let placed = [r1.storage_node_id.unwrap(), r2.storage_node_id.unwrap()];
    assert!(
        placed.contains(&node_a) && placed.contains(&node_b),
        "两个节点都应被使用"
    );
}

/// 2) 路由与跨节点数据隔离:db_a 的数据在节点 A,db_b 在节点 B,互不可见。
#[tokio::test]
async fn routing_isolates_data_per_node() {
    let (url_a, _) = spawn_data_node().await;
    let (url_b, _) = spawn_data_node().await;

    let registry = Arc::new(NodeRegistry::new());
    let node_a = registry.register(url_a, 10);
    let node_b = registry.register(url_b, 10);
    let metadata: Arc<dyn MetadataStore> = Arc::new(InMemoryStore::new());
    let provider = Arc::new(RoutingProvider::new(
        registry.clone(),
        metadata.clone(),
        None,
    ));

    let db_a = DatabaseId::new();
    let db_b = DatabaseId::new();
    metadata
        .create_database(DEFAULT_TENANT, db_a, Some(node_a))
        .await
        .unwrap();
    metadata
        .create_database(DEFAULT_TENANT, db_b, Some(node_b))
        .await
        .unwrap();

    let ca = provider.client_for(db_a).await.unwrap();
    ca.kv_set(db_a, "k".into(), set_req("on-a"), 0)
        .await
        .unwrap();
    let cb = provider.client_for(db_b).await.unwrap();
    cb.kv_set(db_b, "k".into(), set_req("on-b"), 0)
        .await
        .unwrap();

    // 数据隔离:对方节点上不存在该 Cell 的数据
    assert!(
        ca.kv_get(db_b, "k".into()).await.unwrap().is_none(),
        "A 无 db_b 数据"
    );
    assert!(
        cb.kv_get(db_a, "k".into()).await.unwrap().is_none(),
        "B 无 db_a 数据"
    );
    // 各自数据正确
    assert_eq!(
        ca.kv_get(db_a, "k".into()).await.unwrap().unwrap().value,
        "on-a"
    );
    assert_eq!(
        cb.kv_get(db_b, "k".into()).await.unwrap().unwrap().value,
        "on-b"
    );

    // SQL 同样路由
    ca.execute_sql(
        db_a,
        combee_common::protocol::SqlRequest {
            sql: "CREATE TABLE t (x INTEGER)".into(),
            params: vec![],
        },
        0,
    )
    .await
    .unwrap();
    let r = ca
        .execute_sql(
            db_a,
            combee_common::protocol::SqlRequest {
                sql: "SELECT name FROM sqlite_master WHERE type='table' AND name='t'".into(),
                params: vec![],
            },
            0,
        )
        .await
        .unwrap();
    assert_eq!(r.rows.len(), 1, "表建在节点 A");
}

/// 3) Data Node agent 注册/心跳/注销全链路。
#[tokio::test]
async fn agent_registers_heartbeats_and_unregisters() {
    let (dummy_url, _) = spawn_data_node().await;
    let registry = Arc::new(NodeRegistry::new());
    let metadata: Arc<dyn MetadataStore> = Arc::new(InMemoryStore::new());
    let remote: Arc<dyn DataNodeClient> = Arc::new(RemoteDataNodeClient::new(dummy_url));
    let provider: Arc<dyn DataNodeProvider> = Arc::new(RoutingProvider::new(
        registry.clone(),
        metadata.clone(),
        Some(remote),
    ));
    let usage_meter = combee_api_server::usage::UsageMeter::new(
        metadata.clone(),
        std::time::Duration::from_secs(3600),
    );
    let pricing_meter = combee_api_server::pricing::PricingManager::new(
        metadata.clone(),
        std::time::Duration::from_secs(3600),
    );
    let app = build_app(AppState {
        metadata,
        data_node: provider,
        nodes: registry.clone(),
        auth_mode: combee_api_server::auth::AuthMode::Off,
        control_plane_token: None,
        usage: usage_meter,
        pricing: pricing_meter,
        admin_token: None,
    });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // agent 注册(自愈循环:注册是异步的,轮询等待)
    let (agent, _hb) =
        NodeAgent::start(&format!("http://{addr}"), "http://node-a:9000", 10, None).await;
    let mut node_id = None;
    for _ in 0..30 {
        if let Some(id) = agent.id() {
            node_id = Some(id);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let node_id = node_id.expect("agent registered within timeout");
    assert_eq!(registry.list().len(), 1, "agent 注册后 registry 有一个节点");
    assert!(registry.is_healthy(node_id));
    assert_eq!(
        registry.addr(node_id).as_deref(),
        Some("http://node-a:9000")
    );

    // 心跳任务在跑(300ms 后仍健康)
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(registry.is_healthy(node_id));

    // 注销
    agent.unregister().await;
    assert_eq!(registry.list().len(), 0, "注销后 registry 为空");
}

// ---- helpers ----

async fn create_db(app: &axum::Router) -> String {
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/databases")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::CREATED);
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    v["id"].as_str().unwrap().to_string()
}

// 保持 NodeId import 使用(类型标注)。
#[allow(dead_code)]
fn _node_type(_: NodeId) {}
