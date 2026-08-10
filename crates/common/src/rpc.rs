//! API Server ↔ Data Node 之间的内部 RPC 协议(设计文档 §17,V0 用 HTTP JSON)。
//!
//! 请求体复用 `crate::protocol` 的业务类型,外面包一层 `db` 定位目标 Cell;
//! 响应统一包成 [`RpcResponse`](RpcResponse),错误跨进程还原为
//! [`CombeeError`](crate::CombeeError)(通过 `kind` 分类,见 `CombeeError::kind`)。

use serde::{Deserialize, Serialize};

use crate::errors::CombeeError;
use crate::ids::DatabaseId;
use crate::protocol::{
    KvExpireRequest, KvIncrRequest, KvSetItem, KvSetRequest, SqlRequest, TransactionRequest,
};

macro_rules! rpc_req {
    ($name:ident, { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct $name {
            $(pub $field: $ty,)*
        }
    };
}

rpc_req!(RpcDb, { db: DatabaseId });

rpc_req!(RpcSql, { db: DatabaseId, req: SqlRequest, generation: i64 });

rpc_req!(RpcTransaction, { db: DatabaseId, req: TransactionRequest, generation: i64 });

rpc_req!(RpcKvGet, { db: DatabaseId, key: String });

// KV 扫描(浏览):按 key 前缀列出 key,`cursor` 为上一页最后一个 key。
rpc_req!(RpcKvScan, {
    db: DatabaseId,
    prefix: String,
    limit: u32,
    cursor: String,
});

/// KV 扫描结果:keys 已按字典序排列;`next_cursor` 为空表示已到末尾。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcKvScanResult {
    pub keys: Vec<String>,
    pub next_cursor: String,
}

rpc_req!(RpcKvSet, { db: DatabaseId, key: String, req: KvSetRequest, generation: i64 });

rpc_req!(RpcKvKeys, { db: DatabaseId, keys: Vec<String> });

rpc_req!(RpcKvSetItems, { db: DatabaseId, items: Vec<KvSetItem>, generation: i64 });

rpc_req!(RpcKvIncr, { db: DatabaseId, req: KvIncrRequest, generation: i64 });

rpc_req!(RpcKvExpire, { db: DatabaseId, req: KvExpireRequest, generation: i64 });

/// 一次备份快照的信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    /// 对象存储中的对象 key。
    pub key: String,
    /// 快照大小(字节)。
    pub size_bytes: u64,
    /// 创建时间(unix 秒)。
    pub created_at: u64,
    /// 快照内容 sha256(hex);旧版本备份可能缺失。
    pub checksum: Option<String>,
}

/// 恢复请求:db + 可选快照版本(对象 key;缺省取最新)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRestore {
    pub db: DatabaseId,
    #[serde(default)]
    pub version: Option<String>,
}

/// Data Node 注册请求(内部节点管理)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRegisterRequest {
    /// 节点 ID;`None` 时由 API Server 分配,`Some` 时按指定 id 注册(重启保持身份)。
    #[serde(default)]
    pub id: Option<crate::ids::NodeId>,
    /// 内部 RPC base URL(API Server 用它路由到该节点)。
    pub addr: String,
    /// 上报的连接容量上限(monitoring)。
    pub capacity: usize,
}

/// Data Node 注册响应:分配节点 ID。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRegisterResponse {
    pub id: crate::ids::NodeId,
}

/// 心跳请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeHeartbeatRequest {
    pub id: crate::ids::NodeId,
    /// 当前打开的 SQLite 连接数(monitoring)。
    pub active_conns: usize,
}

/// 注销请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeUnregisterRequest {
    pub id: crate::ids::NodeId,
}

/// RPC 响应包装:成功带 data,失败带错误分类与消息。
/// HTTP 状态恒为 200,错误语义在 body 内 —— 客户端据此还原 `CombeeError`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum RpcResponse<T> {
    Ok { data: T },
    Err { kind: String, message: String },
}

impl<T> RpcResponse<T> {
    pub fn from_result(result: Result<T, CombeeError>) -> Self {
        match result {
            Ok(data) => RpcResponse::Ok { data },
            Err(e) => RpcResponse::Err {
                kind: e.kind().to_string(),
                message: e.to_string(),
            },
        }
    }

    pub fn into_result(self) -> Result<T, CombeeError> {
        match self {
            RpcResponse::Ok { data } => Ok(data),
            RpcResponse::Err { kind, message } => Err(CombeeError::from_kind(&kind, message)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CombeeError;

    #[test]
    fn error_kind_roundtrip() {
        let cases: Vec<CombeeError> = vec![
            CombeeError::DatabaseNotFound(DatabaseId::new()),
            CombeeError::DatabaseAlreadyExists(DatabaseId::new()),
            CombeeError::Unauthorized,
            CombeeError::InvalidRequest("bad".into()),
            CombeeError::Forbidden("no".into()),
            CombeeError::QuotaExceeded("full".into()),
            CombeeError::Sql("syntax".into()),
            CombeeError::Internal("boom".into()),
        ];
        for e in cases {
            let back = CombeeError::from_kind(e.kind(), e.to_string());
            assert_eq!(back.kind(), e.kind(), "kind mismatch for {e:?}");
        }
    }

    #[test]
    fn rpc_response_roundtrip() {
        let ok: RpcResponse<u64> = RpcResponse::from_result(Ok(42));
        let json = serde_json::to_string(&ok).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        let back: RpcResponse<u64> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.into_result().unwrap(), 42);

        let err: RpcResponse<u64> =
            RpcResponse::from_result(Err(CombeeError::InvalidRequest("bad".into())));
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"status\":\"err\""));
        let back: RpcResponse<u64> = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            back.into_result(),
            Err(CombeeError::InvalidRequest(_))
        ));
    }
}

rpc_req!(RpcKvDel, { db: DatabaseId, key: String, generation: i64 });

rpc_req!(RpcFence, { db: DatabaseId, generation: i64 });
