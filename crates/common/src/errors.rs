//! Combee 统一错误类型。

use crate::ids::DatabaseId;

#[derive(Debug, thiserror::Error)]
pub enum CombeeError {
    #[error("database not found: {0}")]
    DatabaseNotFound(DatabaseId),

    #[error("database already exists: {0}")]
    DatabaseAlreadyExists(DatabaseId),

    #[error("api key not found or already revoked")]
    ApiKeyNotFound,

    #[error("unauthorized")]
    Unauthorized,

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("quota exceeded: {0}")]
    QuotaExceeded(String),

    #[error("sql error: {0}")]
    Sql(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl CombeeError {
    /// 稳定的错误分类标识(跨进程 RPC 传输用;`from_kind` 反向还原)。
    pub fn kind(&self) -> &'static str {
        match self {
            CombeeError::DatabaseNotFound(_) => "database_not_found",
            CombeeError::DatabaseAlreadyExists(_) => "database_already_exists",
            CombeeError::Unauthorized => "unauthorized",
            CombeeError::ApiKeyNotFound => "api_key_not_found",
            CombeeError::InvalidRequest(_) => "invalid_request",
            CombeeError::Forbidden(_) => "forbidden",
            CombeeError::QuotaExceeded(_) => "quota_exceeded",
            CombeeError::Sql(_) => "sql",
            CombeeError::Internal(_) => "internal",
        }
    }

    /// 由分类 + 消息还原错误(RPC 反序列化)。
    /// 带 id 的错误:从消息尾部提取 UUID(仅用于分类;精确 id 以请求路径为准)。
    pub fn from_kind(kind: &str, message: String) -> Self {
        let tail_id = || {
            message
                .split_whitespace()
                .last()
                .and_then(|s| s.parse::<DatabaseId>().ok())
        };
        match kind {
            "database_not_found" => {
                CombeeError::DatabaseNotFound(tail_id().unwrap_or_else(DatabaseId::new))
            }
            "database_already_exists" => {
                CombeeError::DatabaseAlreadyExists(tail_id().unwrap_or_else(DatabaseId::new))
            }
            "unauthorized" => CombeeError::Unauthorized,
            "api_key_not_found" => CombeeError::ApiKeyNotFound,
            "invalid_request" => CombeeError::InvalidRequest(message),
            "forbidden" => CombeeError::Forbidden(message),
            "quota_exceeded" => CombeeError::QuotaExceeded(message),
            "sql" => CombeeError::Sql(message),
            _ => CombeeError::Internal(message),
        }
    }
}

pub type Result<T> = std::result::Result<T, CombeeError>;
