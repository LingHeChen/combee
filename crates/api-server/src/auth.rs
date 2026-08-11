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
                internal: false,
            }))
    }
}

/// 控制面认证:保护 `/internal/*` 端点。
///
/// 规则:
/// 1. 携带租户 `x-api-key` 的请求**永远**拒绝(租户 key 不能触碰控制面);
/// 2. 配置了 `control_plane_token` 时,必须提供
///    `Authorization: Bearer <token>` 或 `x-control-token: <token>`;
/// 3. 未配置 token(dev):无 `x-api-key` 即放行。
pub async fn internal_auth(State(state): State<AppState>, req: Request, next: Next) -> Response {
    // 租户 key 永不进入内部接口
    if req.headers().contains_key("x-api-key") {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                code: "unauthorized".into(),
                error: "unauthorized".into(),
            }),
        )
            .into_response();
    }
    if let Some(expected) = &state.control_plane_token {
        let bearer_ok = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|t| t == expected)
            .unwrap_or(false);
        let header_ok = req
            .headers()
            .get("x-control-token")
            .and_then(|v| v.to_str().ok())
            .map(|t| t == expected)
            .unwrap_or(false);
        if !bearer_ok && !header_ok {
            tracing::warn!(
                service = "combee-api",
                event = "internal.unauthorized",
                path = %req.uri(),
                provided = req.headers().get("x-control-token").and_then(|v| v.to_str().ok()).map(|s| s.chars().take(8).collect::<String>()),
            );
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorBody {
                    code: "unauthorized".into(),
                    error: "unauthorized".into(),
                }),
            )
                .into_response();
        }
    }
    next.run(req).await
}

/// Operator/Admin 认证:`COMBEE_ADMIN_TOKEN`(与租户 key、control-plane token 三者互不相同)。
/// 未配置 token 时 admin 接口一律 401(必须显式配置)。
pub async fn admin_auth(State(state): State<AppState>, req: Request, next: Next) -> Response {
    // 预配置的 admin API key(COMBEE_ADMIN_API_KEY)可以调用 admin 接口;
    // 其余租户 key 永远不能调用 admin 接口。
    if let Some(admin_key) = &state.admin_api_key {
        if req.headers().get("x-api-key").and_then(|v| v.to_str().ok()) == Some(admin_key.as_str())
        {
            return next.run(req).await;
        }
    }
    if req.headers().contains_key("x-api-key") {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                code: "unauthorized".into(),
                error: "unauthorized".into(),
            }),
        )
            .into_response();
    }
    let Some(expected) = &state.admin_token else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                code: "unauthorized".into(),
                error: "COMBEE_ADMIN_TOKEN not configured".into(),
            }),
        )
            .into_response();
    };
    let bearer_ok = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t == expected)
        .unwrap_or(false);
    let header_ok = req
        .headers()
        .get("x-admin-token")
        .and_then(|v| v.to_str().ok())
        .map(|t| t == expected)
        .unwrap_or(false);
    if !bearer_ok && !header_ok {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                code: "unauthorized".into(),
                error: "unauthorized".into(),
            }),
        )
            .into_response();
    }
    next.run(req).await
}

/// request-id 中间件:透传客户端 `x-request-id`,缺失则生成;响应与错误均回显。
/// SDK 通过它关联请求与日志/支持工单。
pub async fn request_id(mut req: Request, next: Next) -> Response {
    let id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("req_{}", uuid::Uuid::new_v4().simple()));
    req.extensions_mut().insert(crate::RequestId(id.clone()));
    // task-local scope:同 task 的 handler / RPC 调用可读到 request_id
    crate::REQUEST_ID
        .scope(id.clone(), async {
            let mut response = next.run(req).await;
            if let Ok(v) = id.parse() {
                response.headers_mut().insert("x-request-id", v);
            }
            response
        })
        .await
}

/// 认证中间件:校验 key(若启用)并注入 AuthContext。
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let mut internal = false;
    // 平台服务账号(COMBEE_ADMIN_API_KEY):key 明文匹配 → internal,不计费。
    // BFF/console 平台请求统一用它;租户 API key 直连(SDK/curl)不计 internal。
    // 仅比对明文避免伪造;该 key 仍需在 metadata 中有效(admin_auth 也校验)。
    let tenant: TenantId = if state.auth_mode == AuthMode::Key {
        match req.headers().get("x-api-key").and_then(|v| v.to_str().ok()) {
            Some(key) => {
                if state
                    .admin_api_key
                    .as_deref()
                    .map(|admin| admin == key)
                    .unwrap_or(false)
                {
                    internal = true;
                }
                let key_hash = hash(key);
                match state.metadata.lookup_api_key_by_hash(&key_hash).await {
                    Ok(Some(record)) => record.tenant_id,
                    _ => {
                        return (
                            StatusCode::UNAUTHORIZED,
                            Json(ErrorBody {
                                code: "unauthorized".into(),
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
                        code: "unauthorized".into(),
                        error: "unauthorized".into(),
                    }),
                )
                    .into_response();
            }
        }
    } else {
        DEFAULT_TENANT
    };
    req.extensions_mut().insert(AuthContext {
        tenant_id: tenant,
        internal,
    });
    next.run(req).await
}
