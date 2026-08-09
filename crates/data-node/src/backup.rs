//! 备份 / 恢复:SQLite 快照 → 对象存储(S3 / MinIO),以及从对象存储恢复。
//!
//! - 快照用 SQLite 的 `VACUUM INTO`(事务内一致性快照,WAL 安全);
//! - 对象存储走 `object_store` 抽象:生产用 S3/MinIO,测试可注入
//!   `LocalFileSystem` / `InMemory`;
//! - 对象布局:
//!   - 全量快照:`backups/{db_id}/{unix_ts}-{rand}.sqlite`(`VACUUM INTO`);
//!   - **增量备份**:`backups/{db_id}/incr/snapshot-{rev}.sqlite`(主库) +
//!     `backups/{db_id}/incr/wal-{rev}.sqlite-wal`(该时刻累积的 WAL)。
//!     rev 为 unix 毫秒,周期归档"主库 + WAL"对,恢复 = 主库 + WAL 重放(缩短 RPO);
//! - 恢复缺省时优先取增量备份最新,再回退全量快照。

use std::sync::Arc;

use combee_common::rpc::BackupInfo;
use combee_common::{CombeeError, DatabaseId, Result};
use futures::StreamExt;
use object_store::path::Path as ObjPath;
use object_store::{ObjectMeta, ObjectStore, PutPayload};

use crate::ttl;

/// 备份对象前缀。
fn prefix(db: DatabaseId) -> ObjPath {
    ObjPath::from(format!("backups/{db}/"))
}

/// 从 S3 配置构建对象存储客户端(endpoint 为空表示未启用)。
pub fn build_s3_store(
    endpoint: &str,
    access_key: &str,
    secret_key: &str,
    bucket: &str,
    region: &str,
    virtual_hosted: bool,
) -> Result<Arc<dyn ObjectStore>> {
    if endpoint.is_empty() {
        return Err(CombeeError::Internal(
            "object storage not configured (COMBEE_S3_ENDPOINT)".into(),
        ));
    }
    let store = object_store::aws::AmazonS3Builder::new()
        .with_endpoint(endpoint)
        .with_access_key_id(access_key)
        .with_secret_access_key(secret_key)
        .with_bucket_name(bucket)
        .with_region(region)
        .with_allow_http(true)
        .with_virtual_hosted_style_request(virtual_hosted) // true=虚拟主机(COS),false=path-style(MinIO)
        .build()
        .map_err(|e| CombeeError::Internal(format!("s3 build: {e}")))?;
    Ok(Arc::new(store))
}

/// 生成一次快照的临时文件路径(系统临时目录,避免引号问题)。
pub(crate) fn temp_snapshot_path(db: DatabaseId) -> std::path::PathBuf {
    let ts = ttl::unix_now();
    std::env::temp_dir().join(format!(
        "combee-snap-{db}-{ts}-{}.sqlite",
        uuid::Uuid::new_v4()
    ))
}

