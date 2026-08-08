//! API Server 与 Data Node 之间的内部协议类型(HTTP JSON 序列化)。

use serde::{Deserialize, Serialize};

/// SQL 参数,与 `?` 位置占位符一一对应。
/// 允许 JSON 里的 `null` / number / string / bool,由 Data Node 转换为 SQLite 值。
pub type Param = serde_json::Value;

/// 单条 SQL 执行请求。
#[derive(utoipa::ToSchema, Debug, Clone, Serialize, Deserialize)]
pub struct SqlRequest {
    pub sql: String,
    #[serde(default)]
    pub params: Vec<Param>,
}

/// 单条 SQL 执行结果。
#[derive(utoipa::ToSchema, Debug, Clone, Serialize, Deserialize)]
pub struct SqlResult {
    /// 查询语句的列名;非查询语句为空数组。
    #[serde(default)]
    pub columns: Vec<String>,
    /// 查询结果行,每行与 `columns` 对齐;非查询语句为空数组。
    #[serde(default)]
    pub rows: Vec<Vec<serde_json::Value>>,
    /// 受影响行数(INSERT / UPDATE / DELETE 等)。
    pub rows_affected: u64,
}

/// 事务请求:多条语句在同一个 SQLite 事务中原子执行。
#[derive(utoipa::ToSchema, Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRequest {
    pub statements: Vec<SqlRequest>,
}

/// KV GET 结果。
#[derive(utoipa::ToSchema, Debug, Clone, Serialize, Deserialize)]
pub struct KvEntry {
    pub value: String,
    /// 剩余存活秒数;`None` 表示持久 key(无 TTL)。
    pub ttl_seconds: Option<i64>,
}

/// KV SET 请求体。
#[derive(utoipa::ToSchema, Debug, Clone, Serialize, Deserialize)]
pub struct KvSetRequest {
    pub value: String,
    /// 可选 TTL(秒);缺省为持久 key。
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
    /// 仅当 key 不存在时写入(SET NX)。
    #[serde(default)]
    pub nx: bool,
    /// 仅当 key 已存在时写入(SET XX)。
    #[serde(default)]
    pub xx: bool,
}

/// INCR / DECR 请求体。
#[derive(utoipa::ToSchema, Debug, Clone, Serialize, Deserialize)]
pub struct KvIncrRequest {
    pub key: String,
    #[serde(default = "default_delta")]
    pub delta: i64,
    /// 可选 TTL(秒);缺省保持 key 原有 TTL 不变。
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
}

/// EXPIRE / PERSIST 请求体。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvExpireRequest {
    pub key: String,
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
}

fn default_delta() -> i64 {
    1
}

/// 批量 KV 存在性检查(MGET 风格的批量版本)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvKeysRequest {
    pub keys: Vec<String>,
}

/// 批量 KV 读取结果,与请求中的 keys 一一对应。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvMultiGetResponse {
    pub values: Vec<Option<String>>,
}

/// 批量 KV 写入项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvSetItem {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
}

/// MSET 请求体。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvMultiSetRequest {
    pub items: Vec<KvSetItem>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn sql_request_defaults_to_empty_params() {
        let req: SqlRequest = serde_json::from_str(r#"{"sql": "SELECT 1"}"#).unwrap();
        assert_eq!(req.sql, "SELECT 1");
        assert!(req.params.is_empty());
    }

    #[test]
    fn sql_result_roundtrip() {
        let result = SqlResult {
            columns: vec!["a".into(), "b".into()],
            rows: vec![vec![json!(1), json!(null)]],
            rows_affected: 0,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["columns"], json!(["a", "b"]));
        assert_eq!(json["rows"], json!([[1, null]]));
        assert_eq!(json["rows_affected"], 0);
        let back: SqlResult = serde_json::from_value(json).unwrap();
        assert_eq!(back.columns, result.columns);
        assert_eq!(back.rows, result.rows);
    }

    #[test]
    fn kv_set_request_defaults() {
        // 只给 value,其余字段用默认值
        let req: KvSetRequest = serde_json::from_str(r#"{"value": "x"}"#).unwrap();
        assert_eq!(req.value, "x");
        assert_eq!(req.ttl_seconds, None);
        assert!(!req.nx);
        assert!(!req.xx);
        // 完整字段
        let req: KvSetRequest =
            serde_json::from_str(r#"{"value": "x", "ttl_seconds": 60, "nx": true}"#).unwrap();
        assert_eq!(req.ttl_seconds, Some(60));
        assert!(req.nx);
    }

    #[test]
    fn kv_incr_request_defaults_delta_to_one() {
        let req: KvIncrRequest = serde_json::from_str(r#"{"key": "c"}"#).unwrap();
        assert_eq!(req.key, "c");
        assert_eq!(req.delta, 1, "delta defaults to 1 (INCR)");
        assert_eq!(req.ttl_seconds, None);

        let req: KvIncrRequest =
            serde_json::from_str(r#"{"key": "c", "delta": -3, "ttl_seconds": 5}"#).unwrap();
        assert_eq!(req.delta, -3);
        assert_eq!(req.ttl_seconds, Some(5));
    }

    #[test]
    fn kv_expire_request_without_ttl_means_persist() {
        let req: KvExpireRequest = serde_json::from_str(r#"{"key": "k"}"#).unwrap();
        assert_eq!(req.key, "k");
        assert_eq!(req.ttl_seconds, None, "missing ttl = PERSIST");
    }

    #[test]
    fn kv_keys_request_rejects_missing_field() {
        assert!(serde_json::from_str::<KvKeysRequest>(r#"{}"#).is_err());
        let req: KvKeysRequest = serde_json::from_str(r#"{"keys": ["a"]}"#).unwrap();
        assert_eq!(req.keys, vec!["a".to_string()]);
    }

    #[test]
    fn kv_entry_serializes_ttl_null_as_absent_or_null() {
        let entry = KvEntry {
            value: "v".into(),
            ttl_seconds: None,
        };
        let v: Value = serde_json::to_value(entry.clone()).unwrap();
        assert_eq!(v["value"], "v");
        assert_eq!(v["ttl_seconds"], Value::Null);
        let back: KvEntry = serde_json::from_value(v).unwrap();
        assert_eq!(back.value, entry.value);
        assert_eq!(back.ttl_seconds, None);
    }
}
