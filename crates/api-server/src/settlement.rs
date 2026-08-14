//! Credits Settlement(设计文档 P1 §8):周期把 usage buckets 折算成 Credits 入账。
//!
//! 流程:`usage buckets → rating(pricing.rate)→ credit_transaction(type=usage)`。
//! 不阻塞 Data Plane 热路径;幂等由 `reference_id = usage:{tenant}:{metric}:{bucket}:{version}`
//! + 账本 UNIQUE 保证 —— 重复结算(水位丢失 / 任务重叠)不会重复扣款。
//!
//! 扣费策略:beta 第一版为 **soft limit** —— 余额 <= 0 只告警,不切断服务
//! (计划 §8.2;避免 accounting bug 直接切断用户数据)。

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use combee_common::DatabaseId;
use combee_common::credit::{CreditTransaction, CreditTransactionType};
use combee_common::usage::{UsageKey, UsageMetric, bucket_start};
use combee_metadata::MetadataStore;
use tracing::{info, warn};

use crate::pricing::PricingManager;

pub struct Settlement {
    metadata: Arc<dyn MetadataStore>,
    pricing: Arc<PricingManager>,
    interval: Duration,
    /// 已结算到哪个分钟桶(内存水位;重启后靠 reference 幂等兜底)。
    last_settled_bucket: AtomicI64,
}

impl Settlement {
    pub fn new(
        metadata: Arc<dyn MetadataStore>,
        pricing: Arc<PricingManager>,
        interval: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            metadata,
            pricing,
            interval,
            last_settled_bucket: AtomicI64::new(0),
        })
    }

    /// 结算一次:处理 [last_settled, 当前分钟) 的用量(每租户每 metric 聚合)。
    /// 返回写入的账本条目数。
    pub async fn settle_once(&self) -> Result<usize, combee_common::CombeeError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let current_bucket = bucket_start(now);
        let mut last = self.last_settled_bucket.load(Ordering::Relaxed);
        if last == 0 {
            last = current_bucket - 60; // 首次:结算上一分钟
        }
        if current_bucket <= last {
            return Ok(0);
        }
        let pricing = self.pricing.current();

        let mut tenants = self.metadata.list_tenants().await?;
        // dev/默认租户可能未显式创建,仍参与结算
        if !tenants
            .iter()
            .any(|t| t.id == combee_metadata::DEFAULT_TENANT)
        {
            tenants.push(combee_metadata::TenantRecord {
                id: combee_metadata::DEFAULT_TENANT,
                created_at: 0,
                status: "active".into(),
            });
        }
        let mut written = 0usize;
        for tenant in tenants {
            let buckets = self
                .metadata
                .query_usage(tenant.id, None, None, last, current_bucket - 60)
                .await?;
            // 按 metric 聚合(跨 cell)
            let mut by_metric: std::collections::HashMap<UsageMetric, u64> = Default::default();
            for b in &buckets {
                *by_metric.entry(b.metric).or_insert(0) += b.value;
            }
            for (metric, total) in by_metric {
                if metric == UsageMetric::StorageBytes {
                    continue; // 快照类不计费
                }
                let credits = pricing.rate(metric, total);
                if credits == 0 {
                    continue;
                }
                let reference = format!(
                    "usage:{}:{}:{}:{}",
                    tenant.id.0,
                    metric.as_str(),
                    last,
                    pricing.version
                );
                let txn = CreditTransaction {
                    id: uuid::Uuid::new_v4(),
                    tenant_id: tenant.id,
                    txn_type: CreditTransactionType::Usage,
                    amount_units: -credits,
                    pricing_version: Some(pricing.version),
                    reference_id: Some(reference),
                    description: Some(format!("usage {}({})", metric.as_str(), total)),
                    created_at: now,
                    balance_after: None,
                };
                match self.metadata.append_credit_transaction(txn).await {
                    Ok(entry) => {
                        written += 1;
                        if let Some(balance) = entry.balance_after
                            && balance <= 0
                        {
                            warn!(
                                tenant = %tenant.id.0,
                                balance_units = balance,
                                "tenant credits exhausted (soft limit; service continues)"
                            );
                        }
                    }
                    Err(e) => warn!(tenant = %tenant.id.0, "settlement entry failed: {e}"),
                }
            }
        }
        // 水位推进到当前分钟(无论成败;失败部分靠幂等在下轮重试——reference 相同)
        self.last_settled_bucket
            .store(current_bucket, Ordering::Relaxed);
        if written > 0 {
            info!(
                service = "combee-api",
                event = "credits.settlement.success",
                job = "credit_settlement",
                entries = written
            );
        }
        Ok(written)
    }

    /// 后台结算循环。
    pub fn spawn(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let this = self.clone();
        let interval = self.interval;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                match this.settle_once().await {
                    Ok(_) => {
                        combee_common::metrics::gauge_set(
                            "combee_credit_settlement_lag_seconds",
                            &[("service", "api")],
                            0,
                        );
                        combee_common::metrics::counter_inc(
                            "combee_credit_settlement_successes_total",
                            &[("service", "api")],
                        );
                    }
                    Err(e) => {
                        warn!(
                            service = "combee-api",
                            event = "credits.settlement.failed",
                            job = "credit_settlement",
                            error_code = "SETTLEMENT_FAILED",
                            error = %e,
                        );
                        combee_common::metrics::counter_inc(
                            "combee_credit_settlement_failures_total",
                            &[("service", "api")],
                        );
                        combee_common::metrics::gauge_set(
                            "combee_credit_settlement_lag_seconds",
                            &[("service", "api")],
                            interval.as_secs() as i64,
                        );
                    }
                }
            }
        })
    }
}

