//! Pricing Manager(设计文档 P1 §5):active 定价版本热切换。
//!
//! 原理:周期轮询 metadata 的 active 版本,版本变化时**原子替换** `Arc<PricingConfig>`;
//! 无需重启、无需 Pub/Sub。无效配置(unit_size/price_units <= 0)不会替换当前有效配置。

use std::sync::{Arc, RwLock};
use std::time::Duration;

use combee_common::credit::PricingRule;
use combee_common::usage::UsageMetric;
use combee_common::{PricingConfig, PricingStatus};
use combee_metadata::MetadataStore;
use tracing::warn;

pub struct PricingManager {
    inner: RwLock<Arc<PricingConfig>>,
    metadata: Arc<dyn MetadataStore>,
    interval: Duration,
}

impl PricingManager {
    pub fn new(metadata: Arc<dyn MetadataStore>, interval: Duration) -> Arc<Self> {
        Arc::new(Self {
            inner: RwLock::new(Arc::new(PricingConfig::empty())),
            metadata,
            interval,
        })
    }

    /// 加载 active 版本并(仅在有效且版本变化时)原子替换。
    pub async fn refresh(&self) -> Result<bool, combee_common::CombeeError> {
        let (version, rules) = self.metadata.get_active_pricing().await?;
        if version.status != PricingStatus::Active {
            return Ok(false);
        }
        // 无效配置检查:任一规则 unit_size/price_units <= 0 → 拒绝替换(保留当前)
        if rules.iter().any(|r| r.unit_size <= 0 || r.price_units <= 0) {
            warn!(
                version = version.version,
                "invalid pricing config ignored (kept current)"
            );
            return Ok(false);
        }
        let current = self.current();
        if current.version == version.version {
            return Ok(false);
        }
        let mut map = std::collections::HashMap::new();
        for r in &rules {
            map.insert(r.metric, (r.unit_size, r.price_units));
        }
        let cfg = Arc::new(PricingConfig {
            version: version.version,
            effective_at: version.effective_at,
            rules: map,
        });
        *self.inner.write().unwrap() = cfg;
        Ok(true)
    }

    pub fn current(&self) -> Arc<PricingConfig> {
        self.inner.read().unwrap().clone()
    }

    /// 按当前定价折算 microcredits。
    pub fn rate(&self, metric: UsageMetric, units: u64) -> i64 {
        self.current().rate(metric, units)
    }

    /// 后台刷新循环。
    pub fn spawn_refresher(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let this = self.clone();
        let interval = self.interval;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                if let Err(e) = this.refresh().await {
                    warn!("pricing refresh failed: {e}");
                }
            }
        })
    }
}

/// 播种默认定价:仅当尚无 active 定价(version == 0)时,为 `StorageByteSecs` 配一条
/// GB·h 计费规则,让新部署无需手工建规则即可对存储计费(见 COMBEE_STORAGE_BILLING.md)。
/// 已通过 admin API 配置定价的部署不受影响。
pub async fn seed_default_pricing(
    metadata: &Arc<dyn MetadataStore>,
) -> Result<(), combee_common::CombeeError> {
    let (version, _) = metadata.get_active_pricing().await?;
    if version.version > 0 {
        return Ok(());
    }
    let price_units = std::env::var("COMBEE_STORAGE_PRICE_MICROCREDITS_PER_GB_HOUR")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(combee_common::credit::DEFAULT_STORAGE_PRICE_UNITS_PER_GB_HOUR);
    metadata
        .create_pricing_version(vec![PricingRule {
            pricing_version: 0,
            metric: UsageMetric::StorageByteSecs,
            unit_size: combee_common::credit::BYTE_SECS_PER_GB_HOUR,
            price_units,
        }])
        .await?;
    tracing::info!(
        service = "combee-api",
        event = "pricing.seed.storage",
        price_units = price_units,
        "seeded default storage pricing (GB·h)"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use combee_common::credit::PricingRule;
    use combee_metadata::InMemoryStore;

    #[tokio::test]
    async fn hot_reload_and_invalid_config_rollback() {
        let metadata: Arc<dyn MetadataStore> = Arc::new(InMemoryStore::new());
        let mgr = PricingManager::new(metadata.clone(), Duration::from_secs(3600));

        // 初始:empty(v0)
        assert_eq!(mgr.current().version, 0);
        assert_eq!(mgr.rate(UsageMetric::KvRead, 1_000), 0);

        // 创建 v1 并刷新 → 热切换
        metadata
            .create_pricing_version(vec![PricingRule {
                pricing_version: 0,
                metric: UsageMetric::KvRead,
                unit_size: 1_000,
                price_units: 10,
            }])
            .await
            .unwrap();
        let changed = mgr.refresh().await.unwrap();
        assert!(changed);
        assert_eq!(mgr.current().version, 1);
        assert_eq!(mgr.rate(UsageMetric::KvRead, 1_000), 10);

        // 创建无效 v2(price_units=0)→ refresh 拒绝替换,保留 v1
        metadata
            .create_pricing_version(vec![PricingRule {
                pricing_version: 0,
                metric: UsageMetric::KvRead,
                unit_size: 1_000,
                price_units: 0,
            }])
            .await
            .unwrap();
        let changed = mgr.refresh().await.unwrap();
        assert!(!changed, "invalid config must not replace current");
        assert_eq!(mgr.current().version, 1, "仍保留 v1");
        assert_eq!(mgr.rate(UsageMetric::KvRead, 1_000), 10);
    }
}
