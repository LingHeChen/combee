//! SQLite 文件布局与连接初始化。
//!
//! 存储布局(设计文档第 7 节):
//! ```text
//! /data/
//!   00/
//!   ...
//!   ff/
//!       <database-id>.sqlite
//! ```
//! 首次访问时按需创建(lazy create),并初始化 `__sys_*` 内部表。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use combee_common::config::KvDurability;
use combee_common::{CombeeError, DatabaseId, Result};
use rusqlite::Connection;

use crate::sql_err;

/// KV 内部表:SQL 与 KV 共享持久化。
const SCHEMA_KV: &str = "CREATE TABLE IF NOT EXISTS __sys_kv (
    key BLOB PRIMARY KEY,
    value BLOB NOT NULL,
    expires_at INTEGER,
    version INTEGER NOT NULL DEFAULT 0
)";

const SCHEMA_KV_IDX: &str =
    "CREATE INDEX IF NOT EXISTS __sys_kv_expires_at ON __sys_kv(expires_at)";

/// 内部元数据表(预留 migration 用)。
const SCHEMA_META: &str =
    "CREATE TABLE IF NOT EXISTS __sys_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)";

/// 计算数据库文件的落盘路径。
pub fn db_path(data_dir: &Path, db: DatabaseId) -> PathBuf {
    let hex = db.0.to_string();
    let bucket = &hex[..2];
    data_dir.join(bucket).join(format!("{hex}.sqlite"))
}

/// 打开(必要时创建)一个 Cell 的 SQLite 连接,并初始化 schema。
/// `durability` 决定 `synchronous` pragma(设计文档第 14 节)。
pub fn open(path: &Path, durability: KvDurability) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            CombeeError::Internal(format!("create data dir {}: {e}", parent.display()))
        })?;
    }
    let conn = Connection::open(path).map_err(sql_err)?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(sql_err)?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(sql_err)?;
    // synchronous pragma 需要 OFF / NORMAL / FULL(不能用 KvDurability 的枚举名)
    let sync_mode = match durability {
        KvDurability::Fast => "OFF",
        KvDurability::Normal => "NORMAL",
        KvDurability::Strict => "FULL",
    };
    conn.pragma_update(None, "synchronous", sync_mode)
        .map_err(sql_err)?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(sql_err)?;
    init_schema(&conn)?;
    Ok(conn)
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA_KV).map_err(sql_err)?;
    conn.execute_batch(SCHEMA_KV_IDX).map_err(sql_err)?;
    conn.execute_batch(SCHEMA_META).map_err(sql_err)?;
    conn.execute(
        "INSERT OR IGNORE INTO __sys_meta (key, value) VALUES ('schema_version', '1')",
        [],
    )
    .map_err(sql_err)?;
    Ok(())
}

/// 将 WAL 合并回主库(连接休眠/关闭前调用)。
pub fn checkpoint(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
        .map_err(sql_err)
}

/// 删除数据库对应的主库、WAL、SHM 文件(文件不存在时静默跳过)。
pub fn remove_files(data_dir: &Path, db: DatabaseId) -> Result<()> {
    let path = db_path(data_dir, db);
    for suffix in ["", "-wal", "-shm"] {
        let p = PathBuf::from(format!("{}{suffix}", path.display()));
        if p.exists() {
            fs::remove_file(&p)
                .map_err(|e| CombeeError::Internal(format!("remove {}: {e}", p.display())))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn db_path_uses_hex_bucket() {
        let dir = tempfile::tempdir().unwrap();
        let id = DatabaseId::from_str("01234567-89ab-cdef-0123-456789abcdef").unwrap();
        let p = db_path(dir.path(), id);
        let hex = id.0.to_string();
        let bucket = p.parent().unwrap().file_name().unwrap().to_str().unwrap();
        assert_eq!(bucket, &hex[..2], "bucket = first two hex chars");
        assert_eq!(
            p.file_name().unwrap().to_str().unwrap(),
            format!("{hex}.sqlite")
        );
    }

    #[test]
    fn db_path_is_stable_and_unique_per_db() {
        let dir = tempfile::tempdir().unwrap();
        let a = DatabaseId::new();
        let b = DatabaseId::new();
        let p1 = db_path(dir.path(), a);
        let p2 = db_path(dir.path(), a);
        assert_eq!(p1, p2, "same db -> same path");
        assert_ne!(p1, db_path(dir.path(), b), "different db -> different path");
    }

    #[test]
    fn open_initializes_internal_schema_and_wal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.sqlite");
        let conn = open(&path, KvDurability::Normal).unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<String>, _>>()
            .unwrap();
        assert!(tables.contains(&"__sys_kv".to_string()));
        assert!(tables.contains(&"__sys_meta".to_string()));
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn open_is_idempotent_across_connections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.sqlite");
        {
            let conn = open(&path, KvDurability::Normal).unwrap();
            conn.execute("CREATE TABLE t (x)", []).unwrap();
            conn.execute("INSERT INTO t VALUES (1)", []).unwrap();
            checkpoint(&conn).unwrap();
        }
        // 重新打开:数据仍在,schema 幂等
        let conn = open(&path, KvDurability::Normal).unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        // schema_version 唯一
        let v: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM __sys_meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, 1);
    }

    #[test]
    fn checkpoint_flushes_wal_into_main_db() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.sqlite");
        {
            let conn = open(&path, KvDurability::Normal).unwrap();
            conn.execute("CREATE TABLE t (x)", []).unwrap();
            conn.execute("INSERT INTO t VALUES (1)", []).unwrap();
            checkpoint(&conn).unwrap();
            // checkpoint 后 WAL 被清空
            let wal_size: i64 = conn
                .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |r| r.get(0))
                .unwrap();
            assert_eq!(wal_size, 0, "wal should be empty after TRUNCATE checkpoint");
        }
        let conn = open(&path, KvDurability::Normal).unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn remove_files_cleans_all_suffixes() {
        let dir = tempfile::tempdir().unwrap();
        let id = DatabaseId::new();
        let path = db_path(dir.path(), id);
        // 打开连接会产生 -wal / -shm
        let conn = open(&path, KvDurability::Normal).unwrap();
        conn.execute("CREATE TABLE t (x)", []).unwrap();
        assert!(path.exists());
        drop(conn);

        remove_files(dir.path(), id).unwrap();
        assert!(!path.exists());
        assert!(!PathBuf::from(format!("{}-wal", path.display())).exists());
        assert!(!PathBuf::from(format!("{}-shm", path.display())).exists());
    }

    #[test]
    fn remove_files_skips_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let id = DatabaseId::new();
        // 从未创建过的 db(懒创建未触发)
        remove_files(dir.path(), id).unwrap();
    }

    #[test]
    fn open_applies_durability_pragma() {
        // PRAGMA synchronous 返回整数:0=OFF, 1=NORMAL, 2=FULL
        let dir = tempfile::tempdir().unwrap();
        for (durability, expected) in [
            (KvDurability::Fast, 0),
            (KvDurability::Normal, 1),
            (KvDurability::Strict, 2),
        ] {
            let path = dir.path().join(format!("{}.sqlite", durability.as_str()));
            let conn = open(&path, durability).unwrap();
            let sync: i64 = conn
                .query_row("PRAGMA synchronous", [], |r| r.get(0))
                .unwrap();
            eprintln!("durability={durability} -> synchronous={sync}");
            assert_eq!(
                sync, expected,
                "{} should map to synchronous={}",
                durability, expected
            );
        }
    }
}
