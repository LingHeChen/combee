//! Credits 与 Pricing 协议(设计文档 P1 / COMBEE_NEXT_PHASE_V0.1.0_BETA_PLAN.md §5-8)。
//!
//! 原则:
//! - 所有金额用整数最小单位 microcredits(1 credit = 1_000_000 microcredits),杜绝浮点;
//! - 账本 append-only,修正使用 compensating transaction;
//! - Pricing 与 Usage 分离:Metering(发生了多少)vs Rating(按版本计价值多少);
//! - 每笔 usage 结算必须记录 pricing_version,历史可重放。

use serde::{Deserialize, Serialize};

use crate::{TenantId, usage::UsageMetric};

/// 1 Credit = 1_000_000 microcredits。
pub const CREDIT_UNITS_PER_CREDIT: i64 = 1_000_000;

/// 1 GB·h 对应的「字节·秒」数:1e9 字节 × 3600 秒(十进制 GB,计费展示口径)。
/// 用作 `StorageByteSecs` 定价规则的 `unit_size`。
pub const BYTE_SECS_PER_GB_HOUR: i64 = 3_600_000_000_000;

/// 存储计费默认单价:0.01 credit / GB·h(= 10_000 microcredits)。
/// 可用 `COMBEE_STORAGE_PRICE_MICROCREDITS_PER_GB_HOUR` 覆盖;仅在新部署播种默认定价时生效。
pub const DEFAULT_STORAGE_PRICE_UNITS_PER_GB_HOUR: i64 = 10_000;

/// 账本条目类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CreditTransactionType {
    Recharge,
    Usage,
    Grant,
    Voucher,
    Refund,
    Adjustment,
}

impl CreditTransactionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            CreditTransactionType::Recharge => "recharge",
            CreditTransactionType::Usage => "usage",
            CreditTransactionType::Grant => "grant",
            CreditTransactionType::Voucher => "voucher",
            CreditTransactionType::Refund => "refund",
            CreditTransactionType::Adjustment => "adjustment",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "recharge" => CreditTransactionType::Recharge,
            "usage" => CreditTransactionType::Usage,
            "grant" => CreditTransactionType::Grant,
            "voucher" => CreditTransactionType::Voucher,
            "refund" => CreditTransactionType::Refund,
            "adjustment" => CreditTransactionType::Adjustment,
            _ => return None,
        })
    }
}

/// 一条账本记录(append-only,不 UPDATE)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditTransaction {
    pub id: uuid::Uuid,
    pub tenant_id: TenantId,
    pub txn_type: CreditTransactionType,
    /// 金额(microcredits;usage/refund/adjustment 可为负)。
    pub amount_units: i64,
    /// 结算时采用的定价版本(usage 类必填;其余可空)。
    pub pricing_version: Option<i64>,
    /// 幂等引用(如 usage 聚合键 / voucher code hash);同一 reference 只结算一次。
    pub reference_id: Option<String>,
    pub description: Option<String>,
    pub created_at: i64,
    /// 入账后余额(microcredits)。
    pub balance_after: Option<i64>,
}

/// 租户余额账户。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditAccount {
    pub tenant_id: TenantId,
    /// 可用余额(microcredits)。
    pub balance_units: i64,
    /// 预留(计费中;beta 第一版恒 0)。
    pub reserved_units: i64,
    pub updated_at: i64,
}

/// 兑换券状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VoucherStatus {
    Active,
    Used,
    Expired,
    Revoked,
}

impl VoucherStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            VoucherStatus::Active => "active",
            VoucherStatus::Used => "used",
            VoucherStatus::Expired => "expired",
            VoucherStatus::Revoked => "revoked",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "active" => VoucherStatus::Active,
            "used" => VoucherStatus::Used,
            "expired" => VoucherStatus::Expired,
            "revoked" => VoucherStatus::Revoked,
            _ => return None,
        })
    }
}

/// 兑换券(数据库只存 code 的哈希)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditVoucher {
    pub id: uuid::Uuid,
    /// `CMB-XXXX-XXXX-XXXX` 的 sha256 hex(前 16 位即可区分)。
    pub code_hash: String,
    pub amount_units: i64,
    pub status: VoucherStatus,
    pub campaign: Option<String>,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub redeemed_by: Option<TenantId>,
    pub redeemed_at: Option<i64>,
}

/// Pricing 版本状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PricingStatus {
    Active,
    Inactive,
}

impl PricingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PricingStatus::Active => "active",
            PricingStatus::Inactive => "inactive",
        }
    }
}

/// 定价版本(元数据)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingVersion {
    pub version: i64,
    pub status: PricingStatus,
    pub effective_at: i64,
    pub created_at: i64,
}

