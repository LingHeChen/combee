//! Cell Manifest / 只读保护(roadmap 4.1):
//! 打开 Cell 时跑 PRAGMA quick_check;完整性失败 → 进入只读保护模式,
//! 写操作被拒绝(不静默修复),读/恢复仍可进行。

use std::io::{Seek, SeekFrom, Write};
use std::sync::Arc;

use combee_common::config::KvDurability;
use combee_data_node::{DataNode, DataNodeConfig};
use combee_common::DatabaseId;
use object_store::local::LocalFileSystem;
use tempfile::TempDir;

fn node(data_dir: &std::path::Path, os_dir: &std::path::Path) -> DataNode {
    let store: Arc<dyn object_store::ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(os_dir).unwrap());
    DataNode::new(DataNodeConfig {
        data_dir: data_dir.to_path_buf(),
        max_active_dbs: 8,
        db_idle_timeout: std::time::Duration::from_secs(3600),
        ttl_gc_interval: std::time::Duration::from_secs(3600),
        kv_cache_capacity: 10_000,
        kv_durability: KvDurability::Normal,
        quota: combee_common::config::QuotaConfig::default(),
        sql_timeout: Some(std::time::Duration::from_secs(5)),
    })
    .with_object_store(store)
}

#[tokio::test]
async fn manifest_created_on_open() {
    let dir = TempDir::new().unwrap();
    let os = TempDir::new().unwrap();
    let n = node(dir.path(), os.path());
    let db = DatabaseId::new();

    n.kv_set(db, "k".into(), "v".into(), None, false, false, 0)
        .await
        .unwrap();
    n.shutdown().await;

    let manifest = combee_data_node::storage::read_manifest(dir.path(), db)
        .expect("manifest written after first open");
    assert_eq!(manifest["format_version"], 1);
    assert_eq!(manifest["cell_id"].as_str(), Some(db.to_string().as_str()));
}

#[tokio::test]
async fn corrupted_cell_enters_readonly_and_blocks_writes() {
    let dir = TempDir::new().unwrap();
    let os = TempDir::new().unwrap();
    let n = node(dir.path(), os.path());
    let db = DatabaseId::new();

    n.kv_set(db, "k".into(), "v".into(), None, false, false, 0)
        .await
        .unwrap();
    n.shutdown().await;

    // 破坏主库文件中部(覆盖页面内容 → quick_check 应失败)
    let path = combee_data_node::storage::db_path(dir.path(), db);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap();
    let len = f.metadata().unwrap().len();
    assert!(len > 4096, "cell file has pages");
    f.seek(SeekFrom::Start(len / 2)).unwrap();
    f.write_all(b"XXXXXXXXCORRUPTEDXXXXXXXX").unwrap();
    f.sync_all().unwrap();
    drop(f);

    // 再次打开 → quick_check 失败 → 进入只读保护;写操作被明确拒绝
    let err = n
        .kv_set(db, "k2".into(), "v2".into(), None, false, false, 0)
        .await
        .expect_err("write to corrupted cell must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("read-only") || msg.contains("integrity"),
        "unexpected error: {msg}"
    );
}
