//! HTTP 路由。

use axum::Router;
use axum::middleware;
use axum::routing::{delete, get, post};
use tower_http::trace::TraceLayer;

use crate::AppState;
use crate::auth;
use crate::handlers::{backup, database, failover, internal, keys, kv, replication, sql};

pub fn build_app(state: AppState) -> Router {
    // public 路由:走租户 key 认证(auth_middleware)
    let public = Router::new()
        .route(
            "/v1/databases",
            get(database::list_databases).post(database::create_database),
        )
        .route("/v1/databases/{id}", delete(database::delete_database))
        .route("/v1/databases/{id}/sql", post(sql::execute_sql))
        .route("/v1/databases/{id}/backup", post(backup::backup))
        .route(
            "/v1/databases/{id}/backup/incr",
            post(backup::incremental_backup),
        )
        .route("/v1/databases/{id}/restore", post(backup::restore))
        .route(
            "/v1/databases/{id}/transaction",
            post(sql::execute_transaction),
        )
        .route(
            "/v1/databases/{id}/kv/{key}",
            get(kv::kv_get).put(kv::kv_set).delete(kv::kv_del),
        )
        // 操作端点统一放在 /kv/ops/* 下,避免与任意 key 名冲突
        // (若直接放在 /kv/exists 等,同名 key 的 GET/PUT 会因静态路由优先返回 405)。
        .route("/v1/databases/{id}/kv/ops/exists", post(kv::kv_exists))
        .route("/v1/databases/{id}/kv/ops/mget", post(kv::kv_mget))
        .route("/v1/databases/{id}/kv/ops/mset", post(kv::kv_mset))
        .route("/v1/databases/{id}/kv/ops/ttl", post(kv::kv_ttl))
        .route("/v1/databases/{id}/kv/ops/expire", post(kv::kv_expire))
        .route("/v1/databases/{id}/kv/ops/incr", post(kv::kv_incr))
        .route(
            "/v1/tenants",
            post(keys::create_tenant).get(keys::list_tenants),
        )
        .route(
            "/v1/api-keys",
            post(keys::create_api_key).get(keys::list_api_keys),
        )
        .route("/v1/api-keys/{id}", delete(keys::revoke_api_key))
        .route("/v1/databases/{id}/failover", post(failover::failover))
        .route(
            "/v1/databases/{id}/replication",
            get(replication::get_replica)
                .post(replication::set_replica)
                .delete(replication::unset_replica),
        );

    // internal 控制面路由:独立认证(internal_auth),不经过租户 key 中间件
    let internal = Router::new()
        .route("/internal/nodes/register", post(internal::register))
        .route("/internal/nodes/heartbeat", post(internal::heartbeat))
        .route("/internal/nodes/unregister", post(internal::unregister))
        .route("/internal/nodes", get(internal::list))
        .route("/internal/nodes/{node}/replicas", get(internal::replicas))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::internal_auth,
        ));

    public
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .merge(internal)
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}