/// 单条定价规则:每 `unit_size` 次(或字节)计量消耗 `price_units` microcredits。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingRule {
    pub pricing_version: i64,
    pub metric: UsageMetric,
    pub unit_size: i64,
    pub price_units: i64,
}

/// 内存中的定价配置(热切换单元;不可变,Arc 原子替换)。
#[derive(Debug, Clone)]
pub struct PricingConfig {
    pub version: i64,
    pub effective_at: i64,
    /// metric → (unit_size, price_units)
    pub rules: std::collections::HashMap<UsageMetric, (i64, i64)>,
}

impl PricingConfig {
    /// 为空(未配置定价)——结算 0。
    pub fn empty() -> Self {
        Self {
            version: 0,
            effective_at: 0,
            rules: Default::default(),
        }
    }

    /// 把用量折算为 microcredits(向上取整到整数计费单位)。
    /// 未配置的 metric 计 0。
    pub fn rate(&self, metric: UsageMetric, units: u64) -> i64 {
        let Some((unit_size, price_units)) = self.rules.get(&metric).copied() else {
            return 0;
        };
        if unit_size <= 0 || price_units <= 0 || units == 0 {
            return 0;
        }
        let groups = units.div_ceil(unit_size as u64);
        (groups as i64).saturating_mul(price_units)
    }
}

/// 创建兑换码:CMB-XXXX-XXXX-XXXX(大写字母+数字,去掉易混淆字符)。
pub fn generate_voucher_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    let mut code = String::from("CMB-");
    for i in 0..12 {
        if i > 0 && i % 4 == 0 {
            code.push('-');
        }
        let idx = rand::Rng::gen_range(&mut rng, 0..ALPHABET.len());
        code.push(ALPHABET[idx] as char);
    }
    code
}

/// 兑换码 → 哈希(仅存哈希;sha256 hex 前 32 字符足够区分)。
pub fn hash_voucher_code(code: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(code.as_bytes());
    let out = h.finalize();
    out.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pricing_rate_rounds_up_and_missing_is_zero() {
        let mut rules = std::collections::HashMap::new();
        rules.insert(UsageMetric::KvRead, (1_000, 10));
        let cfg = PricingConfig {
            version: 7,
            effective_at: 0,
            rules,
        };
        assert_eq!(cfg.rate(UsageMetric::KvRead, 0), 0);
        assert_eq!(cfg.rate(UsageMetric::KvRead, 1_000), 10);
        assert_eq!(cfg.rate(UsageMetric::KvRead, 1_001), 20, "向上取整");
        assert_eq!(cfg.rate(UsageMetric::KvRead, 500), 10);
        assert_eq!(
            cfg.rate(UsageMetric::SqlWrite, 99),
            0,
            "未配置的 metric 计 0"
        );
    }

    #[test]
    fn storage_gb_hour_rate_rounds_up() {
        let mut rules = std::collections::HashMap::new();
        rules.insert(
            UsageMetric::StorageByteSecs,
            (BYTE_SECS_PER_GB_HOUR, 10_000),
        );
        let cfg = PricingConfig {
            version: 1,
            effective_at: 0,
            rules,
        };
        assert_eq!(
            cfg.rate(UsageMetric::StorageByteSecs, BYTE_SECS_PER_GB_HOUR as u64),
            10_000,
            "3.6e12 字节·秒 = 1 GB·h"
        );
        assert_eq!(
            cfg.rate(
                UsageMetric::StorageByteSecs,
                BYTE_SECS_PER_GB_HOUR as u64 + 1
            ),
            20_000,
            "多 1 字节·秒向上取整到 2 GB·h"
        );
        assert_eq!(cfg.rate(UsageMetric::StorageByteSecs, 0), 0);
    }

    #[test]
    fn voucher_code_format_and_hash() {
        let code = generate_voucher_code();
        assert!(code.starts_with("CMB-"), "{code}");
        assert_eq!(code.len(), 18, "CMB-XXXX-XXXX-XXXX");
        let h1 = hash_voucher_code(&code);
        let h2 = hash_voucher_code(&code);
        assert_eq!(h1, h2, "同码同哈希");
        assert_ne!(h1, hash_voucher_code("CMB-AAAA-AAAA-AAAA"));
        assert_eq!(h1.len(), 32);
    }

    #[test]
    fn transaction_type_roundtrip() {
        for t in [
            CreditTransactionType::Recharge,
            CreditTransactionType::Usage,
            CreditTransactionType::Grant,
            CreditTransactionType::Voucher,
            CreditTransactionType::Refund,
            CreditTransactionType::Adjustment,
        ] {
            assert_eq!(CreditTransactionType::parse(t.as_str()), Some(t));
        }
    }
}
