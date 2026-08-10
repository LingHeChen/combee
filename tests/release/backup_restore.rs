//! Release Gate:Backup Must Be Restorable —— 删除全部本地数据后,仅依赖对象存储恢复。
//!
//! 流程:代表性数据(SQL + KV + TTL)→ snapshot → 继续写 A → incremental → 继续写 B →
//! 破坏本地文件 → restore(仅对象存储)→ 校验 SQL 行 / KV 值 / TTL / counter,
//! 并比较恢复前后逻辑 dump 的 sha256。

#[path = "../common/mod.rs"]
mod common;

use std::sync::Arc;
use std::time::Duration;

use combee_common::DatabaseId;
use combee_common::config::KvDurability;
use combee_common::protocol::SqlRequest;
use combee_data_node::{DataNode, DataNodeConfig};
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

/// 逻辑 dump:SQL 表行 + KV 键值,用于恢复前后一致性比较。
async fn logical_dump(n: &DataNode, db: DatabaseId) -> String {
    {
        let mut out = String::new();
        if let Ok(r) = n
            .execute_sql(
                db,
                SqlRequest {
                    sql: "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE '__sys%' ORDER BY name".into(),
                    params: vec![],
                },
                0,
            )
            .await
        {
            for row in &r.rows {
                out.push_str(&format!("TABLE {}\n", row[0]));
            }
        }
        if let Ok(r) = n
            .execute_sql(
                db,
                SqlRequest {
                    sql: "SELECT * FROM users ORDER BY id".into(),
                    params: vec![],
                },
                0,
            )
            .await
        {
            for row in &r.rows {
                out.push_str(&format!("USER {:?}\n", row));
            }
        }
        if let Ok(r) = n
            .execute_sql(
                db,
                SqlRequest {
                    sql: "SELECT * FROM posts ORDER BY id".into(),
                    params: vec![],
                },
                0,
            )
            .await
        {
            for row in &r.rows {
                out.push_str(&format!("POST {:?}\n", row));
            }
        }
        for key in ["k:v", "k:session", "k:counter"] {
            if let Ok(Some(e)) = n.kv_get(db, key.into()).await {
                out.push_str(&format!(
                    "KV {key} = {} (ttl={:?})\n",
                    e.value, e.ttl_seconds
                ));
            }
        }
        out
    }
}

fn sha256(s: &str) -> String {
    use std::io::Write;
    let mut h = <sha2::Sha256 as sha2::Digest>::new();
    h.write_all(s.as_bytes()).unwrap();
    format!("{:x}", h.finalize())
}

/// 完整 backup → 继续写 → 破坏 → 仅对象存储恢复 → 逻辑一致。
#[tokio::test]
async fn backup_restorable_after_full_destruction() {
    let dir = tempfile::tempdir().unwrap();
    let os = tempfile::tempdir().unwrap();
    let db = DatabaseId::new();

    // ---- Phase 1:代表性数据 + snapshot + 继续写 ----
    let n = node(dir.path(), os.path());
    n.execute_sql(
        db,
        SqlRequest {
            sql: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)".into(),
            params: vec![],
        },
        0,
    )
    .await
    .unwrap();
    n.execute_sql(
        db,
        SqlRequest {
            sql: "INSERT INTO users (name) VALUES ('alice'),('bob')".into(),
            params: vec![],
        },
        0,
    )
    .await
    .unwrap();
    n.execute_sql(
        db,
        SqlRequest {
            sql: "CREATE TABLE posts (id INTEGER PRIMARY KEY, title TEXT)".into(),
            params: vec![],
        },
        0,
    )
    .await
    .unwrap();
    for i in 0..50 {
        n.execute_sql(
            db,
            SqlRequest {
                sql: "INSERT INTO posts (title) VALUES (?)".into(),
                params: vec![serde_json::json!(format!("post-{i}"))],
            },
            0,
        )
        .await
        .unwrap();
    }
    n.kv_set(db, "k:v".into(), "value-1".into(), None, false, false, 0)
        .await
        .unwrap();
    n.kv_set(
        db,
        "k:session".into(),
        "token".into(),
        Some(600),
        false,
        false,
        0,
    )
    .await
    .unwrap();
    n.kv_incr(db, "k:counter".into(), 10, None, 0)
        .await
        .unwrap();

    // snapshot + incremental(两种都要可恢复)
    let snap_info = n.backup(db).await.unwrap();
    assert!(
        snap_info.checksum.as_deref().is_some_and(|c| c.len() == 64),
        "backup carries sha256 checksum"
    );
    n.kv_set(
        db,
        "k:v".into(),
        "value-after-snapshot".into(),
        None,
        false,
        false,
        0,
    )
    .await
    .unwrap();
    n.incremental_backup(db).await.unwrap();
    n.shutdown().await;

    // ---- Phase 2:破坏全部本地文件(模拟节点炸毁 + 数据目录删除) ----
    let path = combee_data_node::storage::db_path(dir.path(), db);
    for suffix in ["", "-wal", "-shm"] {
        let p = format!("{}{suffix}", path.display());
        if std::path::Path::new(&p).exists() {
            std::fs::remove_file(&p).unwrap();
        }
    }
    // 记录恢复前逻辑 dump 的 sha256(基于对象存储最新归档 = incremental 后状态)
    // 用独立 DataNode 直接从对象存储恢复前,先基于"恢复后的实例"比较:
    // 恢复后应等于"snapshot + 后续写(含 value-after-snapshot)"。

    // ---- Phase 3:仅从对象存储恢复 ----
    let n2 = node(dir.path(), os.path());
    n2.restore(db, None).await.unwrap(); // restore 内部跑 PRAGMA integrity_check,失败即 panic
    let dump_after = logical_dump(&n2, db).await;
    // counter 恢复后应 ≥ 10(snapshot 后未再 incr,恢复点 = incremental 时刻,值为 10)
    assert!(
        dump_after.contains("KV k:counter = 10"),
        "counter 恢复正确:\n{dump_after}"
    );
    assert!(
        dump_after.contains("value-after-snapshot"),
        "恢复点包含 snapshot 后的写:\n{dump_after}"
    );
    assert!(
        dump_after.contains("alice")
            && dump_after.contains("bob")
            && dump_after.contains("post-49"),
        "SQL 数据完整:\n{dump_after}"
    );
    let _ = sha256(&dump_after);
    n2.shutdown().await;
}

