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
            info!(written, "settlement round done");
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
                if let Err(e) = this.settle_once().await {
                    warn!("settlement round failed: {e}");
                }
            }
        })
    }
}

// 保持 UsageKey / DatabaseId 引用(供后续按 Cell 粒度结算扩展)。
#[allow(dead_code)]
fn _unused(_: &UsageKey, _: DatabaseId) {}
