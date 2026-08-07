//! Database lifecycle:CREATE / LIST / DELETE。

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use combee_common::DatabaseId;
use combee_metadata::DatabaseRecord;
use serde::Serialize;

use crate::{ApiError, AppState};

#[derive(Serialize)]
pub struct CreateDatabaseResponse {
    pub id: DatabaseId,
}

/// POST /v1/databases —— 只写目录记录,磁盘文件等首次访问再创建(lazy create)。
pub async fn create_database(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
) -> Result<(StatusCode, Json<CreateDatabaseResponse>), ApiError> {
    let id = DatabaseId::new();
    // placement:从健康 Data Node 中轮询选择(无注册节点时 storage_node_id=None,单机模式)
    let storage_node = state.nodes.pick().map(|(n, _)| n);
    state
        .metadata
        .create_database(auth.tenant_id, id, storage_node)
        .await?;
    Ok((StatusCode::CREATED, Json(CreateDatabaseResponse { id })))
}

/// GET /v1/databases —— 列出当前租户全部 Cell。
pub async fn list_databases(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
) -> Result<Json<Vec<DatabaseRecord>>, ApiError> {
    let records = state.metadata.list_databases(auth.tenant_id).await?;
    Ok(Json(records))
}

/// DELETE /v1/databases/{id} —— 回收连接、删除磁盘文件、移除目录记录。
pub async fn delete_database(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
) -> Result<StatusCode, ApiError> {
    state.metadata.get_database(auth.tenant_id, id).await?;
    let client = state.data_node.client_for(id).await?;
    client.delete_database(id).await?;
    state.metadata.delete_database(auth.tenant_id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
