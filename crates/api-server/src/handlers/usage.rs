//! Usage API(设计文档 P0 §4.5)。
//!
//! - `GET /v1/usage/summary?from&to` —— 租户整体用量汇总;
//! - `GET /v1/usage/timeseries?metric&interval&from&to` —— 时序(合并到 minute/hour/day);
//! - `GET /v1/cells/{id}/usage?from&to` —— 单 Cell 用量 + 当前存储字节。
//!
//! 所有查询以 `AuthContext.tenant_id` 为界(隔离在 repository 层)。

use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use combee_common::usage::{UsageBucket, UsageMetric, bucket_start};
use combee_common::{AuthContext, DatabaseId};
use serde::{Deserialize, Serialize};

use crate::{ApiError, AppState};

#[derive(Debug, Deserialize)]
pub struct UsageQuery {
    /// ISO8601(RFC3339),缺省为最近 24 小时。
    pub from: Option<String>,
    pub to: Option<String>,
    /// timeseries 专用:metrics 之一(kv_read/kv_write/sql_read/sql_write/requests/bytes_in/bytes_out/storage_bytes)。
    pub metric: Option<String>,
    /// timeseries 专用:minute(默认)/hour/day。
    pub interval: Option<String>,
}

#[derive(utoipa::ToSchema, Debug, Serialize)]
pub struct UsagePeriod {
    pub from: String,
    pub to: String,
}

#[derive(utoipa::ToSchema, Debug, Default, Serialize)]
pub struct UsageOperations {
    pub kv_reads: u64,
    pub kv_writes: u64,
    pub sql_reads: u64,
    pub sql_writes: u64,
}

#[derive(utoipa::ToSchema, Debug, Serialize)]
pub struct UsageSummary {
    pub period: UsagePeriod,
    pub operations: UsageOperations,
    pub request_count: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    /// 当前存储字节(仅单 Cell 查询时精确统计;租户汇总为各 Cell 之和)。
    pub current_storage_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct TimeseriesPoint {
    pub bucket_start: String,
    pub value: u64,
}

fn parse_time(s: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc).timestamp())
}

/// 解析 from/to;缺省最近 24h。返回 (from, to) unix 秒。
fn resolve_range(q: &UsageQuery) -> (i64, i64) {
    let now = Utc::now().timestamp();
    let from = q
        .from
        .as_deref()
        .and_then(parse_time)
        .unwrap_or(now - 86_400);
    let to = q.to.as_deref().and_then(parse_time).unwrap_or(now);
    (from, to)
}

fn summarize(buckets: &[UsageBucket], from: i64, to: i64) -> UsageSummary {
    let mut ops = UsageOperations::default();
    let mut requests = 0u64;
    let mut bytes_in = 0u64;
    let mut bytes_out = 0u64;
    for b in buckets {
        match b.metric {
            UsageMetric::KvRead => ops.kv_reads += b.value,
            UsageMetric::KvWrite => ops.kv_writes += b.value,
            UsageMetric::SqlRead => ops.sql_reads += b.value,
            UsageMetric::SqlWrite => ops.sql_writes += b.value,
            UsageMetric::Requests => requests += b.value,
            UsageMetric::BytesIn => bytes_in += b.value,
            UsageMetric::BytesOut => bytes_out += b.value,
            UsageMetric::StorageBytes => {}
        }
    }
    UsageSummary {
        period: UsagePeriod {
            from: DateTime::from_timestamp(from, 0)
                .map(|d| d.to_rfc3339())
                .unwrap_or_default(),
            to: DateTime::from_timestamp(to, 0)
                .map(|d| d.to_rfc3339())
                .unwrap_or_default(),
        },
        operations: ops,
        request_count: requests,
        bytes_in,
        bytes_out,
        current_storage_bytes: 0,
    }
}

fn fmt_bucket(bucket: i64) -> String {
    DateTime::from_timestamp(bucket, 0)
        .map(|d| d.to_rfc3339())
        .unwrap_or_default()
}

