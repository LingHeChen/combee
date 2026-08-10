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
    pub name: String,
}

/// POST body:可选 name(提供时严格:重名 409;不提供生成 `cell-<short-id>`)。
#[derive(serde::Deserialize)]
pub struct CreateDatabaseRequest {
    pub name: Option<String>,
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
    body: Option<Json<CreateDatabaseRequest>>,
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
            // 已存在:重放首次响应(幂等)——payload 存首次 id,查真实记录拿 name
            let cached_id: DatabaseId = serde_json::from_str::<serde_json::Value>(&existing)
                .ok()
                .and_then(|v| v["id"].as_str().map(|s| s.to_string()))
                .ok_or_else(|| {
                    ApiError(combee_common::CombeeError::Internal(
                        "idempotency payload missing id".into(),
                    ))
                })?
                .parse()
                .map_err(|e| {
                    ApiError(combee_common::CombeeError::Internal(format!(
                        "idempotency payload corrupt: {e}"
                    )))
                })?;
            let rec = state
                .metadata
                .get_database(auth.tenant_id, cached_id)
                .await?;
            return Ok((
                StatusCode::OK,
                Json(CreateDatabaseResponse {
                    id: rec.id,
                    name: rec.name,
                }),
            ));
        }
    }

    // 配额:每租户最大 Cell 数
    if state.quota.max_cells_per_tenant > 0 {
        let count = state.metadata.list_databases(auth.tenant_id).await?.len();
        if count >= state.quota.max_cells_per_tenant {
            return Err(ApiError(combee_common::CombeeError::QuotaExceeded(
                format!(
                    "cell limit reached: {} (max {})",
                    count, state.quota.max_cells_per_tenant
                ),
            )));
        }
    }

    // placement:从健康 Data Node 中轮询选择(无注册节点时 storage_node_id=None,单机模式)
    let storage_node = state.nodes.pick().await.map(|(n, _)| n);
    let name = body.as_ref().and_then(|b| b.0.name.as_deref());
    if let Some(n) = name {
        validate_cell_name(n)?;
    }
    let record = state
        .metadata
        .create_database(auth.tenant_id, id, storage_node, name)
        .await?;
    // 生命周期:磁盘初始化(created → active)。失败时回滚目录记录并返回错误,
    // 避免出现"目录存在但磁盘从未落盘"的悬空 Cell。
    let ensure = match state.data_node.client_for(id).await {
        Ok(client) => client.ensure_database(id).await,
        Err(e) => Err(e),
    };
    if let Err(e) = ensure {
        tracing::error!(%id, "ensure_database failed, rolling back cell record: {e}");
        let _ = state.metadata.delete_database(auth.tenant_id, id).await;
        return Err(ApiError(e));
    }
    if let Err(e) = state
        .metadata
        .set_database_state(auth.tenant_id, id, combee_metadata::DatabaseState::Active)
        .await
    {
        tracing::warn!(%id, "failed to mark cell active: {e}");
    }
    Ok((
        StatusCode::CREATED,
        Json(CreateDatabaseResponse {
            id,
            name: record.name,
        }),
    ))
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
    // 生命周期:先置 deleting(半删除可见状态),再删磁盘文件,最后删目录记录。
    state
        .metadata
        .set_database_state(auth.tenant_id, id, combee_metadata::DatabaseState::Deleting)
        .await?;
    let client = state.data_node.client_for(id).await?;
    client.delete_database(id).await?;
    state.metadata.delete_database(auth.tenant_id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 校验 Cell name:`^[a-z0-9][a-z0-9-_]{0,62}$`(<=63 字符,小写字母/数字/-/_ 开头为字母或数字)。
pub(crate) fn validate_cell_name(name: &str) -> Result<(), ApiError> {
    let n = name.trim();
    let valid = !n.is_empty()
        && n.len() <= 63
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && (n.chars().next().unwrap().is_ascii_lowercase()
            || n.chars().next().unwrap().is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(ApiError(combee_common::CombeeError::InvalidCellName(
            name.to_string(),
        )))
    }
}

/// PUT /v1/databases/by-name/{name} —— 幂等 ensure:不存在则创建,存在则复用。
pub async fn ensure_database(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(name): Path<String>,
) -> Result<(StatusCode, Json<EnsureDatabaseResponse>), ApiError> {
    validate_cell_name(&name)?;
    if state.quota.max_cells_per_tenant > 0 {
        let count = state.metadata.list_databases(auth.tenant_id).await?.len();
        if count >= state.quota.max_cells_per_tenant {
            return Err(ApiError(combee_common::CombeeError::QuotaExceeded(
                format!(
                    "cell limit reached: {} (max {})",
                    count, state.quota.max_cells_per_tenant
                ),
            )));
        }
    }
    // placement:与 POST /v1/databases 一致,从健康节点轮询(无注册节点时 None,单机模式)
    let storage_node = state.nodes.pick().await.map(|(n, _)| n);
    let (record, created) = state
        .metadata
        .ensure_database_by_name(auth.tenant_id, &name, storage_node)
        .await?;
    if created {
        // 生命周期:新建 Cell 立即初始化磁盘并置 active(与 POST /v1/databases 一致)
        let id = record.id;
        let ensure = match state.data_node.client_for(id).await {
            Ok(client) => client.ensure_database(id).await,
            Err(e) => Err(e),
        };
        if let Err(e) = ensure {
            tracing::error!(%id, "ensure_database failed, rolling back cell record: {e}");
            let _ = state.metadata.delete_database(auth.tenant_id, id).await;
            return Err(ApiError(e));
        }
        if let Err(e) = state
            .metadata
            .set_database_state(auth.tenant_id, id, combee_metadata::DatabaseState::Active)
            .await
        {
            tracing::warn!(%id, "failed to mark cell active: {e}");
        }
    }
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((
        status,
        Json(EnsureDatabaseResponse {
            cell: record,
            created,
        }),
    ))
}

#[derive(Serialize)]
pub struct EnsureDatabaseResponse {
    pub cell: DatabaseRecord,
    pub created: bool,
}

/// GET /v1/databases/by-name/{name} —— 按名查询。
pub async fn get_database_by_name(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(name): Path<String>,
) -> Result<Json<DatabaseRecord>, ApiError> {
    let record = state
        .metadata
        .get_database_by_name(auth.tenant_id, &name)
        .await?;
    Ok(Json(record))
}

/// PATCH /v1/databases/{id} —— 重命名(租户内唯一,冲突 409)。
pub async fn rename_database(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
    Json(body): Json<RenameRequest>,
) -> Result<Json<DatabaseRecord>, ApiError> {
    validate_cell_name(&body.name)?;
    let record = state
        .metadata
        .rename_database(auth.tenant_id, id, &body.name)
        .await?;
    Ok(Json(record))
}

#[derive(Deserialize)]
pub struct RenameRequest {
    pub name: String,
}

/// POST /v1/databases/{id}/reset —— 重置:保留 id/name,generation+1,清空数据。
pub async fn reset_database(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
) -> Result<Json<DatabaseRecord>, ApiError> {
    // 先校验归属
    crate::handlers::sql::require_db(&state, auth.tenant_id, id).await?;
    // 清数据面文件
    let client = state.data_node.client_for(id).await?;
    client
        .reset_database(id)
        .await
        .map_err(|e| ApiError(combee_common::CombeeError::CellResetFailed(e.to_string())))?;
    let record = state.metadata.reset_database(auth.tenant_id, id).await?;
    Ok(Json(record))
}

/// DELETE /v1/databases/by-name/{name} —— 按名删除(删除后 ensure 同名会新建)。
pub async fn delete_database_by_name(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    let record = state
        .metadata
        .get_database_by_name(auth.tenant_id, &name)
        .await?;
    let client = state.data_node.client_for(record.id).await?;
    client.delete_database(record.id).await?;
    state
        .metadata
        .delete_database(auth.tenant_id, record.id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