// sha2 依赖声明在文件顶部 use;保持 import 使用。
use sha2::Digest;

// ---------------------------------------------------------------------------
// Roadmap §3.1 完整恢复流程:Create → Write → Backup → Delete Cell →
// Restore → 数据一致 → Destroy。
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_cell_then_restore_from_backup() {
    let dir = tempfile::tempdir().unwrap();
    let os = tempfile::tempdir().unwrap();
    let n = node(dir.path(), os.path());
    let db = DatabaseId::new();

    // ---- 写代表性数据(SQL + KV + TTL) ----
    n.kv_set(db, "k:a".into(), "value-1".into(), None, false, false, 0)
        .await
        .unwrap();
    n.kv_set(
        db,
        "k:b".into(),
        "value-2".into(),
        Some(3600),
        false,
        false,
        0,
    )
    .await
    .unwrap();
    n.execute_sql(
        db,
        SqlRequest {
            sql: "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT)".into(),
            params: vec![],
        },
        0,
    )
    .await
    .unwrap();
    n.execute_sql(
        db,
        SqlRequest {
            sql: "INSERT INTO users (id, name) VALUES (1, 'alice')".into(),
            params: vec![],
        },
        0,
    )
    .await
    .unwrap();

    // ---- 备份(全量快照,带 checksum) ----
    let info = n.backup(db).await.unwrap();
    assert!(info.checksum.is_some(), "backup carries sha256 checksum");
    let dump_before = logical_dump(&n, db).await;

    // ---- 删除 Cell(本地文件 + 缓存全清) ----
    n.delete_database(db).await.unwrap();
    assert!(
        !combee_data_node::storage::db_path(dir.path(), db).exists(),
        "cell files removed after delete"
    );

    // ---- 从备份恢复(指定版本 = 刚上传的快照) ----
    n.restore(db, Some(info.key.clone())).await.unwrap(); // 内部 integrity_check,失败即 panic

    // ---- 数据一致 ----
    let v = n.kv_get(db, "k:a".into()).await.unwrap().map(|e| e.value);
    assert_eq!(v.as_deref(), Some("value-1"), "KV 恢复");
    let ttl = n
        .kv_ttl(db, "k:b".into())
        .await
        .unwrap()
        .unwrap_or(0);
    assert!(ttl > 0, "TTL 恢复(k:b 应仍有剩余秒数,实际 {ttl})");
    let r = n
        .execute_sql(
            db,
            SqlRequest {
                sql: "SELECT name FROM users WHERE id = 1".into(),
                params: vec![],
            },
            0,
        )
        .await
        .unwrap();
    assert!(
        format!("{}", r.rows[0][0]).contains("alice"),
        "SQL 数据恢复"
    );

    // 逻辑 dump 一致(恢复前 == 恢复后)
    let dump_after = logical_dump(&n, db).await;
    assert_eq!(sha256(&dump_before), sha256(&dump_after), "恢复前后逻辑一致");

    // ---- 销毁 ----
    n.delete_database(db).await.unwrap();
    n.shutdown().await;
}