/// GET /v1/usage/summary —— 租户整体汇总(跨所有 Cell)。
/// 租户用量汇总。
#[utoipa::path(
    get,
    path = "/v1/usage/summary",
    responses((status = 200, description = "usage summary", body = UsageSummary)),
    tag = "usage"
)]
pub async fn usage_summary(
    State(state): State<AppState>,
    auth: AuthContext,
    Query(q): Query<UsageQuery>,
) -> Result<Json<UsageSummary>, ApiError> {
    let (from, to) = resolve_range(&q);
    let buckets = state
        .usage
        .query(
            auth.tenant_id,
            None,
            None,
            bucket_start(from),
            bucket_start(to),
        )
        .await?;
    let mut summary = summarize(&buckets, from, to);

    // 存储字节:汇总各 Cell 当前磁盘占用
    let cells = state.metadata.list_databases(auth.tenant_id).await?;
    for cell in cells {
        if let Ok(client) = state.data_node.client_for(cell.id).await
            && let Ok(b) = client.storage_bytes(cell.id).await
        {
            summary.current_storage_bytes += b;
        }
    }
    Ok(Json(summary))
}

/// GET /v1/usage/timeseries —— 按 metric + interval 聚合的时序。
pub async fn usage_timeseries(
    State(state): State<AppState>,
    auth: AuthContext,
    Query(q): Query<UsageQuery>,
) -> Result<Json<Vec<TimeseriesPoint>>, ApiError> {
    let metric = q
        .metric
        .as_deref()
        .and_then(UsageMetric::parse)
        .ok_or_else(|| {
            ApiError(combee_common::CombeeError::InvalidRequest(
                "invalid or missing metric".into(),
            ))
        })?;
    let (from, to) = resolve_range(&q);
    let buckets = state
        .usage
        .query(
            auth.tenant_id,
            None,
            Some(metric),
            bucket_start(from),
            bucket_start(to),
        )
        .await?;

    let interval_secs = match q.interval.as_deref().unwrap_or("minute") {
        "minute" => 60,
        "hour" => 3600,
        "day" => 86_400,
        other => {
            return Err(ApiError(combee_common::CombeeError::InvalidRequest(
                format!("invalid interval: {other}"),
            )));
        }
    };

    // 按 interval 桶合并
    let mut map: std::collections::BTreeMap<i64, u64> = std::collections::BTreeMap::new();
    for b in &buckets {
        let bucket = (b.bucket_start / interval_secs) * interval_secs;
        *map.entry(bucket).or_insert(0) += b.value;
    }
    Ok(Json(
        map.into_iter()
            .map(|(ts, value)| TimeseriesPoint {
                bucket_start: fmt_bucket(ts),
                value,
            })
            .collect(),
    ))
}

/// GET /v1/cells/{id}/usage —— 单 Cell 汇总 + 当前存储字节。
/// 单 Cell 用量 + 当前存储。
#[utoipa::path(
    get,
    path = "/v1/cells/{id}/usage",
    params(("id" = DatabaseId, Path, description = "Cell id")),
    responses((status = 200, description = "cell usage", body = UsageSummary)),
    tag = "usage"
)]
pub async fn cell_usage(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<DatabaseId>,
    Query(q): Query<UsageQuery>,
) -> Result<Json<UsageSummary>, ApiError> {
    let _record = state.metadata.get_database(auth.tenant_id, id).await?;
    let (from, to) = resolve_range(&q);
    let buckets = state
        .usage
        .query(
            auth.tenant_id,
            Some(id),
            None,
            bucket_start(from),
            bucket_start(to),
        )
        .await?;
    let mut summary = summarize(&buckets, from, to);

    if let Ok(client) = state.data_node.client_for(id).await
        && let Ok(b) = client.storage_bytes(id).await
    {
        summary.current_storage_bytes = b;
    }
    Ok(Json(summary))
}
