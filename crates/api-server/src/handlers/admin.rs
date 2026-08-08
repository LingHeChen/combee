//! Operator / Admin API(设计文档 P1 §6.5、§7、§15)。
//!
//! 由 `COMBEE_ADMIN_TOKEN` 保护(**与租户 `x-api-key`、control-plane token 互不相同**):
//! - `POST /admin/tenants/{id}/credits/grant` —— 运营发额度(alpha tester / 补偿 / 活动);
//! - `POST /admin/vouchers/generate` —— 批量生成兑换券(明文仅返回一次);
//! - `GET /admin/vouchers` —— 全部兑换券;
//! - `POST /admin/pricing/versions` —— 创建并激活新定价版本(热生效 ≤5s);
//! - `GET /admin/pricing/versions` —— 全部版本。
//!
//! 所有涉及金额的 admin 操作应进 audit log(P1 §17;V0.1.0-beta 前补)。

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use combee_common::credit::{CreditTransaction, CreditTransactionType};
use combee_common::usage::UsageMetric;
use combee_common::{PricingRule, TenantId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ApiError, AppState};

#[derive(Debug, Deserialize)]
pub struct GrantRequest {
    /// microcredits 整数(1 credit = 1_000_000 microcredits)。
    pub amount_units: i64,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GrantResponse {
    pub balance: String,
    pub transaction_id: Uuid,
}

/// POST /admin/tenants/{tenant}/credits/grant
pub async fn admin_grant_credits(
    State(state): State<AppState>,
    Path(tenant): Path<TenantId>,
    Json(req): Json<GrantRequest>,
) -> Result<(StatusCode, Json<GrantResponse>), ApiError> {
    if req.amount_units <= 0 {
        return Err(ApiError(combee_common::CombeeError::InvalidRequest(
            "amount_units must be > 0".into(),
        )));
    }
    let txn = CreditTransaction {
        id: Uuid::new_v4(),
        tenant_id: tenant,
        txn_type: CreditTransactionType::Grant,
        amount_units: req.amount_units,
        pricing_version: None,
        reference_id: Some(format!("grant:{}:{}", tenant.0, Uuid::new_v4())),
        description: req.reason.or(Some("admin grant".into())),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
        balance_after: None,
    };
    let entry = state.metadata.append_credit_transaction(txn).await?;
    Ok((
        StatusCode::CREATED,
        Json(GrantResponse {
            balance: entry.balance_after.unwrap_or(0).to_string(),
            transaction_id: entry.id,
        }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct GenerateVouchersRequest {
    pub amount_units: i64,
    pub count: u32,
    pub campaign: Option<String>,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct VoucherPlain {
    pub code: String,
    pub amount_units: i64,
}

#[derive(Debug, Serialize)]
pub struct GenerateVouchersResponse {
    /// 明文兑换码(仅生成时可见;库中只存哈希)。
    pub codes: Vec<VoucherPlain>,
}

/// POST /admin/vouchers/generate
pub async fn admin_generate_vouchers(
    State(state): State<AppState>,
    Json(req): Json<GenerateVouchersRequest>,
) -> Result<Json<GenerateVouchersResponse>, ApiError> {
    if req.amount_units <= 0 || req.count == 0 || req.count > 1000 {
        return Err(ApiError(combee_common::CombeeError::InvalidRequest(
            "amount_units > 0 and 1 <= count <= 1000".into(),
        )));
    }
    let vouchers = state
        .metadata
        .create_vouchers(req.amount_units, req.count, req.campaign, req.expires_at)
        .await?;
    Ok(Json(GenerateVouchersResponse {
        codes: vouchers
            .into_iter()
            .map(|(code, v)| VoucherPlain {
                code,
                amount_units: v.amount_units,
            })
            .collect(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct ListVouchersQuery {
    pub limit: Option<i64>,
}

/// GET /admin/vouchers
pub async fn admin_list_vouchers(
    State(state): State<AppState>,
    Query(q): Query<ListVouchersQuery>,
) -> Result<Json<Vec<combee_common::CreditVoucher>>, ApiError> {
    let vouchers = state
        .metadata
        .list_vouchers(q.limit.unwrap_or(100).clamp(1, 1000))
        .await?;
    Ok(Json(vouchers))
}

#[derive(Debug, Deserialize)]
pub struct CreatePricingVersionRequest {
    /// 形如 [{"metric": "kv_read", "unit_size": 1000, "price_units": 10}]
    pub rules: Vec<PricingRuleIn>,
}

#[derive(Debug, Deserialize)]
pub struct PricingRuleIn {
    pub metric: String,
    pub unit_size: i64,
    pub price_units: i64,
}

#[derive(Debug, Serialize)]
pub struct CreatePricingVersionResponse {
    pub version: i64,
    pub status: String,
}

/// POST /admin/pricing/versions —— 创建并激活(旧版本自动 inactive;PricingManager ≤5s 热生效)。
pub async fn admin_create_pricing_version(
    State(state): State<AppState>,
    Json(req): Json<CreatePricingVersionRequest>,
) -> Result<Json<CreatePricingVersionResponse>, ApiError> {
    if req.rules.is_empty() {
        return Err(ApiError(combee_common::CombeeError::InvalidRequest(
            "at least one rule required".into(),
        )));
    }
    let mut rules = Vec::new();
    for r in req.rules {
        let metric = UsageMetric::parse(&r.metric).ok_or_else(|| {
            ApiError(combee_common::CombeeError::InvalidRequest(format!(
                "unknown metric: {}",
                r.metric
            )))
        })?;
        if r.unit_size <= 0 || r.price_units <= 0 {
            return Err(ApiError(combee_common::CombeeError::InvalidRequest(
                "unit_size and price_units must be > 0".into(),
            )));
        }
        rules.push(PricingRule {
            pricing_version: 0,
            metric,
            unit_size: r.unit_size,
            price_units: r.price_units,
        });
    }
    let version = state.metadata.create_pricing_version(rules).await?;
    Ok(Json(CreatePricingVersionResponse {
        version: version.version,
        status: "active".into(),
    }))
}

/// GET /admin/pricing/versions
pub async fn admin_list_pricing_versions(
    State(state): State<AppState>,
) -> Result<Json<Vec<combee_common::PricingVersion>>, ApiError> {
    let versions = state.metadata.list_pricing_versions().await?;
    Ok(Json(versions))
}
