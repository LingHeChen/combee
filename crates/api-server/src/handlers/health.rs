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
    // 多节点模式下要求至少一个健康 DataNode(单机/未注册节点模式跳过)
    if state.nodes.shared() {
        let healthy = state.nodes.healthy().await;
        if healthy.is_empty() {
            return Err((
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(serde_json::json!({"status": "not_ready", "reason": "no_data_node"})),
            ));
        }
    }
    Ok(axum::Json(serde_json::json!({"status": "ok"})))
}

// 占位避免未使用告警
#[allow(dead_code)]
fn _unused(_: CombeeError) {}
