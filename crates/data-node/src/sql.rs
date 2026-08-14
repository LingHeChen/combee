//! SQL 执行:直接使用 SQLite,不重造 parser / planner / 事务引擎。

use combee_common::protocol::{SqlRequest, SqlResult, TransactionRequest};
use combee_common::{CombeeError, Result};
use rusqlite::{Connection, Row, types::Value as SqlValue};
use serde_json::Value as Json;

use crate::sql_err;

/// 这些语句会改变连接的事务状态或附加数据库,破坏连接复用模型,一律拒绝。
/// 事务请走 `/transaction` 端点。
const FORBIDDEN_PREFIXES: &[&str] = &[
    "begin",
    "commit",
    "rollback",
    "end",
    "savepoint",
    "release",
    "attach",
    "detach",
    // VACUUM INTO 可把 Cell 数据写到任意文件系统路径(逃逸);VACUUM 本身对用户无必要暴露
    "vacuum",
];

/// 判断 SQL 是否包含"多条语句"。
///
/// `rusqlite::prepare` 只编译第一条语句并静默忽略其余部分,
/// 这会让 `"SELECT 1; DROP TABLE t"` 中的第二条语句被悄悄丢弃,
/// 行为不透明且有注入风险,因此显式拒绝。
/// 允许:结尾分号(以及分号后仅跟空白/注释)。
fn has_multiple_statements(sql: &str) -> bool {
    let b = sql.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            q @ (b'\'' | b'"' | b'`') => i = skip_quoted(b, i, q),
            b'-' if b.get(i + 1) == Some(&b'-') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(b.len());
            }
            b';' => {
                // 分号后还有实际 SQL → 多语句;否则只是结尾分号
                return trailing_has_sql(b, i + 1);
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// 跳过引号包裹的内容(支持 `''` 转义),返回结束引号的位置。
fn skip_quoted(b: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 1;
    while i < b.len() {
        if b[i] == quote {
            if b.get(i + 1) == Some(&quote) {
                i += 2; // 转义引号
                continue;
            }
            break;
        }
        i += 1;
    }
    i
}

/// 从 `start` 起,若只存在空白/注释则返回 false(视为语句已结束)。
fn trailing_has_sql(b: &[u8], start: usize) -> bool {
    let mut i = start;
    loop {
        while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\r' | b'\n') {
            i += 1;
        }
        if i >= b.len() {
            return false;
        }
        match b[i] {
            b'-' if b.get(i + 1) == Some(&b'-') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(b.len());
            }
            _ => return true,
        }
    }
}

/// 跳过前导空白与注释(`--` 行注释、`/* */` 块注释),返回第一个"真实"语句字符下标。
///
/// 用于防止 `-- x\nATTACH ...` / `/* x */ VACUUM ...` 这类通过前导注释
/// 绕过 [`FORBIDDEN_PREFIXES`] 前缀黑名单的沙箱逃逸。
fn skip_leading_trivia(b: &[u8]) -> usize {
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'-' if b.get(i + 1) == Some(&b'-') => {
                // 行注释:跳到行尾(换行交由外层空白分支继续处理)
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                // 块注释:跳到 `*/`
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(b.len());
            }
            _ => break,
        }
    }
    i
}

fn check_statement(sql: &str) -> Result<()> {
    let lower = sql.trim_start().to_ascii_lowercase();
    if lower.contains("__sys") {
        return Err(CombeeError::Forbidden(
            "access to __sys_* internal tables is not allowed".into(),
        ));
    }
    // 前缀黑名单必须作用于"剥离前导注释后的首条语句",
    // 否则 `-- x\nATTACH ...` 会以 `--` 开头而绕过黑名单。
    let start = skip_leading_trivia(sql.as_bytes());
    let leading = sql[start..].trim_start().to_ascii_lowercase();
    for p in FORBIDDEN_PREFIXES {
        if leading.starts_with(p) {
            return Err(CombeeError::InvalidRequest(format!(
                "statement starting with '{p}' is not allowed; \
                 use the /transaction endpoint for transactions"
            )));
        }
    }
    if has_multiple_statements(sql) {
        return Err(CombeeError::InvalidRequest(
            "multiple statements in one request are not allowed".into(),
        ));
    }
    Ok(())
}

