//! 自动 / 手动 failover + generation fencing。
//!
//! failover 流程(主节点失效时把副本提升为主):
//! 1. 副本追平 —— 立即从对象存储拉取主节点最新归档(`replicate`);
//! 2. 提升 —— metadata `storage_node_id = 副本`、清 `replica_node_id`、`generation += 1`;
//! 3. fence 新主 —— 通知新主该 Cell 的 generation;
//! 4. fence 旧主(尽力)—— 旧主恢复后,写请求带新 generation ≠ 其本地旧值 → 拒绝(防脑裂)。

use std::sync::Arc;
use std::time::Duration;

use combee_common::DatabaseId;
use combee_common::config::Config;
use combee_metadata::{DatabaseRecord, MetadataStore};

use crate::ApiError;
use crate::AppState;
use crate::client::DataNodeProvider;
use crate::nodes::NodeRegistry;

/// 执行一次 failover,返回提升后的目录记录。
pub async fn failover_cell(
    state: &AppState,
    tenant: combee_common::TenantId,
    db: DatabaseId,
) -> Result<DatabaseRecord, ApiError> {
    let record = state.metadata.get_database(tenant, db).await?;
    let Some(replica) = record.replica_node_id else {
        return Err(combee_common::CombeeError::Internal(format!(
            "cell {db} has no replica configured, cannot failover"
        ))
        .into());
    };

    // 1. 副本追平(拉取主节点最新归档)
    let replica_client = state.data_node.client_for_node(replica).await?;
    replica_client.replicate(db).await?;

    // 2. 提升副本为主,generation += 1
    let promoted = state.metadata.promote_replica(tenant, db).await?;

    // 3. fence 新主
    let _ = replica_client.fence_cell(db, promoted.generation).await;
    // 路由缓存失效:让后续写请求按新 storage_node_id(副本)路由,而不是旧主
    state.data_node.invalidate_route(db);

    // 4. fence 旧主(尽力而为;旧主可能不可达)
    // 旧主可能不可达:fence 失败忽略(尽力而为)。
    // 旧主 fence 到 i64::MAX = "降级标记":任何正常写(gen < MAX)都被其拒绝,防脑裂。
    #[allow(clippy::collapsible_if)]
    if let Some(old) = record.storage_node_id {
        if old != replica {
            if let Ok(c) = state.data_node.client_for_node(old).await {
                let _ = c.fence_cell(db, i64::MAX).await;
            }
        }
    }
    tracing::info!(
        %db,
        new_primary = %replica,
        generation = promoted.generation,
        "failover complete"
    );
    Ok(promoted)
}

/// 自动 failover 扫描:周期检查所有 Cell,主节点心跳超时且有副本 → 触发 failover。
pub fn spawn_failover_scanner(
    metadata: Arc<dyn MetadataStore>,
    nodes: Arc<NodeRegistry>,
    provider: Arc<dyn DataNodeProvider>,
    _cfg: &Config,
) -> Option<tokio::task::JoinHandle<()>> {
    let interval_secs: u64 = std::env::var("COMBEE_FAILOVER_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if interval_secs == 0 {
        return None;
    }
    let interval = Duration::from_secs(interval_secs);
    tracing::info!("auto failover scanner every {interval_secs}s");
    Some(tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let records = match metadata.list_all_databases().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("failover scan list failed: {e}");
                    continue;
                }
            };
            for rec in records {
                let Some(primary) = rec.storage_node_id else {
                    continue;
                };
                if rec.replica_node_id.is_some() && !nodes.is_healthy(primary).await {
                    // 主节点心跳超时且有副本 → failover
                    let state = AppState {
                        metadata: metadata.clone(),
                        data_node: provider.clone(),
                        nodes: nodes.clone(),
                        auth_mode: crate::auth::AuthMode::Off,
                        control_plane_token: None,
                        usage: crate::usage::UsageMeter::new(
                            metadata.clone(),
                            std::time::Duration::from_secs(3600),
                        ),
                        pricing: crate::pricing::PricingManager::new(
                            metadata.clone(),
                            std::time::Duration::from_secs(3600),
                        ),
                        admin_token: None,
                        bff_service_key: None,
                        quota: Default::default(),
                        concurrency: Default::default(),
                        min_credit_balance_units: -100
                            * combee_common::credit::CREDIT_UNITS_PER_CREDIT,
                    };
                    match failover_cell(&state, rec.tenant_id, rec.id).await {
                        Ok(promoted) => {
                            combee_common::metrics::counter_inc(
                                "combee_failovers_total",
                                &[("service", "api"), ("trigger", "auto")],
                            );
                            tracing::info!(
                                db = %rec.id,
                                new_primary = %promoted.storage_node_id.map(|n| n.to_string()).unwrap_or_default(),
                                generation = promoted.generation,
                                "auto failover triggered"
                            )
                        }
                        Err(e) => {
                            combee_common::metrics::counter_inc(
                                "combee_failover_failures_total",
                                &[("service", "api"), ("trigger", "auto")],
                            );
                            tracing::warn!(db = %rec.id, "auto failover failed: {}", e.0);
                        }
                    }
                }
            }
        }
    }))
}
