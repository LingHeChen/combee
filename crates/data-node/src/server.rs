//! Data Node 独立进程:内部 RPC 服务(设计文档 §17,V0 用 HTTP JSON)。
//!
//! 端点(`/rpc/*`)与 `DataNodeClient` trait 一一对应,请求体见 `common::rpc`,
//! 响应统一包成 `RpcResponse`(错误通过 `kind` 跨进程还原)。
//! 监听地址由 `COMBEE_DATA_NODE_ADDR` 控制(默认 `0.0.0.0:9000`)。

use std::sync::Arc;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use combee_common::protocol::{KvEntry, SqlResult};
use combee_common::rpc::{
    BackupInfo, RpcDb, RpcFence, RpcKvDel, RpcKvExpire, RpcKvGet, RpcKvIncr, RpcKvKeys, RpcKvSet,
    RpcKvSetItems, RpcResponse, RpcRestore, RpcSql, RpcTransaction,
};

use crate::DataNode;

/// 内部 RPC 认证:与 API Server 的 `internal_auth` 同规则。
///
/// 1. 携带租户 `x-api-key` 一律拒绝;
/// 2. 配置 token 时必须提供 `Authorization: Bearer <token>` 或 `x-control-token: <token>`;
/// 3. 未配置则放行。
pub(crate) async fn rpc_request_id(
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if let Some(rid) = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
    {
        // RPC 层结构化日志:request_id 已从 API/BFF 贯穿到 DataNode
        tracing::debug!(
            service = "combee-data-node",
            event = "rpc.request",
            request_id = %rid,
            path = %req.uri(),
        );
        let span = tracing::info_span!("rpc", request_id = %rid);
        let _g = span.enter();
        return next.run(req).await;
    }
    next.run(req).await
}

pub(crate) async fn rpc_auth(
    State(token): State<Option<String>>,
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if req.headers().contains_key("x-api-key") {
        tracing::warn!(
            service = "combee-data-node",
            event = "rpc.unauthorized",
            path = %req.uri(),
            reason = "tenant api key on internal rpc",
        );
        return (axum::http::StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    if let Some(expected) = &token {
        let bearer_ok = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|t| t == expected)
            .unwrap_or(false);
        let header_ok = req
            .headers()
            .get("x-control-token")
            .and_then(|v| v.to_str().ok())
            .map(|t| t == expected)
            .unwrap_or(false);
        if !bearer_ok && !header_ok {
            tracing::warn!(
                service = "combee-data-node",
                event = "rpc.unauthorized",
                path = %req.uri(),
                reason = "control token mismatch",
                provided = req.headers().get("x-control-token").and_then(|v| v.to_str().ok()).map(|s| s.chars().take(8).collect::<String>()),
            );
            return (axum::http::StatusCode::UNAUTHORIZED, "unauthorized").into_response();
        }
    }
    next.run(req).await
}

/// 构建内部 RPC 路由(供 bin 与测试复用)。`control_token` 为空表示 dev 放行。
pub fn router(node: Arc<DataNode>, control_token: Option<String>) -> Router {
    let rpc = Router::new()
        .route("/rpc/execute_sql", post(rpc_execute_sql))
        .route("/rpc/execute_transaction", post(rpc_execute_transaction))
        .route("/rpc/kv_get", post(rpc_kv_get))
        .route("/rpc/kv_set", post(rpc_kv_set))
        .route("/rpc/kv_del", post(rpc_kv_del))
        .route("/rpc/kv_exists", post(rpc_kv_exists))
        .route("/rpc/kv_mget", post(rpc_kv_mget))
        .route("/rpc/kv_mset", post(rpc_kv_mset))
        .route("/rpc/kv_ttl", post(rpc_kv_ttl))
        .route("/rpc/kv_expire", post(rpc_kv_expire))
        .route("/rpc/kv_incr", post(rpc_kv_incr))
        .route("/rpc/ensure_database", post(rpc_ensure_database))
        .route("/rpc/delete_database", post(rpc_delete_database))
        .route("/rpc/fence_cell", post(rpc_fence_cell))
        .route("/rpc/replicate", post(rpc_replicate))
        .route("/rpc/backup", post(rpc_backup))
        .route("/rpc/incremental_backup", post(rpc_incremental_backup))
        .route("/rpc/restore", post(rpc_restore))
        .route("/rpc/storage_bytes", post(rpc_storage_bytes))
        .route("/rpc/kv_scan", post(rpc_kv_scan))
        .route("/rpc/reset_database", post(rpc_reset_database))
        .layer(axum::middleware::from_fn(rpc_request_id))
        .layer(axum::middleware::from_fn_with_state(
            control_token.clone(),
            rpc_auth,
        ))
        .with_state(node.clone());

    // 探活/就绪:不经过 control-token 认证(供外部探针/Swarm healthcheck)。
    Router::new()
        .route("/health", get(|| async { axum::Json(serde_json::json!({"status": "ok"})) }))
        .route("/ready", get(move || {
            let n = node.clone();
            async move {
                if n.ready().await {
                    axum::Json(serde_json::json!({"status": "ok"})).into_response()
                } else {
                    (
                        axum::http::StatusCode::SERVICE_UNAVAILABLE,
                        axum::Json(serde_json::json!({"status": "not_ready", "reason": "storage_not_writable"})),
                    ).into_response()
                }
            }
        }))
        .merge(rpc)
}

/// 启动内部 RPC 服务(阻塞)。
pub async fn serve(
    node: Arc<DataNode>,
    addr: std::net::SocketAddr,
    control_token: Option<String>,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("data node rpc listening on http://{addr}");
    // 优雅关闭:收到 SIGTERM/Ctrl+C 后停止接新请求,drain 在途请求,
    // serve 返回后由 main 执行 unregister + WAL checkpoint。
    axum::serve(listener, router(node, control_token))
        .with_graceful_shutdown(shutdown_signal())
        .await
}

/// SIGTERM / Ctrl+C → 停止接新请求,等待在途请求完成。
pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received, draining requests");
}

