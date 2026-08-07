//! 单 replica 复制集成测试:主节点 WAL 增量归档 → 副本节点从对象存储拉取应用到本地。
//!
//! 主/副本各自是独立 DataNode,共享同一对象存储(本地 fs 后端模拟 MinIO/S3)。

use std::sync::Arc;
use std::time::Duration;

use combee_api_server::AppState;
use combee_api_server::app::build_app;
use combee_api_server::client::{DataNodeProvider, LocalDataNodeClient, LocalProvider};
use combee_api_server::nodes::NodeRegistry;
use combee_common::config::KvDurability;
use combee_common::protocol::SqlRequest;
use combee_common::{DatabaseId, NodeId};
use combee_data_node::{DataNode, DataNodeConfig};
use combee_metadata::{InMemoryStore, MetadataStore};
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
    })
    .with_object_store(store)
}

/// 主节点写 + 归档 → 副本拉取 → 副本数据与主节点归档点一致;多轮增量可追赶。
#[tokio::test]
async fn replica_syncs_from_primary_archive() {
    let primary_dir = tempfile::tempdir().unwrap();
    let replica_dir = tempfile::tempdir().unwrap();
    let os = tempfile::tempdir().unwrap();
    let db = DatabaseId::new();

    // 主节点:写入并归档
    let primary = node(primary_dir.path(), os.path());
    primary
        .kv_set(db, "k".into(), "v1".into(), None, false, false, 0)
        .await
        .unwrap();
    primary
        .execute_sql(
            db,
            SqlRequest {
                sql: "CREATE TABLE t (x INTEGER)".into(),
                params: vec![],
            },
            0,
        )
        .await
        .unwrap();
    primary
        .execute_sql(
            db,
            SqlRequest {
                sql: "INSERT INTO t VALUES (1),(2)".into(),
                params: vec![],
            },
            0,
        )
        .await
        .unwrap();
    primary.incremental_backup(db).await.unwrap();

    // 副本节点(独立 DataNode,共享对象存储):拉取并应用
    let replica = node(replica_dir.path(), os.path());
    assert!(
        replica.replicate_from_primary(db).await.unwrap(),
        "主节点已有归档,副本应能拉取"
    );
    let e = replica.kv_get(db, "k".into()).await.unwrap().unwrap();
    assert_eq!(e.value, "v1", "副本应同步主节点归档点");
    let r = replica
        .execute_sql(
            db,
            SqlRequest {
                sql: "SELECT COUNT(*) AS n FROM t".into(),
                params: vec![],
            },
            0,
        )
        .await
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![serde_json::json!(2)]],
        "副本 SQL 数据一致"
    );

    // 主节点再写 + 归档 → 副本追赶
    primary
        .kv_set(db, "k".into(), "v2".into(), None, false, false, 0)
        .await
        .unwrap();
    primary.incremental_backup(db).await.unwrap();
    assert!(replica.replicate_from_primary(db).await.unwrap());
    let e = replica.kv_get(db, "k".into()).await.unwrap().unwrap();
    assert_eq!(e.value, "v2", "副本应追上新一轮归档");

    // 主节点写但未归档 → 副本不变(复制延迟 = 归档间隔)
    primary
        .kv_set(db, "k".into(), "v3".into(), None, false, false, 0)
        .await
        .unwrap();
    replica.replicate_from_primary(db).await.unwrap();
    let e = replica.kv_get(db, "k".into()).await.unwrap().unwrap();
    assert_eq!(e.value, "v2", "未归档的 v3 不应复制到副本");
}

/// 副本在对象存储无主节点归档时,拉取返回 false(不产生错误)。
#[tokio::test]
async fn replica_no_archive_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let os = tempfile::tempdir().unwrap();
    let db = DatabaseId::new();
    let n = node(dir.path(), os.path());
    assert!(!n.replicate_from_primary(db).await.unwrap());
}

/// Replication API:POST /v1/databases/{id}/replication 设置副本节点,GET 可查。
#[tokio::test]
async fn replication_api_sets_replica() {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    let dir = tempfile::tempdir().unwrap();
    let metadata: Arc<dyn MetadataStore> = Arc::new(InMemoryStore::new());
    let registry = Arc::new(NodeRegistry::new());
    let replica_node = registry.register("http://replica:9000".into(), 10);
    let local = Arc::new(LocalDataNodeClient::new(Arc::new(DataNode::new(
        DataNodeConfig {
            data_dir: dir.path().to_path_buf(),
            max_active_dbs: 4,
            db_idle_timeout: Duration::from_secs(3600),
            ttl_gc_interval: Duration::from_secs(3600),
            kv_cache_capacity: 1_000,
            kv_durability: KvDurability::Normal,
            sql_timeout: Some(std::time::Duration::from_secs(30)),
        },
    ))));
    let provider: Arc<dyn DataNodeProvider> = Arc::new(LocalProvider::new(local));
    let app = build_app(AppState {
        metadata: metadata.clone(),
        data_node: provider,
        nodes: registry,
        auth_mode: combee_api_server::auth::AuthMode::Off,
        control_plane_token: None,
    });

    // 创建 db
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/databases")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    let db = v["id"].as_str().unwrap().to_string();

    // 设置副本
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/databases/{db}/replication"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"replica_node": replica_node}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "设置副本应成功");

    // GET 查询
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/v1/databases/{db}/replication"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["replica_node"], json!(replica_node));

    // metadata 层面验证
    let rec = metadata
        .get_database(
            combee_metadata::DEFAULT_TENANT,
            db.parse::<DatabaseId>().unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rec.replica_node_id, Some(replica_node));

    // 取消副本
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/v1/databases/{db}/replication"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let rec = metadata
        .get_database(
            combee_metadata::DEFAULT_TENANT,
            db.parse::<DatabaseId>().unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rec.replica_node_id, None);
}

// 保持 NodeId import 使用。
#[allow(dead_code)]
fn _node(_: NodeId) {}
