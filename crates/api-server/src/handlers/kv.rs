//! KV / Redis-style API:GET / SET / DEL / EXISTS / MGET / MSET / TTL / EXPIRE / PERSIST / INCR。

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use combee_common::DatabaseId;
use combee_common::protocol::{
    KvExpireRequest, KvIncrRequest, KvKeysRequest, KvMultiGetResponse, KvMultiSetRequest,
    KvSetRequest,
};
use combee_common::usage::UsageMetric;

use serde::Serialize;

use crate::{ApiError, AppState};

async fn require_db(
    state: &AppState,
    tenant: combee_common::TenantId,
    id: DatabaseId,
    internal: bool,
) -> Result<combee_metadata::DatabaseRecord, ApiError> {
    if internal {
        return state
            .metadata
            .get_database_by_id(id)
            .await
            .map_err(ApiError::from);
    }
    state
        .metadata
        .get_database(tenant, id)
        .await
        .map_err(ApiError::from)
}

#[derive(utoipa::ToSchema, Serialize)]
pub struct KvGetResponse {
    pub exists: bool,
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<i64>,
}

/// GET /v1/databases/{id}/kv/{key}
/// GET KV。
#[utoipa::path(
    get,
    path = "/v1/databases/{id}/kv/{key}",
    params(("id" = DatabaseId, Path, description = "Cell id"), ("key" = String, Path, description = "KV key")),
    responses((status = 200, description = "value or null", body = KvGetResponse)),
    tag = "kv"
)]
pub async fn kv_get(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path((id, key)): Path<(DatabaseId, String)>,
) -> Result<Json<KvGetResponse>, ApiError> {
    require_db(&state, auth.tenant_id, id, auth.internal).await?;
    state
        .usage
        .record(auth.tenant_id, Some(id), UsageMetric::KvRead, 1);
    let client = state.data_node.client_for(id).await?;
    match client.kv_get(id, key).await? {
        Some(entry) => Ok(Json(KvGetResponse {
            exists: true,
            value: Some(entry.value),
            ttl_seconds: entry.ttl_seconds,
        })),
        None => Ok(Json(KvGetResponse {
            exists: false,
            value: None,
            ttl_seconds: None,
        })),
    }
}

#[derive(utoipa::ToSchema, Serialize)]
pub struct KvSetResponse {
    /// 是否真正写入(NX/XX 条件下可能为 false)。
    pub written: bool,
}

/// PUT /v1/databases/{id}/kv/{key} body: `{"value": "...", "ttl_seconds": 60, "nx": false, "xx": false}`
/// PUT KV。
#[utoipa::path(
    put,
    path = "/v1/databases/{id}/kv/{key}",
    params(("id" = DatabaseId, Path, description = "Cell id"), ("key" = String, Path, description = "KV key")),
    request_body = KvSetRequest,
    responses((status = 200, description = "set result", body = KvSetResponse)),
    tag = "kv"
)]
pub async fn kv_set(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path((id, key)): Path<(DatabaseId, String)>,
    Json(req): Json<KvSetRequest>,
) -> Result<Json<KvSetResponse>, ApiError> {
    let record = require_db(&state, auth.tenant_id, id, auth.internal).await?;
    state
        .usage
        .record(auth.tenant_id, Some(id), UsageMetric::KvWrite, 1);
    let client = state.data_node.client_for(id).await?;
    let written = client.kv_set(id, key, req, record.generation).await?;
    Ok(Json(KvSetResponse { written }))
}

#[derive(utoipa::ToSchema, Serialize)]
pub struct KvDelResponse {
    pub deleted: bool,
}

/// DELETE /v1/databases/{id}/kv/{key}
/// DELETE KV。
#[utoipa::path(
    delete,
    path = "/v1/databases/{id}/kv/{key}",
    params(("id" = DatabaseId, Path, description = "Cell id"), ("key" = String, Path, description = "KV key")),
    responses((status = 200, description = "deleted flag", body = KvDelResponse)),
    tag = "kv"
)]
pub async fn kv_del(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path((id, key)): Path<(DatabaseId, String)>,
) -> Result<Json<KvDelResponse>, ApiError> {
    let record = require_db(&state, auth.tenant_id, id, auth.internal).await?;
    state
        .usage
        .record(auth.tenant_id, Some(id), UsageMetric::KvWrite, 1);
    let client = state.data_node.client_for(id).await?;
    let deleted = client.kv_del(id, key, record.generation).await?;
    Ok(Json(KvDelResponse { deleted }))
}

/// POST /v1/databases/{id}/kv/exists body: `{"keys": ["a", "b"]}` → `[true, false]`
pub async fn kv_exists(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
    Json(req): Json<KvKeysRequest>,
) -> Result<Json<Vec<bool>>, ApiError> {
    require_db(&state, auth.tenant_id, id, auth.internal).await?;
    state
        .usage
        .record(auth.tenant_id, Some(id), UsageMetric::KvRead, 1);
    let client = state.data_node.client_for(id).await?;
    let mut out = Vec::with_capacity(req.keys.len());
    for k in &req.keys {
        out.push(client.kv_exists(id, k.clone()).await?);
    }
    Ok(Json(out))
}

