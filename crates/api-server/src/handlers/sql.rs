//! SQL 执行:单条执行 + 事务批量执行。

use axum::Json;
use axum::extract::{Path, State};
use combee_common::DatabaseId;
use combee_common::protocol::{SqlRequest, SqlResult, TransactionRequest};
use combee_common::usage::UsageMetric;

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
/// 执行单条 SQL。
#[utoipa::path(
    post,
    path = "/v1/databases/{id}/sql",
    params(("id" = DatabaseId, Path, description = "Cell id")),
    request_body = SqlRequest,
    responses((status = 200, description = "SQL result", body = SqlResult)),
    tag = "sql"
)]
pub async fn execute_sql(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
    Json(req): Json<SqlRequest>,
) -> Result<Json<SqlResult>, ApiError> {
    let record = require_db(&state, auth.tenant_id, id).await?;
    let client = state.data_node.client_for(id).await?;
    let metric = if crate::usage::is_read_sql(&req.sql) {
        UsageMetric::SqlRead
    } else {
        UsageMetric::SqlWrite
    };
    state.usage.record(auth.tenant_id, Some(id), metric, 1);
    let result = client.execute_sql(id, req, record.generation).await?;
    Ok(Json(result))
}

/// POST /v1/databases/{id}/transaction —— 多条语句在同一个 SQLite 事务中原子执行。
/// 多条 SQL 原子执行。
#[utoipa::path(
    post,
    path = "/v1/databases/{id}/transaction",
    params(("id" = DatabaseId, Path, description = "Cell id")),
    request_body = TransactionRequest,
    responses((status = 200, description = "results", body = Vec<SqlResult>)),
    tag = "sql"
)]
pub async fn execute_transaction(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
    Json(req): Json<TransactionRequest>,
) -> Result<Json<Vec<SqlResult>>, ApiError> {
    let record = require_db(&state, auth.tenant_id, id).await?;
    let client = state.data_node.client_for(id).await?;
    for stmt in &req.statements {
        let metric = if crate::usage::is_read_sql(&stmt.sql) {
            UsageMetric::SqlRead
        } else {
            UsageMetric::SqlWrite
        };
        state.usage.record(auth.tenant_id, Some(id), metric, 1);
    }
    let results = client
        .execute_transaction(id, req, record.generation)
        .await?;
    Ok(Json(results))
}