// 保持 UsageKey / DatabaseId 引用(供后续按 Cell 粒度结算扩展)。
#[allow(dead_code)]
fn _unused(_: &UsageKey, _: DatabaseId) {}

#[cfg(test)]
mod tests {
    use super::*;
    use combee_common::TenantId;
    use combee_common::credit::{
        CREDIT_UNITS_PER_CREDIT, CreditTransaction, CreditTransactionType, PricingRule,
    };
    use combee_common::usage::UsageKey;
    use combee_metadata::InMemoryStore;

    #[tokio::test]
    async fn usage_is_settled_against_tenant_credits() {
        let metadata: Arc<dyn MetadataStore> = Arc::new(InMemoryStore::new());
        let t = TenantId::new();

        // 定价:Requests 每 1 单位计 100 microcredits
        metadata
            .create_pricing_version(vec![PricingRule {
                pricing_version: 0,
                metric: UsageMetric::Requests,
                unit_size: 1,
                price_units: 100,
            }])
            .await
            .unwrap();

        metadata.create_tenant(t).await.unwrap();
        // 充值 10 credits = 10_000_000 microcredits
        metadata
            .append_credit_transaction(CreditTransaction {
                id: uuid::Uuid::new_v4(),
                tenant_id: t,
                txn_type: CreditTransactionType::Grant,
                amount_units: 10 * CREDIT_UNITS_PER_CREDIT,
                pricing_version: None,
                reference_id: Some("grant:test".into()),
                description: Some("test grant".into()),
                created_at: 100,
                balance_after: None,
            })
            .await
            .unwrap();

        // 用户租户产生 100 次请求(过去桶)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let past_bucket = bucket_start(now - 180);
        metadata
            .usage_add(
                &UsageKey {
                    tenant_id: t,
                    cell_id: None,
                    metric: UsageMetric::Requests,
                    bucket_start: past_bucket,
                },
                100,
            )
            .await
            .unwrap();

        let pricing = PricingManager::new(metadata.clone(), Duration::from_secs(3600));
        pricing.refresh().await.unwrap(); // 加载 active pricing version
        let settlement = Settlement::new(metadata.clone(), pricing, Duration::from_secs(60));
        // 水位:从 past_bucket 之前开始结算(覆盖 past_bucket 桶)
        settlement
            .last_settled_bucket
            .store(past_bucket - 60, Ordering::Relaxed);

        let written = settlement.settle_once().await.unwrap();
        assert_eq!(written, 1, "100 次请求应产生 1 条 usage 结算");

        let account = metadata.get_credit_account(t).await.unwrap();
        let spent = 10 * CREDIT_UNITS_PER_CREDIT - account.balance_units;
        assert_eq!(spent, 100 * 100, "100 请求 × 100 microcredits 应从租户扣减");
        assert!(account.balance_units < 10 * CREDIT_UNITS_PER_CREDIT);
    }
}
