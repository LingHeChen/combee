//! KV 操作:基于每个 Cell 的 `__sys_kv` 内部表实现 Redis-style 语义。
//!
//! 存储模型(设计文档第 10 节):
//! ```sql
//! CREATE TABLE __sys_kv (
//!     key BLOB PRIMARY KEY,
//!     value BLOB NOT NULL,
//!     expires_at INTEGER,
//!     version INTEGER NOT NULL DEFAULT 0
//! );
//! ```
//! 同一 Cell 内操作由 `ActiveDbManager` 串行化,因此读改写(如 INCR)天然原子。

use combee_common::protocol::{KvEntry, KvSetItem};
use combee_common::{CombeeError, Result};
use rusqlite::{Connection, params};

use crate::sql_err;
use crate::ttl::{expires_at_from, is_expired, ttl_or_remaining, unix_now};

const SELECT_ROW: &str = "SELECT value, expires_at FROM __sys_kv WHERE key = ?1";

/// 写覆盖(不存在则插入)。
const UPSERT: &str = "INSERT INTO __sys_kv (key, value, expires_at, version) VALUES (?1, ?2, ?3, 1)
    ON CONFLICT(key) DO UPDATE SET value = excluded.value, expires_at = excluded.expires_at, version = version + 1";

/// 仅当 key 不存在时写入(SET NX)。
const INSERT_IF_ABSENT: &str =
    "INSERT INTO __sys_kv (key, value, expires_at, version) VALUES (?1, ?2, ?3, 1)
    ON CONFLICT(key) DO NOTHING";

/// 仅当 key 存在时更新(SET XX)。
const UPDATE_IF_PRESENT: &str =
    "UPDATE __sys_kv SET value = ?2, expires_at = ?3, version = version + 1 WHERE key = ?1";

fn invalid(msg: impl Into<String>) -> CombeeError {
    CombeeError::InvalidRequest(msg.into())
}

fn read_row(conn: &Connection, key: &str) -> Result<Option<(Vec<u8>, Option<i64>)>> {
    let mut stmt = conn.prepare(SELECT_ROW).map_err(sql_err)?;
    let mut rows = stmt.query(params![key.as_bytes()]).map_err(sql_err)?;
    match rows.next().map_err(sql_err)? {
        Some(row) => {
            let value: Vec<u8> = row.get(0).map_err(sql_err)?;
            let expires_at: Option<i64> = row.get(1).map_err(sql_err)?;
            Ok(Some((value, expires_at)))
        }
        None => Ok(None),
    }
}

/// 读取 key 的值与**绝对过期时间**(供缓存填充使用)。
/// 过期 key 视为不存在,并顺手删除(lazy expiration)。
pub fn read_with_expiry(conn: &Connection, key: &str) -> Result<Option<(String, Option<i64>)>> {
    let now = unix_now();
    match read_row(conn, key)? {
        Some((value, expires_at)) if !is_expired(expires_at, now) => {
            let value =
                String::from_utf8(value).map_err(|_| invalid("stored value is not valid UTF-8"))?;
            Ok(Some((value, expires_at)))
        }
        Some(_) => {
            let _ = del(conn, key);
            Ok(None)
        }
        None => Ok(None),
    }
}

/// GET key。过期 key 视为不存在,并顺手删除(lazy expiration)。
pub fn get(conn: &Connection, key: &str) -> Result<Option<KvEntry>> {
    let now = unix_now();
    match read_with_expiry(conn, key)? {
        Some((value, expires_at)) => Ok(Some(KvEntry {
            value,
            ttl_seconds: ttl_or_remaining(expires_at, now),
        })),
        None => Ok(None),
    }
}

