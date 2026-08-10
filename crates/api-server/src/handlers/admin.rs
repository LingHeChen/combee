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
use combee_common::api_key;
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

/// 迁移 Cell 到指定节点(运维;admin API):
/// 1. 冻结写(fencing):generation +1 → 旧节点后续写被拒;
/// 2. 源节点全量备份到对象存储;
/// 3. 目标节点从该快照恢复;
/// 4. 切换路由(storage_node_id = 目标)+ 失效路由缓存。
#[derive(Debug, Deserialize)]
pub struct MigrateCellRequest {
    pub to_node_id: combee_common::NodeId,
}

#[derive(Debug, Serialize)]
pub struct MigrateCellResponse {
    pub cell_id: combee_common::DatabaseId,
    pub from_node: Option<combee_common::NodeId>,
    pub to_node: combee_common::NodeId,
    pub generation: i64,
}

pub async fn admin_migrate_cell(
    State(state): State<AppState>,
    Path(id): Path<combee_common::DatabaseId>,
    Json(req): Json<MigrateCellRequest>,
) -> Result<Json<MigrateCellResponse>, ApiError> {
    let record = state.metadata.get_database_by_id(id).await?;
    let from_node = record.storage_node_id;

    // 目标节点必须存在且不是当前主节点
    if Some(req.to_node_id) == from_node {
        return Err(ApiError(combee_common::CombeeError::InvalidRequest(
            "to_node_id is already the storage node".into(),
        )));
    }
    state
        .data_node
        .client_for_node(req.to_node_id)
        .await?;

    // 1) 冻结写:fencing(generation +1)
    let frozen = state
        .metadata
        .migrate_database(record.tenant_id, id, req.to_node_id)
        .await?;
    let generation = frozen.generation;

    // 2) 源节点备份(全量快照 → 对象存储)
    let backup_info = match state.data_node.client_for(id).await {
        Ok(src) => src.backup(id).await,
        Err(e) => {
            tracing::error!(%id, to_node = %req.to_node_id, "migrate: source backup skipped ({e})");
            // 源节点不可达:仍尝试直接恢复(目标节点从最新快照兜底)
            Err(e)
        }
    };
    let version = backup_info.ok().map(|b| b.key);

    // 3) 目标节点恢复(优先用刚上传的快照;否则用目标侧最新)
    state
        .data_node
        .client_for_node(req.to_node_id)
        .await?
        .restore(id, version)
        .await?;

    // 4) 切路由:metadata 已在第 1 步切好;清理路由缓存让新请求立即走新节点
    state.data_node.invalidate_route(id);
    tracing::info!(
        %id,
        from = %from_node.map(|n| n.to_string()).unwrap_or_else(|| "-".into()),
        to = %req.to_node_id,
        generation,
        "cell migrated"
    );
    Ok(Json(MigrateCellResponse {
        cell_id: id,
        from_node,
        to_node: req.to_node_id,
        generation,
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


/// POST /admin/tenants —— 为独立新租户创建一个专属 API key(注册/开通用)。
/// 响应中的 key 明文仅返回一次。
#[derive(Serialize)]
pub struct CreateTenantKeyResponse {
    pub tenant_id: TenantId,
    pub key: String,
    pub key_id: Uuid,
}

pub async fn admin_create_tenant(
    State(state): State<crate::AppState>,
) -> Result<Json<CreateTenantKeyResponse>, crate::ApiError> {
    let tenant = TenantId::new();
    state.metadata.create_tenant(tenant).await?;
    let key = api_key::generate();
    let hash = api_key::hash(&key);
    let record = state
        .metadata
        .create_api_key(tenant, hash, "console-user")
        .await?;
    Ok(Json(CreateTenantKeyResponse {
        tenant_id: tenant,
        key,
        key_id: record.id,
    }))
}
