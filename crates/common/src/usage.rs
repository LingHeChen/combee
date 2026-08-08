//! Usage Metering 协议(设计文档 P0 / COMBEE_NEXT_PHASE_V0.1.0_BETA_PLAN.md §4)。
//!
//! 计量只描述"发生了多少使用量"(Metering),不参与计价(Rating,见 credits 阶段)。
//! 统计维度:(tenant, cell, metric, time_bucket)。时间桶第一版为 1 分钟。

use serde::{Deserialize, Serialize};

use crate::{DatabaseId, TenantId};

/// 计量指标。`as_str` / `parse` 用于跨进程(PostgreSQL / RPC)传输。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UsageMetric {
    /// KV 读操作(GET / EXISTS / MGET / TTL)。
    KvRead,
    /// KV 写操作(SET / DEL / MSET / EXPIRE / INCR)。
    KvWrite,
    /// SQL 读语句(SELECT / WITH / PRAGMA / EXPLAIN)。
    SqlRead,
    /// SQL 写语句(其余 DML / DDL)。
    SqlWrite,
    /// HTTP 请求数。
    Requests,
    /// 请求体字节数。
    BytesIn,
    /// 响应体字节数。
    BytesOut,
    /// 当前存储字节数(快照类指标,用 set 而非 add)。
    StorageBytes,
}

impl UsageMetric {
    pub fn as_str(&self) -> &'static str {
        match self {
            UsageMetric::KvRead => "kv_read",
            UsageMetric::KvWrite => "kv_write",
            UsageMetric::SqlRead => "sql_read",
            UsageMetric::SqlWrite => "sql_write",
            UsageMetric::Requests => "requests",
            UsageMetric::BytesIn => "bytes_in",
            UsageMetric::BytesOut => "bytes_out",
            UsageMetric::StorageBytes => "storage_bytes",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "kv_read" => UsageMetric::KvRead,
            "kv_write" => UsageMetric::KvWrite,
            "sql_read" => UsageMetric::SqlRead,
            "sql_write" => UsageMetric::SqlWrite,
            "requests" => UsageMetric::Requests,
            "bytes_in" => UsageMetric::BytesIn,
            "bytes_out" => UsageMetric::BytesOut,
            "storage_bytes" => UsageMetric::StorageBytes,
            _ => return None,
        })
    }
}

/// 一分钟时间桶起点(unix 秒)。
pub const BUCKET_SECS: i64 = 60;

/// 由 unix 秒计算所在分钟桶起点。
pub fn bucket_start(unix_secs: i64) -> i64 {
    unix_secs.div_euclid(BUCKET_SECS) * BUCKET_SECS
}

/// 聚合键。`cell` 为 `None` 表示租户级(与具体 Cell 无关的请求,如创建 Cell 本身)。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UsageKey {
    pub tenant_id: TenantId,
    pub cell_id: Option<DatabaseId>,
    pub metric: UsageMetric,
    pub bucket_start: i64,
}

/// 一条持久化的用量桶记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageBucket {
    pub tenant_id: TenantId,
    pub cell_id: Option<DatabaseId>,
    pub metric: UsageMetric,
    pub bucket_start: i64,
    pub value: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_roundtrip() {
        for m in [
            UsageMetric::KvRead,
            UsageMetric::KvWrite,
            UsageMetric::SqlRead,
            UsageMetric::SqlWrite,
            UsageMetric::Requests,
            UsageMetric::BytesIn,
            UsageMetric::BytesOut,
            UsageMetric::StorageBytes,
        ] {
            assert_eq!(UsageMetric::parse(m.as_str()), Some(m));
        }
        assert_eq!(UsageMetric::parse("nope"), None);
    }

    #[test]
    fn bucket_start_floors_to_minute() {
        assert_eq!(bucket_start(0), 0);
        assert_eq!(bucket_start(59), 0);
        assert_eq!(bucket_start(60), 60);
        assert_eq!(bucket_start(1_700_000_000), 1_699_999_980);
    }
}