/// SET key value [EX seconds] [NX|XX]。返回是否真正写入。
pub fn set(
    conn: &Connection,
    key: &str,
    value: &str,
    ttl_seconds: Option<u64>,
    nx: bool,
    xx: bool,
) -> Result<bool> {
    if key.is_empty() {
        return Err(invalid("key must not be empty"));
    }
    let expires_at = expires_at_from(ttl_seconds, unix_now());
    let changed = if nx {
        conn.execute(
            INSERT_IF_ABSENT,
            params![key.as_bytes(), value.as_bytes(), expires_at],
        )
        .map_err(sql_err)?
    } else if xx {
        conn.execute(
            UPDATE_IF_PRESENT,
            params![key.as_bytes(), value.as_bytes(), expires_at],
        )
        .map_err(sql_err)?
    } else {
        conn.execute(
            UPSERT,
            params![key.as_bytes(), value.as_bytes(), expires_at],
        )
        .map_err(sql_err)?;
        1
    };
    Ok(changed > 0)
}

/// DEL key。返回是否删除了 key。
pub fn del(conn: &Connection, key: &str) -> Result<bool> {
    let n = conn
        .execute(
            "DELETE FROM __sys_kv WHERE key = ?1",
            params![key.as_bytes()],
        )
        .map_err(sql_err)?;
    Ok(n > 0)
}

/// EXISTS key(忽略已过期)。
pub fn exists(conn: &Connection, key: &str) -> Result<bool> {
    let now = unix_now();
    match read_row(conn, key)? {
        Some((_, expires_at)) => Ok(!is_expired(expires_at, now)),
        None => Ok(false),
    }
}

/// MGET:批量读取,与 keys 顺序一一对应。
pub fn mget(conn: &Connection, keys: &[String]) -> Result<Vec<Option<String>>> {
    let mut out = Vec::with_capacity(keys.len());
    for k in keys {
        out.push(get(conn, k)?.map(|e| e.value));
    }
    Ok(out)
}

/// MSET:批量写入,原子性由 per-db 串行保证。
pub fn mset(conn: &Connection, items: &[KvSetItem]) -> Result<()> {
    for it in items {
        let written = set(conn, &it.key, &it.value, it.ttl_seconds, false, false)?;
        debug_assert!(written);
    }
    Ok(())
}

/// TTL key:存在且带 TTL 时返回剩余秒数;持久 key 返回 `Some(-1)`;不存在返回 `None`。
pub fn ttl(conn: &Connection, key: &str) -> Result<Option<i64>> {
    let now = unix_now();
    match read_row(conn, key)? {
        Some((_, Some(expires_at))) if !is_expired(Some(expires_at), now) => {
            Ok(Some((expires_at - now).max(0)))
        }
        Some((_, None)) => Ok(Some(-1)),
        _ => Ok(None),
    }
}

/// EXPIRE key seconds / PERSIST key(ttl 为 None)。返回 key 是否存在(未过期)。
pub fn expire(conn: &Connection, key: &str, ttl_seconds: Option<u64>) -> Result<bool> {
    if !exists(conn, key)? {
        return Ok(false);
    }
    let expires_at = expires_at_from(ttl_seconds, unix_now());
    conn.execute(
        "UPDATE __sys_kv SET expires_at = ?2, version = version + 1 WHERE key = ?1",
        params![key.as_bytes(), expires_at],
    )
    .map_err(sql_err)?;
    Ok(true)
}

