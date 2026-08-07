//! API key 生成与哈希 + 请求认证上下文。
//!
//! - 密钥格式:`cmb_sk_` + 64 位 hex;数据库只存 sha256 哈希(明文仅创建时返回一次);
//! - [`AuthContext`] 贯穿整个请求生命周期:认证后只传 `tenant_id`,不传原始 key。

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 密钥前缀。
pub const KEY_PREFIX: &str = "cmb_sk_";

/// 生成一个新密钥(明文)。调用方负责把 `hash(key)` 存入数据库。
pub fn generate() -> String {
    format!(
        "{KEY_PREFIX}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

/// 密钥哈希(sha256 hex)。数据库与日志中只出现哈希。
pub fn hash(key: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(key.as_bytes());
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 校验密钥格式(前缀 + 足够长度)。
pub fn is_valid_format(key: &str) -> bool {
    key.starts_with(KEY_PREFIX) && key.len() >= KEY_PREFIX.len() + 32
}

/// 认证上下文:整个请求生命周期携带的租户标识(不携带原始 key)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthContext {
    pub tenant_id: crate::TenantId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_and_hash() {
        let key = generate();
        assert!(key.starts_with(KEY_PREFIX));
        assert_eq!(key.len(), KEY_PREFIX.len() + 64);
        assert!(is_valid_format(&key));
        assert!(!is_valid_format("not-a-key"));
        assert!(!is_valid_format(&key[..10]));

        // 哈希:确定性、长度 64 hex、不含明文
        let h1 = hash(&key);
        let h2 = hash(&key);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
        assert!(!h1.contains(&key));
        assert_ne!(hash(&generate()), hash(&key));
    }
}