/// 执行单条 SQL。查询返回列 + 行;DML/DDL 返回受影响行数。
pub fn execute_sql(conn: &Connection, req: &SqlRequest) -> Result<SqlResult> {
    execute_sql_quota(conn, req, None)
}

/// 带配额执行(rows / result bytes 截断)。
pub fn execute_sql_quota(
    conn: &Connection,
    req: &SqlRequest,
    quota: Option<&combee_common::config::QuotaConfig>,
) -> Result<SqlResult> {
    check_statement(&req.sql)?;
    let values = to_sql_values(&req.params)?;
    let refs: Vec<&dyn rusqlite::ToSql> =
        values.iter().map(|v| v as &dyn rusqlite::ToSql).collect();

    let mut stmt = conn.prepare(&req.sql).map_err(sql_err)?;
    let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

    let max_rows = quota.map(|q| q.max_sql_rows).unwrap_or(0);
    let max_bytes = quota.map(|q| q.max_sql_result_bytes).unwrap_or(0);

    if columns.is_empty() {
        // DML / DDL:先释放 statement,再执行。
        drop(stmt);
        let affected = conn.execute(&req.sql, refs.as_slice()).map_err(sql_err)?;
        Ok(SqlResult {
            columns: vec![],
            rows: vec![],
            rows_affected: affected as u64,
            truncated: false,
        })
    } else {
        let mut rows_out = Vec::new();
        let mut total_bytes = 0usize;
        let mut truncated = false;
        {
            let mut rows_iter = stmt.query(refs.as_slice()).map_err(sql_err)?;
            while let Some(row) = rows_iter.next().map_err(sql_err)? {
                if max_rows > 0 && rows_out.len() >= max_rows {
                    truncated = true;
                    break;
                }
                let json_row = row_to_json(row, &columns)?;
                if max_bytes > 0 {
                    total_bytes += serde_json::to_vec(&json_row)
                        .map_err(|e| CombeeError::Sql(e.to_string()))?
                        .len();
                    if total_bytes > max_bytes {
                        truncated = true;
                        break;
                    }
                }
                rows_out.push(json_row);
            }
        }
        Ok(SqlResult {
            columns,
            rows: rows_out,
            rows_affected: 0,
            truncated,
        })
    }
}

/// 在单个 SQLite 事务中原子执行多条语句;任一失败则整体回滚。
pub fn execute_transaction(
    conn: &mut Connection,
    req: &TransactionRequest,
    quota: Option<&combee_common::config::QuotaConfig>,
) -> Result<Vec<SqlResult>> {
    if req.statements.is_empty() {
        return Err(CombeeError::InvalidRequest(
            "transaction requires at least one statement".into(),
        ));
    }
    let tx = conn.transaction().map_err(sql_err)?;
    let mut results = Vec::with_capacity(req.statements.len());
    for st in &req.statements {
        results.push(execute_sql_quota(&tx, st, quota)?);
    }
    tx.commit().map_err(sql_err)?;
    Ok(results)
}

fn to_sql_values(params: &[Json]) -> Result<Vec<SqlValue>> {
    params.iter().map(to_sql_value).collect()
}

fn to_sql_value(p: &Json) -> Result<SqlValue> {
    Ok(match p {
        Json::Null => SqlValue::Null,
        Json::Bool(b) => SqlValue::Integer(*b as i64),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                SqlValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                SqlValue::Real(f)
            } else {
                return Err(CombeeError::InvalidRequest(format!(
                    "unsupported number parameter: {n}"
                )));
            }
        }
        Json::String(s) => SqlValue::Text(s.clone()),
        other => {
            return Err(CombeeError::InvalidRequest(format!(
                "unsupported parameter type: {other}"
            )));
        }
    })
}

