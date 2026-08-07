//! SQL 执行:单条执行 + 事务批量执行。

use axum::Json;
use axum::extract::{Path, State};
use combee_common::DatabaseId;
use combee_common::protocol::{SqlRequest, SqlResult, TransactionRequest};

use crate::{ApiError, AppState};

pub(crate) async fn require_db(
    state: &AppState,
    tenant: combee_common::TenantId,
    id: DatabaseId,
) -> Result<combee_metadata::DatabaseRecord, ApiError> {
    state
        .metadata
        .get_database(tenant, id)
        .await
        .map_err(ApiError::from)
}

/// POST /v1/databases/{id}/sql —— 执行单条 SQL。
pub async fn execute_sql(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
    Json(req): Json<SqlRequest>,
) -> Result<Json<SqlResult>, ApiError> {
    let record = require_db(&state, auth.tenant_id, id).await?;
    let client = state.data_node.client_for(id).await?;
    let result = client.execute_sql(id, req, record.generation).await?;
    Ok(Json(result))
}

/// POST /v1/databases/{id}/transaction —— 多条语句在同一个 SQLite 事务中原子执行。
pub async fn execute_transaction(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
    Json(req): Json<TransactionRequest>,
) -> Result<Json<Vec<SqlResult>>, ApiError> {
    let record = require_db(&state, auth.tenant_id, id).await?;
    let client = state.data_node.client_for(id).await?;
    let results = client
        .execute_transaction(id, req, record.generation)
        .await?;
    Ok(Json(results))
}