// ---- handlers ----

async fn rpc_execute_sql(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcSql>,
) -> Json<RpcResponse<SqlResult>> {
    let r = node.execute_sql(rpc.db, rpc.req, rpc.generation).await;
    Json(RpcResponse::from_result(r))
}

async fn rpc_execute_transaction(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcTransaction>,
) -> Json<RpcResponse<Vec<SqlResult>>> {
    let r = node
        .execute_transaction(rpc.db, rpc.req, rpc.generation)
        .await;
    Json(RpcResponse::from_result(r))
}

async fn rpc_kv_get(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcKvGet>,
) -> Json<RpcResponse<Option<KvEntry>>> {
    let r = node.kv_get(rpc.db, rpc.key).await;
    Json(RpcResponse::from_result(r))
}

async fn rpc_kv_set(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcKvSet>,
) -> Json<RpcResponse<bool>> {
    let r = node
        .kv_set(
            rpc.db,
            rpc.key,
            rpc.req.value,
            rpc.req.ttl_seconds,
            rpc.req.nx,
            rpc.req.xx,
            rpc.generation,
        )
        .await;
    Json(RpcResponse::from_result(r))
}

async fn rpc_kv_del(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcKvDel>,
) -> Json<RpcResponse<bool>> {
    let r = node.kv_del(rpc.db, rpc.key, rpc.generation).await;
    Json(RpcResponse::from_result(r))
}

async fn rpc_kv_exists(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcKvGet>,
) -> Json<RpcResponse<bool>> {
    let r = node.kv_exists(rpc.db, rpc.key).await;
    Json(RpcResponse::from_result(r))
}

async fn rpc_kv_mget(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcKvKeys>,
) -> Json<RpcResponse<Vec<Option<String>>>> {
    let r = node.kv_mget(rpc.db, rpc.keys).await;
    Json(RpcResponse::from_result(r))
}

async fn rpc_kv_mset(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcKvSetItems>,
) -> Json<RpcResponse<()>> {
    let r = node.kv_mset(rpc.db, rpc.items, rpc.generation).await;
    Json(RpcResponse::from_result(r))
}

async fn rpc_kv_ttl(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcKvGet>,
) -> Json<RpcResponse<Option<i64>>> {
    let r = node.kv_ttl(rpc.db, rpc.key).await;
    Json(RpcResponse::from_result(r))
}

async fn rpc_kv_expire(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcKvExpire>,
) -> Json<RpcResponse<bool>> {
    let r = node
        .kv_expire(rpc.db, rpc.req.key, rpc.req.ttl_seconds, rpc.generation)
        .await;
    Json(RpcResponse::from_result(r))
}

async fn rpc_kv_incr(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcKvIncr>,
) -> Json<RpcResponse<i64>> {
    let r = node
        .kv_incr(
            rpc.db,
            rpc.req.key,
            rpc.req.delta,
            rpc.req.ttl_seconds,
            rpc.generation,
        )
        .await;
    Json(RpcResponse::from_result(r))
}

async fn rpc_fence_cell(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcFence>,
) -> Json<RpcResponse<()>> {
    node.fence_cell(rpc.db, rpc.generation);
    Json(RpcResponse::from_result(Ok(())))
}

async fn rpc_replicate(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcDb>,
) -> Json<RpcResponse<bool>> {
    let r = node.replicate_from_primary(rpc.db).await;
    Json(RpcResponse::from_result(r))
}

async fn rpc_ensure_database(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcDb>,
) -> Json<RpcResponse<()>> {
    let r = node.ensure_database(rpc.db).await;
    Json(RpcResponse::from_result(r))
}

async fn rpc_delete_database(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcDb>,
) -> Json<RpcResponse<()>> {
    let r = node.delete_database(rpc.db).await;
    Json(RpcResponse::from_result(r))
}

async fn rpc_incremental_backup(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcDb>,
) -> Json<RpcResponse<combee_common::rpc::BackupInfo>> {
    let r = node.incremental_backup(rpc.db).await;
    Json(RpcResponse::from_result(r))
}

async fn rpc_backup(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcDb>,
) -> Json<RpcResponse<BackupInfo>> {
    let r = node.backup(rpc.db).await;
    Json(RpcResponse::from_result(r))
}

async fn rpc_restore(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<RpcRestore>,
) -> Json<RpcResponse<()>> {
    let r = node.restore(rpc.db, rpc.version).await;
    Json(RpcResponse::from_result(r))
}

/// RPC:Cell 磁盘占用。
async fn rpc_storage_bytes(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<combee_common::rpc::RpcDb>,
) -> Json<RpcResponse<u64>> {
    let r = node.storage_bytes(rpc.db).await;
    Json(RpcResponse::from_result(r))
}

/// RPC:KV 前缀扫描(浏览)。
async fn rpc_kv_scan(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<combee_common::rpc::RpcKvScan>,
) -> Json<RpcResponse<combee_common::rpc::RpcKvScanResult>> {
    let r = node
        .kv_scan(rpc.db, rpc.prefix, rpc.limit, rpc.cursor)
        .await;
    Json(RpcResponse::from_result(r))
}

/// RPC:重置 Cell 数据(删除文件与缓存)。
async fn rpc_reset_database(
    State(node): State<Arc<DataNode>>,
    Json(rpc): Json<combee_common::rpc::RpcDb>,
) -> Json<RpcResponse<()>> {
    let r = node.reset_database(rpc.db).await;
    Json(RpcResponse::from_result(r))
}
