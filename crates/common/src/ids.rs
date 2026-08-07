//! 全局唯一标识类型。
//!
//! V0 使用 UUID v4。`DatabaseId` / `TenantId` 使用 newtype 包裹,
//! 避免在 API 边界上把"数据库 ID"与"租户 ID"混用。

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 一个逻辑数据库 / Cell 的 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DatabaseId(pub Uuid);

/// 一个租户(应用所有者)的 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TenantId(pub Uuid);

/// 一个 Data Node(数据面节点)的 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub Uuid);

impl DatabaseId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for DatabaseId {
    fn default() -> Self {
        Self::new()
    }
}

impl TenantId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn from_u128(n: u128) -> Self {
        Self(Uuid::from_u128(n))
    }
}

impl Default for TenantId {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for NodeId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::from_str(s).map(NodeId)
    }
}

impl fmt::Display for DatabaseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for DatabaseId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::from_str(s).map(DatabaseId)
    }
}

impl FromStr for TenantId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::from_str(s).map(TenantId)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "01234567-89ab-cdef-0123-456789abcdef";

    #[test]
    fn parse_and_display_roundtrip() {
        let db = DatabaseId::from_str(VALID).unwrap();
        assert_eq!(db.to_string(), VALID);
        let tenant = TenantId::from_str(VALID).unwrap();
        assert_eq!(tenant.to_string(), VALID);
    }

    #[test]
    fn invalid_uuid_rejected() {
        assert!(DatabaseId::from_str("not-a-uuid").is_err());
        assert!(DatabaseId::from_str("").is_err());
        assert!(TenantId::from_str("01234567").is_err());
    }

    #[test]
    fn new_ids_are_unique() {
        let a = DatabaseId::new();
        let b = DatabaseId::new();
        assert_ne!(a, b);
        assert_ne!(a.0, b.0);
    }

    #[test]
    fn serde_roundtrip_as_plain_string() {
        let db = DatabaseId::new();
        let json = serde_json::to_string(&db).unwrap();
        assert_eq!(json, format!("\"{}\"", db.0));
        let back: DatabaseId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, db);
    }
}
