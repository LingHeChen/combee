//! 自动 / 手动 failover + generation fencing 集成测试。
//!
//! 1. generation fencing:DataNode.fence_cell 后,旧 generation 的写被拒(防脑裂);
//! 2. failover 全链路:主节点失效 → 副本提升为主(generation+1)→ 写走新主 →
//!    旧主(旧 generation)写被拒;
//! 3. metadata promote_replica 语义。

use std::sync::Arc;
use std::time::Duration;

use combee_api_server::AppState;
use combee_api_server::client::{DataNodeProvider, RemoteDataNodeClient, RoutingProvider};
use combee_api_server::nodes::NodeRegistry;
use combee_common::config::KvDurability;
use combee_common::protocol::KvSetRequest;
use combee_common::{CombeeError, DatabaseId, NodeId};
use combee_data_node::server;
use combee_data_node::{DataNode, DataNodeConfig};
use combee_metadata::{DEFAULT_TENANT, InMemoryStore, MetadataStore};
use object_store::ObjectStore;
use object_store::local::LocalFileSystem;

fn node(data_dir: &std::path::Path, os_dir: &std::path::Path) -> DataNode {
    let store: Arc<dyn ObjectStore> = Arc::new(LocalFileSystem::new_with_prefix(os_dir).unwrap());
    DataNode::new(DataNodeConfig {
        data_dir: data_dir.to_path_buf(),
        max_active_dbs: 8,
        db_idle_timeout: Duration::from_secs(3600),
        ttl_gc_interval: Duration::from_secs(3600),
        kv_cache_capacity: 10_000,
        kv_durability: KvDurability::Normal,
        sql_timeout: Some(std::time::Duration::from_secs(30)),
        quota: Default::default(),
    })
    .with_object_store(store)
}

async fn spawn_data_node(os_dir: &std::path::Path) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::mem::forget(dir); // 测试期间保持数据目录
    let node = Arc::new(node(
        std::path::Path::new("/tmp/combee-failover-test"),
        os_dir,
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, server::router(node, None))
            .await
            .unwrap();
    });
    format!("http://{addr}")
}

fn set_req(value: &str) -> KvSetRequest {
    KvSetRequest {
        value: value.into(),
        ttl_seconds: None,
        nx: false,
        xx: false,
    }
}

/// 1) generation fencing:旧 generation 的写被拒。
#[tokio::test]
async fn generation_fencing_rejects_stale_writes() {
    let dir = tempfile::tempdir().unwrap();
    let os = tempfile::tempdir().unwrap();
    let n = node(dir.path(), os.path());
    let db = DatabaseId::new();

    // generation 0(默认)写成功
    n.kv_set(db, "k".into(), "v0".into(), None, false, false, 0)
        .await
        .unwrap();

    // fence 到 5
    n.fence_cell(db, 5);

    // 旧 generation 写被拒(防脑裂)
    let err = n
        .kv_set(db, "k".into(), "stale".into(), None, false, false, 4)
        .await
        .unwrap_err();
    assert!(
        matches!(err, CombeeError::Forbidden(_)),
        "旧 generation 写应被拒绝,got {err:?}"
    );

    // 当前 generation 写成功
    n.kv_set(db, "k".into(), "v5".into(), None, false, false, 5)
        .await
        .unwrap();
    let e = n.kv_get(db, "k".into()).await.unwrap().unwrap();
    assert_eq!(e.value, "v5");
}

/// 2) failover 全链路:副本提升 + generation + 旧主写被拒。
#[tokio::test]
async fn failover_promotes_replica_and_fences_old_primary() {
    let os = tempfile::tempdir().unwrap();
    let url_a = spawn_data_node(os.path()).await; // 主
    let url_b = spawn_data_node(os.path()).await; // 副本

    let registry = Arc::new(NodeRegistry::new());
    let primary = registry.register(url_a, 10);
    let replica = registry.register(url_b, 10);
    let metadata: Arc<dyn MetadataStore> = Arc::new(InMemoryStore::new());
    let provider: Arc<dyn DataNodeProvider> = Arc::new(RoutingProvider::new(
        registry.clone(),
        metadata.clone(),
        None,
    ));

    let db = DatabaseId::new();
    metadata
        .create_database(DEFAULT_TENANT, db, Some(primary), None)
        .await
        .unwrap();
    metadata
        .set_replica_node(DEFAULT_TENANT, db, Some(replica))
        .await
        .unwrap();

    // 主写数据
    let pc = provider.client_for(db).await.unwrap();
    pc.kv_set(db, "k".into(), set_req("v1"), 0).await.unwrap();

    // 手动 failover(主节点可视为已失效)
    let usage_meter = combee_api_server::usage::UsageMeter::new(
        metadata.clone(),
        std::time::Duration::from_secs(3600),
    );
    let state = AppState {
        metadata: metadata.clone(),
        data_node: provider.clone(),
        nodes: registry.clone(),
        auth_mode: combee_api_server::auth::AuthMode::Off,
        control_plane_token: None,
        usage: usage_meter,
        pricing: combee_api_server::pricing::PricingManager::new(
            metadata.clone(),
            std::time::Duration::from_secs(3600),
        ),
        admin_token: None,
        admin_api_key: None,
        quota: Default::default(),
        concurrency: Default::default(),
    };
    let promoted = combee_api_server::failover::failover_cell(&state, DEFAULT_TENANT, db)
        .await
        .unwrap();
    assert_eq!(promoted.storage_node_id, Some(replica), "副本被提升为主");
    assert_eq!(promoted.replica_node_id, None, "副本位清空");
    assert_eq!(promoted.generation, 1, "failover 递增 generation");

    // 写走新主(副本):generation 1
    let nc = provider.client_for(db).await.unwrap();
    nc.kv_set(db, "k".into(), set_req("v2"), 1).await.unwrap();
    let e = nc.kv_get(db, "k".into()).await.unwrap().unwrap();
    assert_eq!(e.value, "v2", "写路由到新主成功");

    // 旧主(本地 generation 仍为 0)收到带 generation 1 的写 → 拒绝
    let oc = provider.client_for_node(primary).await.unwrap();
    let err = oc
        .kv_set(db, "k".into(), set_req("stale"), 1)
        .await
        .unwrap_err();
    assert!(
        matches!(err, CombeeError::Forbidden(_)),
        "旧主应因 generation 不匹配拒绝写,got {err:?}"
    );
}

/// 3) metadata promote_replica:无副本时报错、generation 递增。
#[tokio::test]
async fn metadata_promote_replica_semantics() {
    let metadata: Arc<dyn MetadataStore> = Arc::new(InMemoryStore::new());
    let primary = NodeId::new();
    let replica = NodeId::new();
    let db = DatabaseId::new();
    metadata
        .create_database(DEFAULT_TENANT, db, Some(primary), None)
        .await
        .unwrap();
    metadata
        .set_replica_node(DEFAULT_TENANT, db, Some(replica))
        .await
        .unwrap();

    let promoted = metadata.promote_replica(DEFAULT_TENANT, db).await.unwrap();
    assert_eq!(promoted.storage_node_id, Some(replica));
    assert_eq!(promoted.replica_node_id, None);
    assert_eq!(promoted.generation, 1);

    // 再次 promote(已无副本)→ 报错
    let err = metadata
        .promote_replica(DEFAULT_TENANT, db)
        .await
        .unwrap_err();
    assert!(matches!(err, CombeeError::Internal(_)));
}

// 保持 RemoteDataNodeClient import 使用(RoutingProvider 内部用)。
#[allow(dead_code)]
fn _remote(_: RemoteDataNodeClient) {}
