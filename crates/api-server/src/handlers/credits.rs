//! Credits / Voucher / Pricing 用户 API(设计文档 P1 §6-7)。
//!
//! - `GET /v1/credits/balance` —— 余额(整数 microcredits,decimal string 返回);
//! - `GET /v1/credits/transactions?limit&cursor` —— 账本(append-only,倒序分页);
//! - `POST /v1/credits/redeem {"code": "CMB-..."}` —— 兑换(单次/幂等/并发安全);
//! - `GET /v1/pricing` —— 当前定价(只读;权威计价在服务端)。

use axum::Json;
use axum::extract::{Query, State};
use combee_common::AuthContext;
use combee_common::credit::hash_voucher_code;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ApiError, AppState};

#[derive(Debug, Serialize)]
pub struct CreditBalanceResponse {
    /// 可用余额(decimal string,microcredits 整数;禁止浮点)。
    pub available: String,
    pub reserved: String,
    pub currency: &'static str,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct TxnQuery {
    pub limit: Option<i64>,
    pub cursor: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct TxnPage {
    pub items: Vec<combee_common::CreditTransaction>,
    pub next_cursor: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct RedeemRequest {
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct RedeemResponse {
    pub credits_added: String,
    pub balance: String,
    /// 该 code 此前已兑换(幂等重试)——本次未重复加钱。
    pub already_redeemed: bool,
}

/// GET /v1/credits/balance
pub async fn credits_balance(
    State(state): State<AppState>,
    auth: AuthContext,
) -> Result<Json<CreditBalanceResponse>, ApiError> {
    let account = state.metadata.get_credit_account(auth.tenant_id).await?;
    Ok(Json(CreditBalanceResponse {
        available: account.balance_units.to_string(),
        reserved: account.reserved_units.to_string(),
        currency: "CREDIT",
        updated_at: account.updated_at,
    }))
}

/// GET /v1/credits/transactions?limit=100&cursor=<uuid>
pub async fn credits_transactions(
    State(state): State<AppState>,
    auth: AuthContext,
    Query(q): Query<TxnQuery>,
) -> Result<Json<TxnPage>, ApiError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    let items = state
        .metadata
        .list_credit_transactions(auth.tenant_id, limit + 1, q.cursor)
        .await?;
    let has_more = items.len() as i64 > limit;
    let items: Vec<_> = items.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        items.last().map(|t| t.id)
    } else {
        None
    };
    Ok(Json(TxnPage { items, next_cursor }))
}

/// POST /v1/credits/redeem —— 单次兑换,幂等重试不重复加钱。
pub async fn credits_redeem(
    State(state): State<AppState>,
    auth: AuthContext,
    Json(req): Json<RedeemRequest>,
) -> Result<Json<RedeemResponse>, ApiError> {
    let code = req.code.trim();
    let hash = hash_voucher_code(code);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // 幂等:该 code 已兑换过 → 返回已兑换结果(不重复加钱)
    let reference = format!("voucher:{hash}");
    if let Some(txn) = state
        .metadata
        .find_transaction_by_reference(&reference)
        .await?
    {
        let account = state.metadata.get_credit_account(auth.tenant_id).await?;
        return Ok(Json(RedeemResponse {
            credits_added: txn.amount_units.to_string(),
            balance: account.balance_units.to_string(),
            already_redeemed: true,
        }));
    }

    let amount = state
        .metadata
        .redeem_voucher(&hash, auth.tenant_id, now)
        .await?;
    let account = state.metadata.get_credit_account(auth.tenant_id).await?;
    Ok(Json(RedeemResponse {
        credits_added: amount.to_string(),
        balance: account.balance_units.to_string(),
        already_redeemed: false,
    }))
}

#[derive(Debug, Serialize)]
pub struct PricingResponse {
    pub version: i64,
    pub effective_at: i64,
    pub units: std::collections::HashMap<String, serde_json::Value>,
}

/// GET /v1/pricing —— 当前生效定价(只读)。
pub async fn get_pricing(
    State(state): State<AppState>,
    _auth: AuthContext,
) -> Result<Json<PricingResponse>, ApiError> {
    let cfg = state.pricing.current();
    let mut units = std::collections::HashMap::new();
    for (metric, (unit_size, price_units)) in &cfg.rules {
        units.insert(
            metric.as_str().to_string(),
            serde_json::json!({
                "unit_size": unit_size,
                "price_units": price_units,
                "price_per_unit": format!("{} microcredits / {} {}", price_units, unit_size, metric.as_str()),
            }),
        );
    }
    Ok(Json(PricingResponse {
        version: cfg.version,
        effective_at: cfg.effective_at,
        units,
    }))
}
