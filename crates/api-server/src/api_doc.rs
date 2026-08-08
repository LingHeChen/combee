//! OpenAPI 文档(设计文档 P2 §9.3):`GET /openapi.json` 作为
//! SDK / Console / Docs / contract tests 的共同机器契约。
//!
//! 只暴露 **Public API**(Data Plane + User Control Plane);
//! `/internal/*`、`/rpc/*`、`/admin/*` 不进 OpenAPI(见 docs/API.md 分层)。

use utoipa::OpenApi;

use crate::handlers::{credits, database, keys, kv, sql, usage};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Combee API",
        version = "0.1.0-alpha",
        description = "One app, one Cell. SQL + KV included.\n\n\
            Public user API: Data Plane (SQL / KV) + User Control Plane \
            (Cells / API Keys / Usage / Credits).\n\
            Auth: `x-api-key` header (`cmb_sk_...`).\n\
            Errors: `{code, error}` — code is stable across versions.\n\
            Every response carries `x-request-id`.",
    ),
    paths(
        database::create_database,
        database::list_databases,
        database::delete_database,
        sql::execute_sql,
        sql::execute_transaction,
        kv::kv_get,
        kv::kv_set,
        kv::kv_del,
        usage::usage_summary,
        usage::cell_usage,
        credits::credits_balance,
        credits::credits_redeem,
        keys::create_api_key,
    ),
    components(schemas(
        database::CreateDatabaseResponse,
        combee_metadata::DatabaseRecord,
        combee_common::protocol::SqlRequest,
        combee_common::protocol::SqlResult,
        combee_common::protocol::TransactionRequest,
        combee_common::protocol::KvSetRequest,
        combee_common::protocol::KvEntry,
        kv::KvGetResponse,
        kv::KvSetResponse,
        kv::KvDelResponse,
        usage::UsageSummary,
        usage::UsageOperations,
        usage::UsagePeriod,
        credits::CreditBalanceResponse,
        credits::RedeemRequest,
        credits::RedeemResponse,
        keys::CreateApiKeyResponse,
        combee_common::DatabaseId,
        combee_common::TenantId,
    )),
    tags(
        (name = "databases", description = "Cell lifecycle"),
        (name = "sql", description = "SQL Data Plane"),
        (name = "kv", description = "KV Data Plane"),
        (name = "usage", description = "Usage Metering"),
        (name = "credits", description = "Credits / Voucher"),
        (name = "api-keys", description = "API key management"),
    )
)]
pub struct ApiDoc;

/// 生成 OpenAPI JSON(每请求即时渲染;也可缓存为静态文件)。
pub fn to_json() -> String {
    ApiDoc::openapi()
        .to_pretty_json()
        .unwrap_or_else(|_| "{}".into())
}

/// GET /openapi.json —— 机器可读 API 契约(原始 JSON body)。
pub async fn openapi_json() -> impl axum::response::IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        to_json(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_renders_with_core_paths() {
        let doc: utoipa::openapi::OpenApi = serde_json::from_str(&to_json()).unwrap();
        assert_eq!(doc.info.title, "Combee API");
        let paths = doc.paths.paths;
        for p in [
            "/v1/databases",
            "/v1/databases/{id}/sql",
            "/v1/databases/{id}/transaction",
            "/v1/databases/{id}/kv/{key}",
            "/v1/usage/summary",
            "/v1/cells/{id}/usage",
            "/v1/credits/balance",
            "/v1/credits/redeem",
            "/v1/api-keys",
        ] {
            assert!(paths.contains_key(p), "missing path {p}");
        }
        // internal/admin 不得出现在 OpenAPI
        for p in paths.keys() {
            assert!(
                !p.starts_with("/internal") && !p.starts_with("/admin") && !p.starts_with("/rpc"),
                "internal path leaked into OpenAPI: {p}"
            );
        }
    }
}
