use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;

use crate::ApiError;
use crate::AppState;

/// POST /v1/waitlist —— 公开的 Public Beta 候补登记(无需认证;基础格式校验)。
#[derive(Deserialize)]
pub struct JoinRequest {
    pub email: String,
}

#[derive(Serialize)]
pub struct JoinResponse {
    pub ok: bool,
    pub created_at: i64,
}

pub async fn join(
    State(state): State<AppState>,
    Json(req): Json<JoinRequest>,
) -> Result<Json<JoinResponse>, ApiError> {
    let email = req.email.trim().to_lowercase();
    if !is_valid_email(&email) {
        return Err(crate::ApiError(combee_common::CombeeError::InvalidRequest(
            "invalid email address".into(),
        )));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    state.metadata.create_waitlist_entry(&email, now).await?;
    Ok(Json(JoinResponse {
        ok: true,
        created_at: now,
    }))
}

/// GET /admin/waitlist?limit=100 —— 管理面查看候补列表。
#[derive(Deserialize)]
pub struct ListQuery {
    pub limit: Option<i64>,
}

pub async fn admin_list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<combee_metadata::WaitlistEntry>>, ApiError> {
    let entries = state.metadata.list_waitlist(q.limit.unwrap_or(100)).await?;
    Ok(Json(entries))
}

fn is_valid_email(email: &str) -> bool {
    let parts: Vec<&str> = email.split('@').collect();
    parts.len() == 2 && !parts[0].is_empty() && parts[1].contains('.') && parts[1].len() >= 3
}

/// waitlist 端点专用 CORS:允许任意来源(公开登记,仅 email)。
pub fn waitlist_cors() -> CorsLayer {
    CorsLayer::permissive()
}
