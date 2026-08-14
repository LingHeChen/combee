//! 探活 / 就绪端点(可观测性 P0)。
//! - `/health`:进程存活(不做依赖检查);
//! - `/ready`:本实例能否服务真实流量(PostgreSQL 可达;注册表无健康节点时也返回 503)。

use axum::Json;
use axum::extract::State;
use combee_common::CombeeError;

use crate::AppState;

pub async fn ready(
    State(state): State<AppState>,
) -> Result<axum::Json<serde_json::Value>, (axum::http::StatusCode, axum::Json<serde_json::Value>)>
{
    if let Err(e) = state.metadata.ping().await {
        tracing::error!(event = "ready.failed", error = %e, "metadata postgres unreachable");
        return Err((
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(
                serde_json::json!({"status": "not_ready", "reason": "postgres_unavailable"}),
            ),
        ));
    }
    // 多节点模式下,无健康 DataNode 是"暂时性"状态(等待 data-node 注册)。
    // 不能返回 503 —— Swarm healthcheck 会把本实例判死 → SIGTERM → 重启,
    // 而 data-node 又依赖 api-server 存活才能注册,形成启动死锁。
    // 因此降级为 200 + degraded,等待 data-node 注册后自动恢复 ok。
    if state.nodes.shared() {
        let healthy = state.nodes.healthy().await;
        if healthy.is_empty() {
            tracing::warn!(
                event = "ready.degraded",
                reason = "no_data_node",
                "waiting for data-node registration"
            );
            return Ok(axum::Json(
                serde_json::json!({"status": "degraded", "reason": "no_data_node"}),
            ));
        }
    }
    Ok(axum::Json(serde_json::json!({"status": "ok"})))
}

// 占位避免未使用告警
#[allow(dead_code)]
fn _unused(_: CombeeError) {}

/// Prometheus 文本指标端点(观测计划 §14;不挂租户认证,供监控探针抓取)。
/// 渲染前刷新进程内已知的动态仪表(活跃 Cell 数等,低基数)。
pub async fn metrics(State(state): State<AppState>) -> axum::response::Response {
    // 活跃 Cell 数(逻辑数据库数):惰性查询 metadata,低频率(监控默认 15s 抓取)。
    match state.metadata.list_databases_all().await {
        Ok(dbs) => {
            combee_common::metrics::gauge_set(
                "combee_active_cells",
                &[("service", "api")],
                dbs.len() as i64,
            );
        }
        Err(e) => {
            tracing::warn!(event = "metrics.active_cells_failed", error = %e);
            combee_common::metrics::counter_inc(
                "combee_postgres_errors_total",
                &[("service", "api"), ("error_class", "list_databases_all")],
            );
        }
    }
    axum::response::Response::new(axum::body::Body::from(combee_common::metrics::render()))
}
