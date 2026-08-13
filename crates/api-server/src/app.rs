//! HTTP 路由。

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::routing::{delete, get, post, put};
use tower_http::trace::TraceLayer;

use crate::AppState;
use crate::auth;
use crate::handlers::{
    admin, backup, credits, database, failover, health, internal, keys, kv, replication, sql,
    usage, waitlist,
};

pub fn build_app(state: AppState) -> Router {
    // 探活/就绪:不挂租户认证(供外部探针、Swarm healthcheck、告警使用)。
    let health_routes = Router::new()
        .route(
            "/health",
            get(|| async { axum::Json(serde_json::json!({"status": "ok"})) }),
        )
        .route("/ready", get(health::ready))
        .with_state(state.clone());

    // public 路由:走租户 key 认证(auth_middleware)
    let public = Router::new()
        .route(
            "/v1/databases",
            get(database::list_databases).post(database::create_database),
        )
        .route(
            "/v1/databases/by-name/{name}",
            put(database::ensure_database)
                .get(database::get_database_by_name)
                .delete(database::delete_database_by_name),
        )
        .route(
            "/v1/databases/{id}",
            get(database::get_database)
                .delete(database::delete_database)
                .patch(database::rename_database),
        )
        .route("/v1/databases/{id}/reset", post(database::reset_database))
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
        .route("/v1/databases/{id}/kv", get(kv::kv_list))
        .route("/v1/databases/{id}/kv/ops/exists", post(kv::kv_exists))
        .route("/v1/databases/{id}/kv/ops/mget", post(kv::kv_mget))
        .route("/v1/databases/{id}/kv/ops/mset", post(kv::kv_mset))
        .route("/v1/databases/{id}/kv/ops/ttl", post(kv::kv_ttl))
        .route("/v1/databases/{id}/kv/ops/expire", post(kv::kv_expire))
        .route("/v1/databases/{id}/kv/ops/incr", post(kv::kv_incr))
        .route(
            "/v1/api-keys",
            post(keys::create_api_key).get(keys::list_api_keys),
        )
        .route("/v1/api-keys/{id}", delete(keys::revoke_api_key))
        .route("/v1/usage/summary", get(usage::usage_summary))
        .route("/v1/usage/timeseries", get(usage::usage_timeseries))
        .route(
            "/v1/waitlist",
            post(waitlist::join).layer(waitlist::waitlist_cors()),
        )
        .route("/v1/cells/{id}/usage", get(usage::cell_usage))
        .route("/v1/credits/balance", get(credits::credits_balance))
        .route(
            "/v1/credits/transactions",
            get(credits::credits_transactions),
        )
        .route("/v1/credits/redeem", post(credits::credits_redeem))
        .route("/v1/pricing", get(credits::get_pricing))
        .route("/openapi.json", get(crate::api_doc::openapi_json))
        .route("/v1/databases/{id}/failover", post(failover::failover))
        .route(
            "/v1/databases/{id}/replication",
            get(replication::get_replica)
                .post(replication::set_replica)
                .delete(replication::unset_replica),
        );

    // admin 路由:COMBEE_ADMIN_TOKEN(与租户 key / control-plane token 分离)
    let admin_routes = Router::new()
        .route(
            "/admin/tenants/{tenant}/credits/grant",
            post(admin::admin_grant_credits),
        )
        .route(
            "/admin/vouchers/generate",
            post(admin::admin_generate_vouchers),
        )
        .route("/admin/tenants", post(admin::admin_create_tenant))
        .route("/admin/vouchers", get(admin::admin_list_vouchers))
        .route("/admin/waitlist", get(waitlist::admin_list))
        .route("/admin/cells/{id}/migrate", post(admin::admin_migrate_cell))
        .route(
            "/admin/pricing/versions",
            post(admin::admin_create_pricing_version),
        )
        .route(
            "/admin/pricing/versions",
            get(admin::admin_list_pricing_versions),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::admin_auth,
        ));

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
        .layer(DefaultBodyLimit::max(state.quota.max_request_body_bytes))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::quota::concurrency_quota,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::logging::request_logging,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::usage::usage_tracking,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::quota::credit_quota,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .merge(admin_routes)
        .merge(internal)
        .merge(health_routes)
        .with_state(state)
        .layer(middleware::from_fn(auth::request_id))
        .layer(TraceLayer::new_for_http())
}