/// POST /v1/databases/{id}/kv/mget body: `{"keys": ["a", "b"]}`
pub async fn kv_mget(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
    Json(req): Json<KvKeysRequest>,
) -> Result<Json<KvMultiGetResponse>, ApiError> {
    let _record = require_db(&state, auth.tenant_id, id, auth.internal).await?;
    state
        .usage
        .record(auth.tenant_id, Some(id), UsageMetric::KvRead, 1);
    let client = state.data_node.client_for(id).await?;
    let values = client.kv_mget(id, req.keys).await?;
    Ok(Json(KvMultiGetResponse { values }))
}

/// POST /v1/databases/{id}/kv/mset body: `{"items": [{"key": "a", "value": "1", "ttl_seconds": 60}]}`
pub async fn kv_mset(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
    Json(req): Json<KvMultiSetRequest>,
) -> Result<StatusCode, ApiError> {
    let record = require_db(&state, auth.tenant_id, id, auth.internal).await?;
    state
        .usage
        .record(auth.tenant_id, Some(id), UsageMetric::KvWrite, 1);
    let client = state.data_node.client_for(id).await?;
    client.kv_mset(id, req.items, record.generation).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /v1/databases/{id}/kv/ttl body: `{"keys": ["a", "b"]}` → `[60, -1]`(-1 = 持久, null = 不存在)
pub async fn kv_ttl(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
    Json(req): Json<KvKeysRequest>,
) -> Result<Json<Vec<Option<i64>>>, ApiError> {
    require_db(&state, auth.tenant_id, id, auth.internal).await?;
    state
        .usage
        .record(auth.tenant_id, Some(id), UsageMetric::KvRead, 1);
    let client = state.data_node.client_for(id).await?;
    let mut out = Vec::with_capacity(req.keys.len());
    for k in &req.keys {
        out.push(client.kv_ttl(id, k.clone()).await?);
    }
    Ok(Json(out))
}

#[derive(Serialize)]
pub struct KvExpireResponse {
    /// key 是否存在(未过期)。
    pub updated: bool,
}

/// POST /v1/databases/{id}/kv/expire body: `{"key": "a", "ttl_seconds": 60}`;ttl_seconds 缺省时 PERSIST。
pub async fn kv_expire(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
    Json(req): Json<KvExpireRequest>,
) -> Result<Json<KvExpireResponse>, ApiError> {
    let record = require_db(&state, auth.tenant_id, id, auth.internal).await?;
    state
        .usage
        .record(auth.tenant_id, Some(id), UsageMetric::KvWrite, 1);
    let client = state.data_node.client_for(id).await?;
    let updated = client.kv_expire(id, req, record.generation).await?;
    Ok(Json(KvExpireResponse { updated }))
}

#[derive(Serialize)]
pub struct KvIncrResponse {
    pub value: i64,
}

/// POST /v1/databases/{id}/kv/incr body: `{"key": "c", "delta": 1, "ttl_seconds": 60}`;delta 缺省为 1,负数即 DECR。
pub async fn kv_incr(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
    Json(req): Json<KvIncrRequest>,
) -> Result<Json<KvIncrResponse>, ApiError> {
    let record = require_db(&state, auth.tenant_id, id, auth.internal).await?;
    state
        .usage
        .record(auth.tenant_id, Some(id), UsageMetric::KvWrite, 1);
    let client = state.data_node.client_for(id).await?;
    let value = client.kv_incr(id, req, record.generation).await?;
    Ok(Json(KvIncrResponse { value }))
}

/// GET /v1/databases/{id}/kv?prefix=&limit=&cursor= —— KV 前缀浏览。
#[derive(serde::Deserialize, utoipa::IntoParams)]
pub struct KvListQuery {
    /// key 前缀过滤,缺省列出全部。
    pub prefix: Option<String>,
    /// 每页条数(默认 50,上限 1000)。
    pub limit: Option<u32>,
    /// 分页游标(上一页最后一个 key)。
    pub cursor: Option<String>,
}

pub async fn kv_list(
    State(state): State<AppState>,
    auth: combee_common::AuthContext,
    Path(id): Path<DatabaseId>,
    Query(q): Query<KvListQuery>,
) -> Result<Json<combee_common::rpc::RpcKvScanResult>, ApiError> {
    require_db(&state, auth.tenant_id, id, auth.internal).await?;
    state
        .usage
        .record(auth.tenant_id, Some(id), UsageMetric::KvRead, 1);
    let client = state.data_node.client_for(id).await?;
    let result = client
        .kv_scan(
            id,
            q.prefix.unwrap_or_default(),
            q.limit.unwrap_or(50),
            q.cursor.unwrap_or_default(),
        )
        .await?;
    Ok(Json(result))
}
