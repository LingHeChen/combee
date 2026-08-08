//! Combee API Server:系统的入口与控制面网关。
//!
//! 职责:Authentication / Tenant isolation / Database lifecycle /
//! Cell routing / SQL / KV / Quota(预留)。V0 单进程运行,
//! 通过 [`client::DataNodeClient`] trait 与 Data Node 解耦,
//! 当前使用 [`client::LocalDataNodeClient`],后续可替换为 gRPC 客户端。

pub mod app;
pub mod auth;
pub mod client;
pub mod failover;
pub mod handlers;
pub mod nodes;
pub mod pricing;
pub mod settlement;
pub mod usage;

use std::sync::Arc;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use combee_common::CombeeError;
use combee_metadata::MetadataStore;
use serde::Serialize;

use crate::client::DataNodeProvider;
use crate::nodes::NodeRegistry;

/// 全局应用状态。
#[derive(Clone)]
pub struct AppState {
    pub metadata: Arc<dyn MetadataStore>,
    /// 按 Cell 路由到对应 Data Node 的客户端提供者。
    pub data_node: Arc<dyn DataNodeProvider>,
    /// Data Node 注册表(registration / heartbeat / placement)。
    pub nodes: Arc<NodeRegistry>,
    /// 认证模式(off = 开发放行;key = 强制 API key 校验)。
    pub auth_mode: crate::auth::AuthMode,
    /// 控制面令牌;`/internal/*` 端点必须匹配它(未配置时 dev 放行)。
    pub control_plane_token: Option<String>,
    /// Usage Metering:内存聚合 + 周期 flush 到 metadata。
    pub usage: Arc<crate::usage::UsageMeter>,
    /// Pricing:active 版本热切换。
    pub pricing: Arc<crate::pricing::PricingManager>,
    /// Operator/Admin 令牌(`COMBEE_ADMIN_TOKEN`);未配置时 admin 接口 401。
    pub admin_token: Option<String>,
}

/// 统一的 JSON 错误响应体。
#[derive(Serialize)]
pub struct ErrorBody {
    pub error: String,
}

/// 把 [`CombeeError`] 映射为 HTTP 响应。
pub struct ApiError(pub CombeeError);

impl std::fmt::Debug for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<CombeeError> for ApiError {
    fn from(e: CombeeError) -> Self {
        Self(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            CombeeError::DatabaseNotFound(_) => StatusCode::NOT_FOUND,
            CombeeError::DatabaseAlreadyExists(_) => StatusCode::CONFLICT,
            CombeeError::ApiKeyNotFound => StatusCode::NOT_FOUND,
            CombeeError::Unauthorized => StatusCode::UNAUTHORIZED,
            CombeeError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            CombeeError::Forbidden(_) => StatusCode::FORBIDDEN,
            CombeeError::QuotaExceeded(_) => StatusCode::TOO_MANY_REQUESTS,
            // SQL 语法/约束错误属于用户输入问题,归为 400。
            CombeeError::Sql(_) => StatusCode::BAD_REQUEST,
            CombeeError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = Json(ErrorBody {
            error: self.0.to_string(),
        });
        (status, body).into_response()
    }
}
