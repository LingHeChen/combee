//! Combee 通用基础设施:ID 类型、错误、内部协议类型、配置。

pub mod api_key;
pub mod config;
pub mod credit;
pub mod errors;
pub mod ids;
pub mod protocol;
pub mod rpc;
pub mod usage;

pub use api_key::AuthContext;
pub use credit::{
    CreditAccount, CreditTransaction, CreditTransactionType, CreditVoucher, PricingConfig,
    PricingRule, PricingStatus, PricingVersion, VoucherStatus,
};
pub use errors::{CombeeError, Result};
pub use ids::{DatabaseId, NodeId, TenantId};
pub use usage::{UsageBucket, UsageKey, UsageMetric};
