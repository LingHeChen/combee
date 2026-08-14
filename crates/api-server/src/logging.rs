//! 结构化请求日志中间件(Logging P0):
//! - 普通请求:DEBUG 级 access 日志(operation/status/latency_ms/tenant/cell/request_id);
//! - 事件化:ERROR(5xx)/WARN(auth.failed、rate_limit.exceeded、quota.exceeded)。
//!
//! 原则:hot KV GET 等正常请求不产生 INFO 日志,避免日志打爆磁盘。
//!

use std::time::Instant;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use combee_common::{AuthContext, DatabaseId};
use tracing::{debug, warn};

use crate::AppState;

fn parse_cell_from_path(path: &str) -> Option<DatabaseId> {
    let mut parts = path.split('/').filter(|s| !s.is_empty());
    if parts.next()? != "v1" || parts.next()? != "databases" {
        return None;
    }
    let id = parts.next()?;
    id.parse::<DatabaseId>().ok()
}

/// 结构化请求日志(挂在 auth 之后;access 为 DEBUG,异常事件为 WARN/ERROR)。
pub async fn request_logging(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let started = Instant::now();
    let request_id = crate::REQUEST_ID
        .try_with(|id| id.clone())
        .unwrap_or_default();
    let tenant = req
        .extensions()
        .get::<AuthContext>()
        .map(|a| a.tenant_id)
        .unwrap_or(combee_metadata::DEFAULT_TENANT);
    let cell = parse_cell_from_path(req.uri().path());
    let operation = operation_name(req.uri().path());
    let method = req.method().as_str().to_string();
    let response = next.run(req).await;
    let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
    let status_code = response.status().as_u16();

    // ---- 指标(观测计划 §14:请求量 / 错误 / 延迟直方图)----
    let status_class = match status_code {
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        _ => "5xx",
    };
    let labels = [
        ("service", "api"),
        ("op", operation.as_str()),
        ("status_class", status_class),
    ];
    combee_common::metrics::counter_inc("combee_http_requests_total", &labels);
    combee_common::metrics::histogram_observe(
        "combee_request_duration_seconds",
        &[("service", "api"), ("op", operation.as_str())],
        latency_ms / 1000.0,
    );
    if status_code >= 500 {
        combee_common::metrics::counter_inc("combee_http_errors_total", &labels);
    }

    if latency_ms >= 500.0 && status_code < 500 {
        // 慢请求:WARN,供告警查询 request.slow;不打断上面的错误分类逻辑。
        warn!(
            service = "combee-api",
            event = "request.slow",
            %request_id,
            tenant_id = %tenant.0,
            cell_id = %cell.map(|c| c.0.to_string()).unwrap_or_default(),
            %operation,
            %method,
            status = status_code,
            latency_ms = format!("{latency_ms:.2}"),
            "request.slow"
        );
    }

    if status_code >= 500 {
        tracing::error!(
            service = "combee-api",
            %request_id,
            tenant_id = %tenant.0,
            cell_id = %cell.map(|c| c.0.to_string()).unwrap_or_default(),
            %operation,
            %method,
            status = status_code,
            latency_ms = format!("{latency_ms:.2}"),
            "request.failed"
        );
    } else if status_code == 429 {
        // 区分 rate_limit(并发)与 quota(容量)
        warn!(
            service = "combee-api",
            %request_id,
            tenant_id = %tenant.0,
            cell_id = %cell.map(|c| c.0.to_string()).unwrap_or_default(),
            %operation,
            status = status_code,
            latency_ms = format!("{latency_ms:.2}"),
            "quota.exceeded"
        );
    } else if status_code == 401 {
        warn!(
            service = "combee-api",
            %request_id,
            tenant_id = %tenant.0,
            %operation,
            status = status_code,
            latency_ms = format!("{latency_ms:.2}"),
            "auth.failed"
        );
    } else {
        // 普通请求:DEBUG(默认不输出;RUST_LOG=debug 时可见)
        debug!(
            service = "combee-api",
            %request_id,
            tenant_id = %tenant.0,
            cell_id = %cell.map(|c| c.0.to_string()).unwrap_or_default(),
            %operation,
            %method,
            status = status_code,
            latency_ms = format!("{latency_ms:.2}"),
            "request.completed"
        );
    }
    let _ = &state;
    response
}

fn operation_name(path: &str) -> String {
    // /v1/databases/{id}/sql → databases.{id}.sql;聚合端点为资源名
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut out = Vec::new();
    for p in parts {
        if p.parse::<uuid::Uuid>().is_ok() {
            out.push("{id}".to_string());
        } else {
            out.push(p.to_string());
        }
    }
    out.join(".")
}
