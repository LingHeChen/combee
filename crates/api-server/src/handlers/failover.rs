//! 手动 failover 端点。

use axum::Json;
use axum::extract::{Path, State};
use combee_common::DatabaseId;
use combee_metadata::DatabaseRecord;

use crate::ApiError;
use crate::AppState;

/// POST /v1/databases/{id}/failover —— 手动触发 failover(把副本提升为主)。
pub async fn failover(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
) -> Result<Json<DatabaseRecord>, ApiError> {
    let record = crate::failover::failover_cell(&state, auth.tenant_id, id).await?;
    Ok(Json(record))
}
