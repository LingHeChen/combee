//! 内部节点管理端点(Data Node agent 调用):register / heartbeat / unregister / list。

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use combee_common::rpc::{
    NodeHeartbeatRequest, NodeRegisterRequest, NodeRegisterResponse, NodeUnregisterRequest,
};

use crate::AppState;
use crate::nodes::NodeInfo;

/// POST /internal/nodes/register —— Data Node 启动注册,返回分配的节点 ID。
pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<NodeRegisterRequest>,
) -> Json<NodeRegisterResponse> {
    let id = state.nodes.register_with_id(req.id, req.addr, req.capacity);
    Json(NodeRegisterResponse { id })
}

/// POST /internal/nodes/heartbeat —— 周期心跳;未知节点返回 404。
pub async fn heartbeat(
    State(state): State<AppState>,
    Json(req): Json<NodeHeartbeatRequest>,
) -> Result<StatusCode, StatusCode> {
    if state.nodes.heartbeat(req.id, req.active_conns) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// POST /internal/nodes/unregister —— 退出注销;未知节点返回 404。
pub async fn unregister(
    State(state): State<AppState>,
    Json(req): Json<NodeUnregisterRequest>,
) -> Result<StatusCode, StatusCode> {
    if state.nodes.unregister(req.id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// GET /internal/nodes —— 节点状态(metrics)。
pub async fn list(State(state): State<AppState>) -> Json<Vec<NodeInfo>> {
    Json(state.nodes.list())
}

/// GET /internal/nodes/{node}/replicas —— 该节点作为副本负责的全部 Cell id。
pub async fn replicas(
    State(state): State<AppState>,
    Path(node): Path<combee_common::NodeId>,
) -> Result<Json<Vec<combee_common::DatabaseId>>, crate::ApiError> {
    let records = state.metadata.list_replicas_of(node).await?;
    Ok(Json(records.into_iter().map(|r| r.id).collect()))
}
