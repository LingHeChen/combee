//! Replication API:POST /v1/databases/{id}/replication(设置/取消单 replica)。
//!
//! 复制通道复用 WAL 增量归档:副本 Data Node 周期从对象存储拉取主节点的
//! "主库 + WAL"并应用本地(见 `data-node` 的 replica 任务)。

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use combee_common::DatabaseId;
use combee_common::NodeId;
use combee_metadata::DatabaseRecord;
use serde::{Deserialize, Serialize};

use crate::ApiError;
use crate::AppState;
use crate::handlers::sql::require_db;

#[derive(Debug, Deserialize)]
pub struct SetReplicaRequest {
    /// 副本 Data Node 的 NodeId。
    pub replica_node: NodeId,
}

#[derive(Debug, Serialize)]
pub struct ReplicationStatus {
    pub db: DatabaseId,
    pub replica_node: Option<NodeId>,
}

/// POST /v1/databases/{id}/replication —— 为该 Cell 设置副本节点。
pub async fn set_replica(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
    Json(req): Json<SetReplicaRequest>,
) -> Result<Json<ReplicationStatus>, ApiError> {
    require_db(&state, auth.tenant_id, id, auth.internal).await?;
    let record = state
        .metadata
        .set_replica_node(auth.tenant_id, id, Some(req.replica_node))
        .await?;
    Ok(Json(ReplicationStatus {
        db: record.id,
        replica_node: record.replica_node_id,
    }))
}

/// DELETE /v1/databases/{id}/replication —— 取消副本。
pub async fn unset_replica(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
) -> Result<StatusCode, ApiError> {
    require_db(&state, auth.tenant_id, id, auth.internal).await?;
    state
        .metadata
        .set_replica_node(auth.tenant_id, id, None)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /v1/databases/{id}/replication —— 查询副本状态。
pub async fn get_replica(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
) -> Result<Json<ReplicationStatus>, ApiError> {
    let record = state.metadata.get_database(auth.tenant_id, id).await?;
    Ok(Json(ReplicationStatus {
        db: record.id,
        replica_node: record.replica_node_id,
    }))
}

/// 内部端点使用的完整记录查询(供 /internal/nodes/{id}/replicas)。
pub async fn replica_records(
    metadata: &std::sync::Arc<dyn combee_metadata::MetadataStore>,
    node: NodeId,
) -> Result<Vec<DatabaseRecord>, ApiError> {
    metadata
        .list_replicas_of(node)
        .await
        .map_err(ApiError::from)
}
