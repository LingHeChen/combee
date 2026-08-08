//! Database lifecycle:CREATE / LIST / DELETE。

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use combee_common::DatabaseId;
use combee_metadata::DatabaseRecord;
use serde::{Deserialize, Serialize};

use crate::{ApiError, AppState};

#[derive(utoipa::ToSchema, Serialize, Deserialize)]
pub struct CreateDatabaseResponse {
    pub id: DatabaseId,
}

/// POST /v1/databases —— 只写目录记录,磁盘文件等首次访问再创建(lazy create)。
/// 创建 Cell(懒创建,零 IO;支持 Idempotency-Key)。
#[utoipa::path(
    post,
    path = "/v1/databases",
    responses((status = 201, description = "created", body = CreateDatabaseResponse)),
    tag = "databases"
)]
pub async fn create_database(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    headers: axum::http::HeaderMap,
) -> Result<(StatusCode, Json<CreateDatabaseResponse>), ApiError> {
    // Idempotency-Key:同 key 重试返回首次创建的 Cell(幂等;并发同 key 只落库一次)
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let id = DatabaseId::new();
    if let Some(key) = &idempotency_key {
        let payload = serde_json::json!({ "id": id }).to_string();
        if let Some(existing) = state
            .metadata
            .save_idempotency(key, auth.tenant_id, payload)
            .await?
        {
            // 已存在:重放首次响应(幂等)
            let cached: CreateDatabaseResponse = serde_json::from_str(&existing).map_err(|e| {
                ApiError(combee_common::CombeeError::Internal(format!(
                    "idempotency payload corrupt: {e}"
                )))
            })?;
            return Ok((StatusCode::OK, Json(cached)));
        }
    }

    // placement:从健康 Data Node 中轮询选择(无注册节点时 storage_node_id=None,单机模式)
    let storage_node = state.nodes.pick().map(|(n, _)| n);
    state
        .metadata
        .create_database(auth.tenant_id, id, storage_node)
        .await?;
    Ok((StatusCode::CREATED, Json(CreateDatabaseResponse { id })))
}

/// GET /v1/databases —— 列出当前租户全部 Cell。
/// 列出当前租户全部 Cell。
#[utoipa::path(
    get,
    path = "/v1/databases",
    responses((status = 200, description = "cells", body = Vec<DatabaseRecord>)),
    tag = "databases"
)]
pub async fn list_databases(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
) -> Result<Json<Vec<DatabaseRecord>>, ApiError> {
    let records = state.metadata.list_databases(auth.tenant_id).await?;
    Ok(Json(records))
}

/// DELETE /v1/databases/{id} —— 回收连接、删除磁盘文件、移除目录记录。
/// 删除 Cell(删除磁盘文件与目录记录)。
#[utoipa::path(
    delete,
    path = "/v1/databases/{id}",
    params(("id" = DatabaseId, Path, description = "Cell id")),
    responses((status = 204, description = "deleted")),
    tag = "databases"
)]
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
