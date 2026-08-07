//! API key 管理端点(租户级)。
//!
//! - `POST /v1/api-keys`:创建 key,明文**只返回一次**(数据库只存哈希);
//! - `GET /v1/api-keys`:列出本租户的 key(不含明文);
//! - `DELETE /v1/api-keys/{id}`:撤销。
//! - `POST /v1/tenants`:创建租户(dev/管理;key 模式下需已有合法 key)。

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use combee_common::AuthContext;
use combee_common::api_key;
use combee_metadata::ApiKeyRecord;
use serde::Serialize;
use uuid::Uuid;

use crate::ApiError;
use crate::AppState;

#[derive(Serialize)]
pub struct CreateApiKeyResponse {
    /// 明文密钥,仅此一次返回(后续只能通过哈希校验)。
    pub key: String,
    pub record: ApiKeyRecord,
}

#[derive(Serialize)]
pub struct CreateTenantResponse {
    pub tenant_id: combee_common::TenantId,
}

/// POST /v1/api-keys —— 创建密钥(明文仅返回一次,库中只存 sha256)。
pub async fn create_api_key(
    State(state): State<AppState>,
    auth: AuthContext,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), ApiError> {
    let plain = api_key::generate();
    let key_hash = api_key::hash(&plain);
    let record = state
        .metadata
        .create_api_key(auth.tenant_id, key_hash)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse { key: plain, record }),
    ))
}

/// GET /v1/api-keys —— 列出租户密钥(不含明文)。
pub async fn list_api_keys(
    State(state): State<AppState>,
    auth: AuthContext,
) -> Result<Json<Vec<ApiKeyRecord>>, ApiError> {
    let keys = state.metadata.list_api_keys(auth.tenant_id).await?;
    Ok(Json(keys))
}

/// DELETE /v1/api-keys/{id} —— 撤销密钥。
pub async fn revoke_api_key(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.metadata.revoke_api_key(auth.tenant_id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /v1/tenants —— 创建租户。
pub async fn create_tenant(
    State(state): State<AppState>,
) -> Result<Json<CreateTenantResponse>, ApiError> {
    let tenant = combee_common::TenantId::new();
    state.metadata.create_tenant(tenant).await?;
    Ok(Json(CreateTenantResponse { tenant_id: tenant }))
}

/// GET /v1/tenants —— 列出租户(管理/计费)。
pub async fn list_tenants(
    State(state): State<AppState>,
) -> Result<Json<Vec<combee_metadata::TenantRecord>>, ApiError> {
    let tenants = state.metadata.list_tenants().await?;
    Ok(Json(tenants))
}
