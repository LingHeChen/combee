//! TTL 支持:lazy expiration + 后台 GC 两层策略。

use std::time::{SystemTime, UNIX_EPOCH};

use combee_common::Result;
use rusqlite::{Connection, params};

use crate::sql_err;

/// 当前 unix 时间(秒)。
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 判断是否已过期(`expires_at <= now` 即视为过期)。
pub fn is_expired(expires_at: Option<i64>, now: i64) -> bool {
    matches!(expires_at, Some(e) if e <= now)
}

/// 由绝对过期时间计算剩余秒数;持久 key(无过期)返回 `None`。
pub fn ttl_or_remaining(expires_at: Option<i64>, now: i64) -> Option<i64> {
    expires_at.map(|e| (e - now).max(0))
}

/// 由相对 TTL 秒数计算绝对过期时间。
pub fn expires_at_from(ttl_seconds: Option<u64>, now: i64) -> Option<i64> {
    ttl_seconds.map(|s| now + s as i64)
}

/// 后台 GC:删除已过期 key,单次最多 `limit` 条,返回删除条数。
///
/// 注意:bundled SQLite 默认未开启 `SQLITE_ENABLE_UPDATE_DELETE_LIMIT`,
/// `DELETE ... LIMIT` 会语法报错,因此用子查询挑选待删除行。
pub fn gc_expired(conn: &Connection, limit: i64) -> Result<u64> {
    let now = unix_now();
    let n = conn
        .execute(
            "DELETE FROM __sys_kv WHERE key IN (
                SELECT key FROM __sys_kv
                WHERE expires_at IS NOT NULL AND expires_at <= ?1
                LIMIT ?2
             )",
            params![now, limit],
        )
        .map_err(sql_err)?;
    Ok(n as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage;
    use combee_common::config::KvDurability;

    fn gc_conn() -> (Connection, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let conn = storage::open(&dir.path().join("ttl.sqlite"), KvDurability::Normal).unwrap();
        (conn, dir)
    }

    fn insert(conn: &Connection, key: &[u8], expires_at: Option<i64>) {
        conn.execute(
            "INSERT INTO __sys_kv (key, value, expires_at) VALUES (?1, ?2, ?3)",
            params![key, b"v", expires_at],
        )
        .unwrap();
    }

    #[test]
    fn is_expired_boundaries() {
        assert!(
            is_expired(Some(100), 100),
            "expires_at == now counts as expired"
        );
        assert!(is_expired(Some(99), 100));
        assert!(!is_expired(Some(101), 100));
        assert!(!is_expired(None, 100), "persistent key never expires");
    }

    #[test]
    fn expires_at_from_and_remaining() {
        assert_eq!(expires_at_from(None, 1000), None);
        assert_eq!(expires_at_from(Some(60), 1000), Some(1060));
        assert_eq!(ttl_or_remaining(None, 1000), None);
        assert_eq!(ttl_or_remaining(Some(1060), 1000), Some(60));
        assert_eq!(
            ttl_or_remaining(Some(990), 1000),
            Some(0),
            "already expired clamps to 0"
        );
    }

    #[test]
    fn unix_now_is_reasonable() {
        let now = unix_now();
        // 2020-01-01 ~ 2100-01-01 之间
        assert!(
            (1_577_836_800..=4_102_444_800).contains(&now),
            "unix_now out of range: {now}"
        );
    }

    #[test]
    fn gc_removes_only_expired() {
        let (conn, _d) = gc_conn();
        let now = unix_now();
        insert(&conn, b"expired", Some(now - 1));
        insert(&conn, b"live", Some(now + 100));
        insert(&conn, b"persist", None);

        let deleted = gc_expired(&conn, 100).unwrap();
        assert_eq!(deleted, 1, "only the expired key should be deleted");

        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM __sys_kv", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 2);
    }

    #[test]
    fn gc_respects_limit() {
        let (conn, _d) = gc_conn();
        let now = unix_now();
        for i in 0..5 {
            insert(&conn, &[i], Some(now - 1));
        }
        let deleted = gc_expired(&conn, 2).unwrap();
        assert_eq!(deleted, 2, "limit caps deletions per call");
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM __sys_kv", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 3, "next GC round can pick up the rest");
        assert_eq!(gc_expired(&conn, 100).unwrap(), 3);
    }
}
