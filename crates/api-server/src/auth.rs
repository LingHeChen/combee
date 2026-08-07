//! API key 认证:请求 key → sha256 → 查 api_keys 表 → 注入 AuthContext{tenant_id}。
//!
//! - `COMBEE_AUTH=off`(默认):开发模式,放行并注入默认租户;
//! - `COMBEE_AUTH=key`:强制校验 `x-api-key`(哈希查表,未撤销才放行)。
//!
//! 整个请求生命周期只携带 `AuthContext`,不传原始 key。
//!

use axum::Json;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use combee_common::api_key::hash;
use combee_common::{AuthContext, TenantId};
use combee_metadata::DEFAULT_TENANT;

use crate::{AppState, ErrorBody};

/// 认证模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuthMode {
    /// 开发模式:放行,注入默认租户。
    #[default]
    Off,
    /// 强制 API key 校验(哈希查 api_keys 表)。
    Key,
}

impl AuthMode {
    pub fn from_env() -> Self {
        match std::env::var("COMBEE_AUTH").as_deref() {
            Ok("key") | Ok("db") => AuthMode::Key,
            _ => AuthMode::Off,
        }
    }
}

/// 从请求扩展中取 AuthContext(认证中间件注入;缺失时回退默认租户,安全兜底)。
impl FromRequestParts<AppState> for AuthContext {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(parts
            .extensions
            .get::<AuthContext>()
            .copied()
            .unwrap_or(AuthContext {
                tenant_id: DEFAULT_TENANT,
            }))
    }
}

/// 认证中间件:校验 key(若启用)并注入 AuthContext。
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let tenant: TenantId = if state.auth_mode == AuthMode::Key {
        match req.headers().get("x-api-key").and_then(|v| v.to_str().ok()) {
            Some(key) => {
                let key_hash = hash(key);
                match state.metadata.lookup_api_key_by_hash(&key_hash).await {
                    Ok(Some(record)) => record.tenant_id,
                    _ => {
                        return (
                            StatusCode::UNAUTHORIZED,
                            Json(ErrorBody {
                                error: "unauthorized".into(),
                            }),
                        )
                            .into_response();
                    }
                }
            }
            None => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(ErrorBody {
                        error: "unauthorized".into(),
                    }),
                )
                    .into_response();
            }
        }
    } else {
        DEFAULT_TENANT
    };
    req.extensions_mut()
        .insert(AuthContext { tenant_id: tenant });
    next.run(req).await
}