fn row_to_json(row: &Row, columns: &[String]) -> Result<Vec<Json>> {
    let mut out = Vec::with_capacity(columns.len());
    for i in 0..columns.len() {
        let v: SqlValue = row.get(i).map_err(sql_err)?;
        out.push(match v {
            SqlValue::Null => Json::Null,
            SqlValue::Integer(i) => Json::from(i),
            SqlValue::Real(f) => serde_json::Number::from_f64(f)
                .map(Json::Number)
                .unwrap_or(Json::Null),
            SqlValue::Text(s) => Json::String(s),
            SqlValue::Blob(b) => Json::String(String::from_utf8_lossy(&b).into_owned()),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage;
    use combee_common::config::KvDurability;
    use serde_json::json;

    /// 用真实 storage::open 构造测试连接。
    fn sql_conn() -> (Connection, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let conn = storage::open(&dir.path().join("sql.sqlite"), KvDurability::Normal).unwrap();
        (conn, dir)
    }

    #[test]
    fn check_statement_rejects_dangerous_sql() {
        // 内部表:各种大小写
        for bad in [
            "SELECT * FROM __sys_kv",
            "select * from __sys_kv",
            "INSERT INTO __SYS_META VALUES (1, 2)",
            "SELECT * FROM __sys_kv_expires_at",
        ] {
            assert!(
                matches!(check_statement(bad), Err(CombeeError::Forbidden(_))),
                "should reject {bad}"
            );
        }
        // 事务控制 / 附加库:前导空格、大小写
        for bad in [
            "BEGIN",
            "begin",
            "  BEGIN IMMEDIATE",
            "COMMIT",
            "ROLLBACK",
            "SAVEPOINT sp",
            "RELEASE sp",
            "ATTACH '/etc/passwd' AS evil",
            "DETACH evil",
            // 前导注释绕过(回归:必须剥离注释后再匹配前缀黑名单)
            "-- note\nATTACH '/etc/passwd' AS evil",
            "/* x */ ATTACH '/etc/passwd' AS evil",
            "  -- comment\nVACUUM INTO '/tmp/escape.sqlite'",
            "/* multi\nline */ BEGIN IMMEDIATE",
            "-- c\nSAVEPOINT s1",
            "/* c */ DETACH evil",
        ] {
            assert!(
                matches!(check_statement(bad), Err(CombeeError::InvalidRequest(_))),
                "should reject {bad}"
            );
        }
        // 多条语句:中间的 DROP 必须被拒绝
        for bad in [
            "SELECT 1; DROP TABLE t",
            "INSERT INTO t VALUES (1); INSERT INTO t VALUES (2)",
            "SELECT 1;\nDROP TABLE t",
            "SELECT 'a'; -- note\nDROP TABLE t",
        ] {
            assert!(
                matches!(check_statement(bad), Err(CombeeError::InvalidRequest(_))),
                "should reject multi-statement {bad:?}"
            );
        }
        // 正常语句放行,包括结尾分号与分号后注释
        for ok in [
            "SELECT 1",
            "CREATE TABLE users (id INTEGER)",
            "INSERT INTO users VALUES (1)",
            "SELECT 'BEGIN'",
            "PRAGMA user_version",
            "SELECT 1;",
            "SELECT 'a;b'",
            "SELECT \"a;b\"",
            "SELECT 1; -- trailing comment",
            "SELECT 1; /* trailing block */",
            // 合法语句前带注释仍应放行
            "-- leading comment\nSELECT 1",
            "/* leading block */ SELECT 1",
            "  \t-- comment\n/* block */ SELECT 1;",
        ] {
            assert!(check_statement(ok).is_ok(), "should allow {ok:?}");
        }
    }

    #[test]
    fn multi_statement_detector_edge_cases() {
        // 引号/注释中的分号不算多语句
        assert!(!has_multiple_statements("SELECT ';'"));
        assert!(!has_multiple_statements("SELECT 'it''s; fine'"));
        assert!(!has_multiple_statements("SELECT \"a;b\""));
        assert!(!has_multiple_statements("SELECT 1 -- ;"));
        assert!(!has_multiple_statements("SELECT 1 /* ; */"));
        assert!(!has_multiple_statements("SELECT 1;"));
        assert!(!has_multiple_statements("SELECT 1;  "));
        // 真正的多语句
        assert!(has_multiple_statements("SELECT 1; SELECT 2"));
        assert!(has_multiple_statements("SELECT ';' ; SELECT 2"));
        assert!(has_multiple_statements("SELECT 1; -- x\nSELECT 2"));
    }

    #[test]
    fn param_types_map_to_sqlite_values() {
        let (conn, _d) = sql_conn();
        conn.execute_batch("CREATE TABLE t (a, b, c, d, e)")
            .unwrap();
        let req = SqlRequest {
            sql: "INSERT INTO t VALUES (?, ?, ?, ?, ?)".to_string(),
            params: vec![
                json!(null),
                json!(true),
                json!(false),
                json!(3.5),
                json!("s"),
            ],
        };
        execute_sql(&conn, &req).unwrap();
        let (a, b, c, d, e): (Option<i64>, i64, i64, f64, String) = conn
            .query_row("SELECT a, b, c, d, e FROM t", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap();
        assert_eq!(a, None);
        assert_eq!(b, 1, "true -> 1");
        assert_eq!(c, 0, "false -> 0");
        assert_eq!(d, 3.5);
        assert_eq!(e, "s");
    }

    #[test]
    fn unsupported_param_types_rejected() {
        let (conn, _d) = sql_conn();
        for bad in [json!({"a": 1}), json!([1, 2])] {
            let req = SqlRequest {
                sql: "SELECT ?".to_string(),
                params: vec![bad],
            };
            assert!(matches!(
                execute_sql(&conn, &req),
                Err(CombeeError::InvalidRequest(_))
            ));
        }
    }

    #[test]
    fn parameter_count_mismatch_rejected() {
        let (conn, _d) = sql_conn();
        let req = SqlRequest {
            sql: "SELECT ?".to_string(),
            params: vec![],
        };
        assert!(
            execute_sql(&conn, &req).is_err(),
            "missing param must error"
        );
    }

    #[test]
    fn multi_statement_injection_rejected() {
        let (conn, _d) = sql_conn();
        conn.execute_batch("CREATE TABLE t (x)").unwrap();
        let req = SqlRequest {
            sql: "SELECT 1; DROP TABLE t".to_string(),
            params: vec![],
        };
        assert!(
            execute_sql(&conn, &req).is_err(),
            "multi-statement must be rejected"
        );
        // 表仍然存在,证明第二条语句未执行
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 't'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn select_returns_columns_and_rows() {
        let (conn, _d) = sql_conn();
        conn.execute_batch("CREATE TABLE t (x INTEGER, y TEXT)")
            .unwrap();
        conn.execute("INSERT INTO t VALUES (1, 'a')", []).unwrap();
        let req = SqlRequest {
            sql: "SELECT x, y FROM t".to_string(),
            params: vec![],
        };
        let r = execute_sql(&conn, &req).unwrap();
        assert_eq!(r.columns, vec!["x".to_string(), "y".to_string()]);
        assert_eq!(r.rows, vec![vec![json!(1), json!("a")]]);
        assert_eq!(r.rows_affected, 0);
    }

    #[test]
    fn transaction_commits_and_rolls_back() {
        let (mut conn, _d) = sql_conn();
        conn.execute_batch("CREATE TABLE t (x INTEGER)").unwrap();

        // 全部成功 → 提交
        let ok = TransactionRequest {
            statements: vec![
                SqlRequest {
                    sql: "INSERT INTO t VALUES (1)".into(),
                    params: vec![],
                },
                SqlRequest {
                    sql: "INSERT INTO t VALUES (2)".into(),
                    params: vec![],
                },
            ],
        };
        let results = execute_transaction(&mut conn, &ok, None).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].rows_affected, 1);

        // 任一失败 → 整体回滚
        let bad = TransactionRequest {
            statements: vec![
                SqlRequest {
                    sql: "INSERT INTO t VALUES (3)".into(),
                    params: vec![],
                },
                SqlRequest {
                    sql: "INSERT INTO nosuch_table VALUES (1)".into(),
                    params: vec![],
                },
            ],
        };
        assert!(execute_transaction(&mut conn, &bad, None).is_err());
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2, "failed transaction must roll back everything");
    }

    #[test]
    fn empty_transaction_rejected() {
        let (mut conn, _d) = sql_conn();
        let req = TransactionRequest { statements: vec![] };
        assert!(matches!(
            execute_transaction(&mut conn, &req, None),
            Err(CombeeError::InvalidRequest(_))
        ));
    }

    #[test]
    fn transaction_rejects_sys_table_access() {
        let (mut conn, _d) = sql_conn();
        let req = TransactionRequest {
            statements: vec![SqlRequest {
                sql: "SELECT * FROM __sys_kv".into(),
                params: vec![],
            }],
        };
        assert!(matches!(
            execute_transaction(&mut conn, &req, None),
            Err(CombeeError::Forbidden(_))
        ));
    }
}
