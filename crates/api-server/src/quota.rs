//! 资源配额(安全护栏):per-tenant / per-Cell 并发限制(429)。
//!
//! 简单 in-flight 计数器(非精确信号量,护栏足够);超限返回 QuotaExceeded。

use std::collections::HashMap;
use std::sync::Mutex;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use combee_common::usage::UsageMetric;
use combee_common::{AuthContext, DatabaseId};

use crate::AppState;

#[derive(Default)]
pub struct ConcurrencyCounters {
    inflight: Mutex<HashMap<String, usize>>,
}

impl ConcurrencyCounters {
    #[allow(clippy::result_unit_err)]
    pub fn try_enter(&self, key: &str, max: usize) -> Result<Guard<'_>, ()> {
        let mut m = self.inflight.lock().unwrap();
        let cur = m.entry(key.to_string()).or_insert(0);
        if max > 0 && *cur >= max {
            return Err(());
        }
        *cur += 1;
        Ok(Guard {
            map: &self.inflight,
            key: key.to_string(),
        })
    }
}

pub struct Guard<'a> {
    map: &'a Mutex<HashMap<String, usize>>,
    key: String,
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        let Ok(mut m) = self.map.lock() else { return };
        if let Some(v) = m.get_mut(&self.key) {
            *v = v.saturating_sub(1);
            if *v == 0 {
                m.remove(&self.key);
            }
        }
    }
}

fn parse_cell_from_path(path: &str) -> Option<DatabaseId> {
    let mut parts = path.split('/').filter(|s| !s.is_empty());
    if parts.next()? != "v1" || parts.next()? != "databases" {
        return None;
    }
    let id = parts.next()?;
    id.parse::<DatabaseId>().ok()
}

/// 余额护栏:非 internal 请求在余额低于阈值时 402(预充值模型的拒付边界)。
/// 挂载在 auth 之后、usage 计费之前;internal(BFF/admin 平台请求)豁免。
pub async fn credit_quota(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if let Some(auth) = req.extensions().get::<AuthContext>().copied()
        && !auth.internal
        && let Ok(account) = state.metadata.get_credit_account(auth.tenant_id).await
        && account.balance_units < state.min_credit_balance_units
    {
        return (
            axum::http::StatusCode::PAYMENT_REQUIRED,
            axum::Json(serde_json::json!({
                "code": "insufficient_credits",
                "error": "credit balance below minimum; top up to continue"
            })),
        )
            .into_response();
    }
    // 余额查询失败或无 AuthContext:放行(降级,避免计费故障阻塞业务面)
    next.run(req).await
}

/// 并发配额中间件(挂在 auth 之后;读 AuthContext 与路径 Cell)。
pub async fn concurrency_quota(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let tenant = req
        .extensions()
        .get::<AuthContext>()
        .map(|a| a.tenant_id)
        .unwrap_or(combee_metadata::DEFAULT_TENANT);
    let cell = parse_cell_from_path(req.uri().path());
    let q = &state.quota;

    let tenant_key = format!("t:{}", tenant.0);
    let tenant_guard = match state
        .concurrency
        .try_enter(&tenant_key, q.max_per_tenant_concurrency)
    {
        Ok(g) => g,
        Err(()) => {
            state.usage.record(tenant, cell, UsageMetric::Requests, 1);
            return quota_response();
        }
    };
    let cell_guard = match cell {
        Some(c) if q.max_per_cell_concurrency > 0 => {
            let key = format!("c:{}", c.0);
            match state
                .concurrency
                .try_enter(&key, q.max_per_cell_concurrency)
            {
                Ok(g) => Some(g),
                Err(()) => {
                    state
                        .usage
                        .record(tenant, Some(c), UsageMetric::Requests, 1);
                    return quota_response();
                }
            }
        }
        _ => None,
    };

    let _ = (tenant_guard, cell_guard); // 作用域结束即释放
    next.run(req).await
}

fn quota_response() -> Response {
    let body = crate::ErrorBody {
        code: "quota_exceeded".into(),
        error: "concurrency limit exceeded".into(),
    };
    (axum::http::StatusCode::TOO_MANY_REQUESTS, axum::Json(body)).into_response()
}