/// INCR / DECR:key 不存在时初始化为 `delta`;值非整数时报错。
/// 携带 TTL 时更新过期时间,否则保留原有 TTL(Redis 语义)。
pub fn incr(conn: &Connection, key: &str, delta: i64, ttl_seconds: Option<u64>) -> Result<i64> {
    if key.is_empty() {
        return Err(invalid("key must not be empty"));
    }
    let now = unix_now();
    let (new_value, expires_at) = match read_row(conn, key)? {
        Some((value, old_expires)) if !is_expired(old_expires, now) => {
            let text =
                std::str::from_utf8(&value).map_err(|_| invalid("value is not an integer"))?;
            let old: i64 = text
                .parse()
                .map_err(|_| invalid(format!("value is not an integer: {text:?}")))?;
            let new = old
                .checked_add(delta)
                .ok_or_else(|| invalid("integer overflow"))?;
            let exp = ttl_seconds.map(|s| now + s as i64).or(old_expires);
            (new, exp)
        }
        Some((_, _)) => {
            // 已过期:按不存在处理
            let _ = del(conn, key);
            (delta, ttl_seconds.map(|s| now + s as i64))
        }
        None => (delta, ttl_seconds.map(|s| now + s as i64)),
    };
    conn.execute(
        UPSERT,
        params![key.as_bytes(), new_value.to_string().as_bytes(), expires_at],
    )
    .map_err(sql_err)?;
    Ok(new_value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage;
    use combee_common::config::KvDurability;

    /// 用真实 storage::open(含 __sys_kv schema)构造测试连接。
    fn kv_conn() -> (Connection, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let conn = storage::open(&dir.path().join("kv.sqlite"), KvDurability::Normal).unwrap();
        (conn, dir)
    }

    #[test]
    fn set_get_roundtrip() {
        let (conn, _d) = kv_conn();
        assert!(set(&conn, "k", "v", None, false, false).unwrap());
        let e = get(&conn, "k").unwrap().unwrap();
        assert_eq!(e.value, "v");
        assert_eq!(e.ttl_seconds, None);
    }

    #[test]
    fn set_overwrites_existing() {
        let (conn, _d) = kv_conn();
        set(&conn, "k", "v1", None, false, false).unwrap();
        set(&conn, "k", "v2", None, false, false).unwrap();
        assert_eq!(get(&conn, "k").unwrap().unwrap().value, "v2");
    }

    #[test]
    fn empty_key_rejected() {
        let (conn, _d) = kv_conn();
        assert!(matches!(
            set(&conn, "", "v", None, false, false),
            Err(CombeeError::InvalidRequest(_))
        ));
        assert!(matches!(
            incr(&conn, "", 1, None),
            Err(CombeeError::InvalidRequest(_))
        ));
    }

    #[test]
    fn get_missing_returns_none() {
        let (conn, _d) = kv_conn();
        assert!(get(&conn, "nope").unwrap().is_none());
    }

    #[test]
    fn get_expired_invisible_and_deleted_lazily() {
        let (conn, _d) = kv_conn();
        set(&conn, "k", "v", Some(0), false, false).unwrap();
        assert!(get(&conn, "k").unwrap().is_none());
        // lazy expiration 应顺手删除底层行
        assert!(read_row(&conn, "k").unwrap().is_none());
    }

    #[test]
    fn get_non_utf8_value_rejected() {
        let (conn, _d) = kv_conn();
        conn.execute(
            "INSERT INTO __sys_kv (key, value, expires_at) VALUES (?1, ?2, ?3)",
            params![b"k", b"\xff\xfe", Option::<i64>::None],
        )
        .unwrap();
        assert!(matches!(
            get(&conn, "k"),
            Err(CombeeError::InvalidRequest(_))
        ));
    }

    #[test]
    fn set_nx_and_xx() {
        let (conn, _d) = kv_conn();
        assert!(set(&conn, "k", "a", None, true, false).unwrap());
        assert!(
            !set(&conn, "k", "b", None, true, false).unwrap(),
            "NX must not overwrite"
        );
        assert_eq!(get(&conn, "k").unwrap().unwrap().value, "a");
        assert!(
            !set(&conn, "nope", "x", None, false, true).unwrap(),
            "XX needs existing key"
        );
        assert!(set(&conn, "k", "c", None, false, true).unwrap());
        assert_eq!(get(&conn, "k").unwrap().unwrap().value, "c");
    }

    #[test]
    fn del_semantics() {
        let (conn, _d) = kv_conn();
        assert!(!del(&conn, "k").unwrap());
        set(&conn, "k", "v", None, false, false).unwrap();
        assert!(del(&conn, "k").unwrap());
        assert!(!del(&conn, "k").unwrap());
    }

    #[test]
    fn exists_ignores_expired() {
        let (conn, _d) = kv_conn();
        assert!(!exists(&conn, "k").unwrap());
        set(&conn, "k", "v", None, false, false).unwrap();
        assert!(exists(&conn, "k").unwrap());
        set(&conn, "e", "v", Some(0), false, false).unwrap();
        assert!(!exists(&conn, "e").unwrap());
    }

    #[test]
    fn mget_mset_preserve_order() {
        let (conn, _d) = kv_conn();
        mset(
            &conn,
            &[
                KvSetItem {
                    key: "a".into(),
                    value: "1".into(),
                    ttl_seconds: None,
                },
                KvSetItem {
                    key: "b".into(),
                    value: "2".into(),
                    ttl_seconds: Some(100),
                },
            ],
        )
        .unwrap();
        let vals = mget(&conn, &["b".into(), "a".into(), "z".into()]).unwrap();
        assert_eq!(vals, vec![Some("2".into()), Some("1".into()), None]);
    }

    #[test]
    fn ttl_semantics() {
        let (conn, _d) = kv_conn();
        assert_eq!(ttl(&conn, "missing").unwrap(), None, "missing -> None");
        set(&conn, "k", "v", None, false, false).unwrap();
        assert_eq!(ttl(&conn, "k").unwrap(), Some(-1), "persistent -> -1");
        set(&conn, "e", "v", Some(0), false, false).unwrap();
        assert_eq!(ttl(&conn, "e").unwrap(), None, "expired -> None");
        set(&conn, "t", "v", Some(100), false, false).unwrap();
        let t = ttl(&conn, "t").unwrap().unwrap();
        assert!(
            (1..=100).contains(&t),
            "remaining seconds in (0,100], got {t}"
        );
    }

    #[test]
    fn expire_and_persist() {
        let (conn, _d) = kv_conn();
        assert!(
            !expire(&conn, "k", Some(10)).unwrap(),
            "missing key -> false"
        );
        set(&conn, "k", "v", None, false, false).unwrap();
        assert!(expire(&conn, "k", Some(100)).unwrap());
        assert!(ttl(&conn, "k").unwrap().unwrap() > 0);
        assert!(expire(&conn, "k", None).unwrap(), "persist");
        assert_eq!(ttl(&conn, "k").unwrap(), Some(-1));
    }

    #[test]
    fn incr_semantics() {
        let (conn, _d) = kv_conn();
        assert_eq!(incr(&conn, "c", 1, None).unwrap(), 1, "starts from zero");
        assert_eq!(incr(&conn, "c", 10, None).unwrap(), 11);
        assert_eq!(
            incr(&conn, "c", -5, None).unwrap(),
            6,
            "negative delta = DECR"
        );
    }

    #[test]
    fn incr_keeps_original_ttl() {
        let (conn, _d) = kv_conn();
        set(&conn, "c", "5", Some(100), false, false).unwrap();
        incr(&conn, "c", 1, None).unwrap();
        let t = ttl(&conn, "c").unwrap().unwrap();
        assert!(
            (1..=100).contains(&t),
            "incr without ttl must keep original ttl, got {t}"
        );
    }

    #[test]
    fn incr_sets_ttl_when_provided() {
        let (conn, _d) = kv_conn();
        incr(&conn, "c", 1, Some(100)).unwrap();
        let t = ttl(&conn, "c").unwrap().unwrap();
        assert!((1..=100).contains(&t));
    }

    #[test]
    fn incr_on_non_integer_errors() {
        let (conn, _d) = kv_conn();
        set(&conn, "s", "hello", None, false, false).unwrap();
        assert!(matches!(
            incr(&conn, "s", 1, None),
            Err(CombeeError::InvalidRequest(_))
        ));
    }

    #[test]
    fn incr_overflow_errors() {
        let (conn, _d) = kv_conn();
        set(&conn, "m", &i64::MAX.to_string(), None, false, false).unwrap();
        assert!(matches!(
            incr(&conn, "m", 1, None),
            Err(CombeeError::InvalidRequest(_))
        ));
    }

    #[test]
    fn incr_on_expired_resets() {
        let (conn, _d) = kv_conn();
        set(&conn, "c", "5", Some(0), false, false).unwrap();
        assert_eq!(
            incr(&conn, "c", 1, None).unwrap(),
            1,
            "expired key resets to delta"
        );
    }
}
