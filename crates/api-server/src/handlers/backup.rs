//! Backup / restore API:POST /v1/databases/:id/backup 与 /restore。
//!
//! 由 Data Node 侧执行(VACUUM INTO 快照 → 对象存储);API Server 仅路由。

use axum::Json;
use axum::extract::{Path, State};
use combee_common::DatabaseId;
use combee_common::rpc::BackupInfo;

use crate::AppState;
use crate::handlers::sql::require_db;

/// POST /v1/databases/{id}/backup —— 快照到对象存储。
pub async fn backup(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
) -> Result<Json<BackupInfo>, crate::ApiError> {
    require_db(&state, auth.tenant_id, id, auth.internal).await?;
    let client = state.data_node.client_for(id).await?;
    let info = client.backup(id).await?;
    Ok(Json(info))
}

/// POST /v1/databases/{id}/backup/incr —— WAL 增量备份(主库 + WAL 周期归档)。
pub async fn incremental_backup(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
) -> Result<Json<BackupInfo>, crate::ApiError> {
    require_db(&state, auth.tenant_id, id, auth.internal).await?;
    let client = state.data_node.client_for(id).await?;
    let info = client.incremental_backup(id).await?;
    Ok(Json(info))
}

/// POST /v1/databases/{id}/restore —— 从对象存储恢复(缺省最新快照)。
pub async fn restore(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
    Json(req): Json<RestoreRequest>,
) -> Result<axum::http::StatusCode, crate::ApiError> {
    require_db(&state, auth.tenant_id, id, auth.internal).await?;
    let client = state.data_node.client_for(id).await?;
    client.restore(id, req.version).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
pub struct RestoreRequest {
    /// 快照对象 key;缺省取最新。
    #[serde(default)]
    pub version: Option<String>,
}