/// 把快照文件上传到对象存储并返回对象信息。
pub async fn upload_snapshot(
    store: &Arc<dyn ObjectStore>,
    db: DatabaseId,
    tmp_path: &std::path::Path,
) -> Result<BackupInfo> {
    let size = tokio::fs::metadata(tmp_path)
        .await
        .map_err(|e| CombeeError::Internal(format!("snapshot metadata: {e}")))?
        .len();
    let bytes = tokio::fs::read(tmp_path)
        .await
        .map_err(|e| CombeeError::Internal(format!("snapshot read: {e}")))?;
    // key 以毫秒时间戳为前缀(字符串排序即时间序),uuid 兜底同毫秒碰撞
    let ts_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let key = ObjPath::from(format!(
        "backups/{db}/{ts_ms}-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    store
        .put(&key, PutPayload::from(bytes))
        .await
        .map_err(|e| CombeeError::Internal(format!("upload backup: {e}")))?;
    Ok(BackupInfo {
        key: key.to_string(),
        size_bytes: size,
        created_at: ttl::unix_now().max(0) as u64,
    })
}

/// 列出某 db 的全部快照(按对象名排序,通常即时间顺序)。
pub async fn list_snapshots(
    store: &Arc<dyn ObjectStore>,
    db: DatabaseId,
) -> Result<Vec<ObjectMeta>> {
    let mut metas = Vec::new();
    let mut stream = store.list(Some(&prefix(db)));
    while let Some(meta) = stream
        .next()
        .await
        .transpose()
        .map_err(|e| CombeeError::Internal(format!("list backups: {e}")))?
    {
        metas.push(meta);
    }
    metas.sort_by_key(|m| m.location.to_string());
    Ok(metas)
}

/// 下载快照到目标文件(原子:先写临时文件再 rename)。
pub async fn download_snapshot(
    store: &Arc<dyn ObjectStore>,
    key: &ObjPath,
    dest: &std::path::Path,
) -> Result<()> {
    let bytes = store
        .get(key)
        .await
        .map_err(|e| CombeeError::Internal(format!("download backup {key}: {e}")))?
        .bytes()
        .await
        .map_err(|e| CombeeError::Internal(format!("download backup bytes: {e}")))?;
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| CombeeError::Internal(format!("create dir {}: {e}", parent.display())))?;
    }
    let tmp = dest.with_extension("sqlite.restoring");
    tokio::fs::write(&tmp, &bytes)
        .await
        .map_err(|e| CombeeError::Internal(format!("write snapshot: {e}")))?;
    tokio::fs::rename(&tmp, dest)
        .await
        .map_err(|e| CombeeError::Internal(format!("atomic replace: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DataNode;
    use crate::manager::DataNodeConfig;
    use crate::storage;
    use combee_common::config::KvDurability;
    use combee_common::protocol::SqlRequest;
    use object_store::local::LocalFileSystem;

    fn node_with_store(data_dir: &std::path::Path, store_dir: &std::path::Path) -> DataNode {
        let store = Arc::new(LocalFileSystem::new_with_prefix(store_dir).unwrap());
        DataNode::new(DataNodeConfig {
            data_dir: data_dir.to_path_buf(),
            max_active_dbs: 8,
            db_idle_timeout: std::time::Duration::from_secs(3600),
            ttl_gc_interval: std::time::Duration::from_secs(3600),
            kv_cache_capacity: 10_000,
            kv_durability: KvDurability::Normal,
            sql_timeout: Some(std::time::Duration::from_secs(30)),
            quota: Default::default(),
        })
        .with_object_store(store)
    }

    /// 备份 → 模拟节点炸毁(删本地文件)→ 恢复 → 数据一致。
    #[tokio::test]
    async fn backup_then_restore_after_destroy() {
        let dir = tempfile::tempdir().unwrap();
        let os_dir = tempfile::tempdir().unwrap();
        let db = DatabaseId::new();

        {
            let n = node_with_store(dir.path(), os_dir.path());
            n.kv_set(db, "k".into(), "v".into(), None, false, false, 0)
                .await
                .unwrap();
            n.execute_sql(
                db,
                SqlRequest {
                    sql: "CREATE TABLE t (x INTEGER)".into(),
                    params: vec![],
                },
                0,
            )
            .await
            .unwrap();
            n.execute_sql(
                db,
                SqlRequest {
                    sql: "INSERT INTO t VALUES (42)".into(),
                    params: vec![],
                },
                0,
            )
            .await
            .unwrap();

            // backup → 对象存储里应有对象
            let info = n.backup(db).await.unwrap();
            assert!(info.size_bytes > 0, "snapshot non-empty");
            assert!(info.key.contains(&db.to_string()));
            let os: Arc<dyn ObjectStore> =
                Arc::new(LocalFileSystem::new_with_prefix(os_dir.path()).unwrap());
            let metas = list_snapshots(&os, db).await.unwrap();
            assert_eq!(metas.len(), 1);
            n.shutdown().await;
        }

        // 模拟节点炸毁:删除本地 SQLite 文件
        let path = storage::db_path(dir.path(), db);
        for suffix in ["", "-wal", "-shm"] {
            let p = format!("{}{suffix}", path.display());
            if std::path::Path::new(&p).exists() {
                std::fs::remove_file(&p).unwrap();
            }
        }
        assert!(!path.exists(), "node data destroyed");

        // 新节点实例(同数据目录)从最新快照恢复
        let n = node_with_store(dir.path(), os_dir.path());
        n.restore(db, None).await.unwrap();
        assert!(path.exists(), "restored file exists");

        // 数据一致
        let e = n.kv_get(db, "k".into()).await.unwrap().unwrap();
        assert_eq!(e.value, "v");
        let r = n
            .execute_sql(
                db,
                SqlRequest {
                    sql: "SELECT x FROM t".into(),
                    params: vec![],
                },
                0,
            )
            .await
            .unwrap();
        assert_eq!(r.rows, vec![vec![serde_json::json!(42)]]);
    }

    /// 多次备份:list 取最新,指定旧版本也能恢复。
    #[tokio::test]
    async fn restore_specific_version() {
        let dir = tempfile::tempdir().unwrap();
        let os_dir = tempfile::tempdir().unwrap();
        let db = DatabaseId::new();

        {
            let n = node_with_store(dir.path(), os_dir.path());
            n.kv_set(db, "k".into(), "v1".into(), None, false, false, 0)
                .await
                .unwrap();
            let info1 = n.backup(db).await.unwrap();
            n.kv_set(db, "k".into(), "v2".into(), None, false, false, 0)
                .await
                .unwrap();
            n.backup(db).await.unwrap();
            n.shutdown().await;

            // 模拟炸毁
            let path = storage::db_path(dir.path(), db);
            for suffix in ["", "-wal", "-shm"] {
                let p = format!("{}{suffix}", path.display());
                if std::path::Path::new(&p).exists() {
                    std::fs::remove_file(&p).unwrap();
                }
            }

            // 恢复到旧版本 v1
            let n2 = node_with_store(dir.path(), os_dir.path());
            n2.restore(db, Some(info1.key.clone())).await.unwrap();
            let e = n2.kv_get(db, "k".into()).await.unwrap().unwrap();
            assert_eq!(e.value, "v1", "restored specific version");
            n2.shutdown().await;

            // 再恢复到最新(v2)
            let n3 = node_with_store(dir.path(), os_dir.path());
            n3.restore(db, None).await.unwrap();
            let e = n3.kv_get(db, "k".into()).await.unwrap().unwrap();
            assert_eq!(e.value, "v2", "restored latest");
        }
    }
}

// ---- WAL 增量备份 ----

/// 增量备份对象前缀。
fn incr_prefix(db: DatabaseId) -> ObjPath {
    ObjPath::from(format!("backups/{db}/incr/"))
}

/// 解析对象名中的 rev(毫秒时间戳)。
fn parse_rev(name: &str) -> Option<u128> {
    for prefix in ["snapshot-", "wal-"] {
        if let Some(idx) = name.find(prefix) {
            let rest = &name[idx + prefix.len()..];
            let rev: u128 = rest.split('.').next()?.parse().ok()?;
            return Some(rev);
        }
    }
    None
}

/// 上传一轮增量备份(主库 + WAL 对,同一 rev)。WAL 为空可不传。
pub async fn upload_incr(
    store: &Arc<dyn ObjectStore>,
    db: DatabaseId,
    snap_bytes: Vec<u8>,
    wal_bytes: Option<Vec<u8>>,
) -> Result<BackupInfo> {
    let rev = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let snap_key = ObjPath::from(format!("backups/{db}/incr/snapshot-{rev}.sqlite"));
    store
        .put(&snap_key, PutPayload::from(snap_bytes.clone()))
        .await
        .map_err(|e| CombeeError::Internal(format!("upload incr snapshot: {e}")))?;
    let wal_size = if let Some(w) = wal_bytes {
        if w.is_empty() {
            0
        } else {
            let wal_key = ObjPath::from(format!("backups/{db}/incr/wal-{rev}.sqlite-wal"));
            store
                .put(&wal_key, PutPayload::from(w))
                .await
                .map_err(|e| CombeeError::Internal(format!("upload incr wal: {e}")))?;
            wal_key.to_string().len()
        }
    } else {
        0
    };
    Ok(BackupInfo {
        key: snap_key.to_string(),
        size_bytes: snap_bytes.len() as u64 + wal_size as u64,
        created_at: (rev / 1000) as u64,
    })
}

/// 取最新一轮增量备份的 (snapshot key, wal key?)。没有增量备份时返回 None。
pub async fn latest_incr(
    store: &Arc<dyn ObjectStore>,
    db: DatabaseId,
) -> Result<Option<(ObjPath, Option<ObjPath>)>> {
    #[derive(Default)]
    struct IncrMeta {
        snap: Option<ObjPath>,
        wal: Option<ObjPath>,
    }
    let mut by_rev: std::collections::BTreeMap<u128, IncrMeta> = Default::default();
    let mut stream = store.list(Some(&incr_prefix(db)));
    while let Some(meta) = stream
        .next()
        .await
        .transpose()
        .map_err(|e| CombeeError::Internal(format!("list incr backups: {e}")))?
    {
        let name = meta.location.to_string();
        if let Some(rev) = parse_rev(&name) {
            let e = by_rev.entry(rev).or_default();
            if name.contains("snapshot-") {
                e.snap = Some(meta.location.clone());
            } else if name.contains("wal-") {
                e.wal = Some(meta.location.clone());
            }
        }
    }
    Ok(by_rev
        .values()
        .rev()
        .find(|m| m.snap.is_some())
        .map(|m| (m.snap.clone().expect("checked"), m.wal.clone())))
}

/// 从最新增量备份恢复到 `dest`(主库)与 `dest-wal`(WAL)。
/// 返回是否找到了增量备份。
pub async fn restore_from_incr(
    store: &Arc<dyn ObjectStore>,
    db: DatabaseId,
    dest: &std::path::Path,
) -> Result<bool> {
    let Some((snap_key, wal_key)) = latest_incr(store, db).await? else {
        return Ok(false);
    };
    download_snapshot(store, &snap_key, dest).await?;
    let wal_dest = std::path::PathBuf::from(format!("{}-wal", dest.display()));
    match wal_key {
        Some(k) => download_snapshot(store, &k, &wal_dest).await?,
        None => {
            let _ = tokio::fs::remove_file(&wal_dest).await;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod incremental_tests {
    use super::*;
    use crate::storage;
    use crate::{DataNode, DataNodeConfig};
    use combee_common::config::KvDurability;
    use combee_common::protocol::SqlRequest;
    use object_store::local::LocalFileSystem;
    use std::path::Path;

    fn node_with_store(data_dir: &Path, os_dir: &Path) -> DataNode {
        let store: Arc<dyn ObjectStore> =
            Arc::new(LocalFileSystem::new_with_prefix(os_dir).unwrap());
        DataNode::new(DataNodeConfig {
            data_dir: data_dir.to_path_buf(),
            max_active_dbs: 8,
            db_idle_timeout: std::time::Duration::from_secs(3600),
            ttl_gc_interval: std::time::Duration::from_secs(3600),
            kv_cache_capacity: 10_000,
            kv_durability: KvDurability::Normal,
            sql_timeout: Some(std::time::Duration::from_secs(30)),
            quota: Default::default(),
        })
        .with_object_store(store)
    }

    fn destroy_files(dir: &Path, db: DatabaseId) {
        let path = storage::db_path(dir, db);
        for suffix in ["", "-wal", "-shm"] {
            let p = format!("{}{suffix}", path.display());
            if std::path::Path::new(&p).exists() {
                std::fs::remove_file(&p).unwrap();
            }
        }
    }

    /// 多轮增量:恢复点 = 最后一次归档时的状态(归档后的写入不出现)。
    #[tokio::test]
    async fn incremental_restores_latest_archived_state() {
        let dir = tempfile::tempdir().unwrap();
        let os = tempfile::tempdir().unwrap();
        let db = DatabaseId::new();
        {
            let n = node_with_store(dir.path(), os.path());
            n.kv_set(db, "k".into(), "v1".into(), None, false, false, 0)
                .await
                .unwrap();
            n.incremental_backup(db).await.unwrap(); // rev1: v1
            n.kv_set(db, "k".into(), "v2".into(), None, false, false, 0)
                .await
                .unwrap();
            n.execute_sql(
                db,
                SqlRequest {
                    sql: "CREATE TABLE t (x INTEGER)".into(),
                    params: vec![],
                },
                0,
            )
            .await
            .unwrap();
            n.incremental_backup(db).await.unwrap(); // rev2: v2 + t
            n.kv_set(db, "k".into(), "v3".into(), None, false, false, 0)
                .await
                .unwrap(); // 未归档
            n.shutdown().await;
        }
        destroy_files(dir.path(), db);

        let n = node_with_store(dir.path(), os.path());
        n.restore(db, None).await.unwrap();
        let e = n.kv_get(db, "k".into()).await.unwrap().unwrap();
        assert_eq!(
            e.value, "v2",
            "恢复点应为最后一次归档(rev2),未归档的 v3 不应出现"
        );
        let r = n
            .execute_sql(
                db,
                SqlRequest {
                    sql: "SELECT x FROM t".into(),
                    params: vec![],
                },
                0,
            )
            .await
            .unwrap();
        assert_eq!(r.columns, vec!["x".to_string()], "SQL 表也在恢复点");
    }

    /// checkpoint 干扰:实例 1 归档后 shutdown(checkpoint 清 WAL),实例 2 继续写并归档。
    /// 恢复 = 实例 2 的归档点,两个实例的数据都在。
    #[tokio::test]
    async fn incremental_across_checkpoint_restart() {
        let dir = tempfile::tempdir().unwrap();
        let os = tempfile::tempdir().unwrap();
        let db = DatabaseId::new();
        {
            let n = node_with_store(dir.path(), os.path());
            n.kv_set(db, "k".into(), "v1".into(), None, false, false, 0)
                .await
                .unwrap();
            n.incremental_backup(db).await.unwrap();
            n.shutdown().await; // checkpoint(TRUNCATE WAL)
        }
        {
            let n = node_with_store(dir.path(), os.path());
            n.kv_set(db, "k".into(), "v2".into(), None, false, false, 0)
                .await
                .unwrap();
            n.incremental_backup(db).await.unwrap();
            n.shutdown().await;
        }
        destroy_files(dir.path(), db);

        let n = node_with_store(dir.path(), os.path());
        n.restore(db, None).await.unwrap();
        let e = n.kv_get(db, "k".into()).await.unwrap().unwrap();
        assert_eq!(e.value, "v2", "跨 checkpoint 的多轮增量恢复仍正确");
    }

    /// 缺省恢复优先取增量备份(比旧全量快照新)。
    #[tokio::test]
    async fn restore_prefers_incremental_over_full_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let os = tempfile::tempdir().unwrap();
        let db = DatabaseId::new();
        {
            let n = node_with_store(dir.path(), os.path());
            n.kv_set(db, "k".into(), "snap-v".into(), None, false, false, 0)
                .await
                .unwrap();
            n.backup(db).await.unwrap(); // 全量快照: snap-v
            n.kv_set(db, "k".into(), "incr-v".into(), None, false, false, 0)
                .await
                .unwrap();
            n.incremental_backup(db).await.unwrap(); // 增量: incr-v
            n.shutdown().await;
        }
        destroy_files(dir.path(), db);

        let n = node_with_store(dir.path(), os.path());
        n.restore(db, None).await.unwrap();
        let e = n.kv_get(db, "k".into()).await.unwrap().unwrap();
        assert_eq!(e.value, "incr-v", "缺省恢复应优先取更新的增量备份");
    }
}
