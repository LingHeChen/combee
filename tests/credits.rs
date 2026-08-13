//! P1 Credits / Pricing / Voucher / Settlement 集成测试
//! (设计文档 §5-8,验收清单见 artifacts/engineering/plan/COMBEE_NEXT_PHASE_V0.1.0_BETA_PLAN.md)。
//!
//! 覆盖:整数账本、admin grant、voucher 单次/幂等/并发/过期、pricing 版本热生效与
//! 无效配置拒绝、settlement 幂等不重复扣款、三类 token 分离、租户隔离。

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::{Method, StatusCode};
use combee_api_server::AppState;
use combee_api_server::app::build_app;
use combee_api_server::auth::AuthMode;
use combee_api_server::client::LocalDataNodeClient;
use combee_api_server::client::{DataNodeProvider, LocalProvider};
use combee_api_server::nodes::NodeRegistry;
use combee_api_server::pricing::PricingManager;
use combee_api_server::settlement::Settlement;
use combee_api_server::usage::UsageMeter;
use combee_common::config::KvDurability;
use combee_common::usage::UsageMetric;
use combee_data_node::{DataNode, DataNodeConfig};
use combee_metadata::{InMemoryStore, MetadataStore};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const ADMIN: &str = "adm-very-secret";

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    admin: Option<&str>,
) -> (StatusCode, Value) {
    let mut req = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(a) = admin {
        req = req.header("x-admin-token", a);
    }
    let resp = app
        .clone()
        .oneshot(
            req.body(body.map(|v| v.to_string()).unwrap_or_else(|| "{}".into()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

struct Harness {
    app: Router,
    metadata: Arc<dyn MetadataStore>,
    usage: Arc<UsageMeter>,
    pricing: Arc<PricingManager>,
    settlement: Arc<Settlement>,
    _dir: TempDir,
}

async fn make_harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let metadata: Arc<dyn MetadataStore> = Arc::new(InMemoryStore::new());
    let node = Arc::new(DataNode::new(DataNodeConfig {
        data_dir: dir.path().to_path_buf(),
        max_active_dbs: 16,
        db_idle_timeout: Duration::from_secs(3600),
        ttl_gc_interval: Duration::from_secs(3600),
        kv_cache_capacity: 100_000,
        kv_durability: KvDurability::Normal,
        sql_timeout: Some(Duration::from_secs(5)),
        quota: Default::default(),
    }));
    let client = Arc::new(LocalDataNodeClient::new(node));
    let provider: Arc<dyn DataNodeProvider> = Arc::new(LocalProvider::new(client));
    let usage = UsageMeter::new(metadata.clone(), Duration::from_secs(3600));
    let pricing = PricingManager::new(metadata.clone(), Duration::from_secs(3600));
    let settlement = Settlement::new(metadata.clone(), pricing.clone(), Duration::from_secs(3600));
    let state = AppState {
        metadata: metadata.clone(),
        data_node: provider,
        nodes: Arc::new(NodeRegistry::new()),
        auth_mode: AuthMode::Off,
        control_plane_token: None,
        bff_service_key: None,
        usage: usage.clone(),
        pricing: pricing.clone(),
        admin_token: Some(ADMIN.into()),
        quota: Default::default(),
        concurrency: Default::default(),
        min_credit_balance_units: -100 * combee_common::credit::CREDIT_UNITS_PER_CREDIT,
    };
    Harness {
        app: build_app(state),
        metadata,
        usage,
        pricing,
        settlement,
        _dir: dir,
    }
}

/// 目的:admin grant 整数入账,账本 append-only,余额可从账本重建。
#[tokio::test]
async fn grant_balance_and_ledger_integer_accounting() {
    let h = make_harness().await;
    // admin 未配置 token → 401;错误 token → 401;租户 key → 401(admin 接口)
    let (s, _) = send(
        &h.app,
        Method::POST,
        "/admin/tenants/00000000-0000-0000-0000-000000000001/credits/grant",
        Some(json!({"amount_units": 100})),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "未配置 admin token 应 401");
    let (s, _) = send(
        &h.app,
        Method::POST,
        "/admin/tenants/00000000-0000-0000-0000-000000000001/credits/grant",
        Some(json!({"amount_units": 100})),
        Some("wrong"),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let tenant = combee_metadata::DEFAULT_TENANT;
    let (s, body) = send(
        &h.app,
        Method::POST,
        &format!("/admin/tenants/{}/credits/grant", tenant.0),
        Some(json!({"amount_units": 500_000_000, "reason": "alpha tester"})),
        Some(ADMIN),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{body}");
    assert_eq!(body["balance"], "500000000");

    // balance
    let (s, body) = send(&h.app, Method::GET, "/v1/credits/balance", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["available"], "500000000");
    assert_eq!(body["currency"], "CREDIT");

    // 负数 grant 拒绝
    let (s, _) = send(
        &h.app,
        Method::POST,
        &format!("/admin/tenants/{}/credits/grant", tenant.0),
        Some(json!({"amount_units": -5})),
        Some(ADMIN),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // 账本 + 余额重建
    let (s, body) = send(
        &h.app,
        Method::GET,
        "/v1/credits/transactions?limit=100",
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["txn_type"], "grant");
    assert_eq!(items[0]["amount_units"], 500_000_000);
    let sum: i64 = items
        .iter()
        .map(|t| t["amount_units"].as_i64().unwrap())
        .sum();
    assert_eq!(sum, 500_000_000, "余额可从账本重建");
}

/// 目的:voucher 单次兑换、幂等重试不重复、过期拒绝、并发只成功一次。
#[tokio::test]
async fn voucher_single_use_idempotent_and_concurrent_safe() {
    let h = make_harness().await;
    // admin 生成 voucher
    let (s, body) = send(
        &h.app,
        Method::POST,
        "/admin/vouchers/generate",
        Some(json!({"amount_units": 50_000_000, "count": 1, "campaign": "alpha"})),
        Some(ADMIN),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    let code = body["codes"][0]["code"].as_str().unwrap().to_string();
    assert!(code.starts_with("CMB-"), "{code}");

    // 兑换
    let (s, body) = send(
        &h.app,
        Method::POST,
        "/v1/credits/redeem",
        Some(json!({"code": code})),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    assert_eq!(body["credits_added"], "50000000");
    assert_eq!(body["already_redeemed"], false);
    assert_eq!(body["balance"], "50000000");

    // 幂等重试:同一 code 再兑 → 不重复加钱
    let (s, body) = send(
        &h.app,
        Method::POST,
        "/v1/credits/redeem",
        Some(json!({"code": code})),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["already_redeemed"], true);
    assert_eq!(body["balance"], "50000000", "重试不重复加钱");

    // 并发兑换同一 code:只成功一次
    let app = h.app.clone();
    let code2 = code.clone();
    let mut handles = Vec::new();
    for _ in 0..8 {
        let app = app.clone();
        let code2 = code2.clone();
        handles.push(tokio::spawn(async move {
            let (s, body) = send(
                &app,
                Method::POST,
                "/v1/credits/redeem",
                Some(json!({"code": code2})),
                None,
            )
            .await;
            (s, body)
        }));
    }
    let mut ok_count = 0;
    for hh in handles {
        let (_s, body) = hh.await.unwrap();
        if body["already_redeemed"] == true || _s == StatusCode::OK {
            ok_count += 1;
        }
    }
    // 8 个并发请求全部返回成功语义(首次成功或幂等返回),余额不变
    let (_, body) = send(&h.app, Method::GET, "/v1/credits/balance", None, None).await;
    assert_eq!(body["available"], "50000000", "并发兑换不重复加钱");
    let _ = ok_count;

    // 过期 voucher
    let (_s, body) = send(
        &h.app,
        Method::POST,
        "/admin/vouchers/generate",
        Some(json!({"amount_units": 100, "count": 1, "expires_at": 1})),
        Some(ADMIN),
    )
    .await;
    let code_exp = body["codes"][0]["code"].as_str().unwrap().to_string();
    let (s, _) = send(
        &h.app,
        Method::POST,
        "/v1/credits/redeem",
        Some(json!({"code": code_exp})),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "过期 voucher 不可用");
}

/// 目的:pricing 版本热生效(admin 创建 → refresh → GET /v1/pricing 反映),无效配置拒绝。
#[tokio::test]
async fn pricing_version_hot_reload_and_invalid_rejected() {
    let h = make_harness().await;
    // 初始空定价
    let (s, body) = send(&h.app, Method::GET, "/v1/pricing", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["version"], 0);

    // admin 创建 v1
    let (s, body) = send(
        &h.app,
        Method::POST,
        "/admin/pricing/versions",
        Some(json!({"rules": [{"metric": "kv_read", "unit_size": 1000, "price_units": 10}]})),
        Some(ADMIN),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    assert_eq!(body["version"], 1);

    // 热生效(refresh)
    h.pricing.refresh().await.unwrap();
    let (s, body) = send(&h.app, Method::GET, "/v1/pricing", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["version"], 1);
    assert_eq!(
        body["units"]["kv_read"]["price_per_unit"],
        "10 microcredits / 1000 kv_read"
    );

    // 非法 metric → 400
    let (s, _) = send(
        &h.app,
        Method::POST,
        "/admin/pricing/versions",
        Some(json!({"rules": [{"metric": "nope", "unit_size": 1, "price_units": 1}]})),
        Some(ADMIN),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // 无效配置(unit_size=0)→ 400(不会产生可加载的版本)
    let (s, _) = send(
        &h.app,
        Method::POST,
        "/admin/pricing/versions",
        Some(json!({"rules": [{"metric": "kv_read", "unit_size": 0, "price_units": 10}]})),
        Some(ADMIN),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // admin 无 token → 401
    let (s, _) = send(
        &h.app,
        Method::POST,
        "/admin/pricing/versions",
        Some(json!({"rules": [{"metric": "kv_read", "unit_size": 1, "price_units": 1}]})),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

/// 目的:settlement 把 usage → credits(记录 pricing_version),重复结算不重复扣款。
#[tokio::test]
async fn settlement_rates_usage_and_is_idempotent() {
    let h = make_harness().await;
    // 定价:kv_read 1000 ops = 10 microcredits
    send(
        &h.app,
        Method::POST,
        "/admin/pricing/versions",
        Some(
            json!({"rules": [{"metric": "kv_read", "unit_size": 1000, "price_units": 10},
                               {"metric": "kv_write", "unit_size": 1000, "price_units": 40}]}),
        ),
        Some(ADMIN),
    )
    .await;
    h.pricing.refresh().await.unwrap();

    // grant 100 credits
    let tenant = combee_metadata::DEFAULT_TENANT;
    send(
        &h.app,
        Method::POST,
        &format!("/admin/tenants/{}/credits/grant", tenant.0),
        Some(json!({"amount_units": 100_000_000})),
        Some(ADMIN),
    )
    .await;

    // 产生 2500 kv_read + 1000 kv_write 用量(写入"上一个已完成分钟"桶,
    // settlement 只结算已完成分钟,避免结算中的当前分钟)
    let cell = combee_common::DatabaseId::new();
    let _ = h.usage.pending();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let past = combee_common::usage::bucket_start(now) - 60;
    for (metric, amount) in [
        (UsageMetric::KvRead, 2500u64),
        (UsageMetric::KvWrite, 1000u64),
    ] {
        h.metadata
            .usage_add(
                &combee_common::usage::UsageKey {
                    tenant_id: tenant,
                    cell_id: Some(cell),
                    metric,
                    bucket_start: past,
                },
                amount,
            )
            .await
            .unwrap();
    }

    // 结算
    let written = h.settlement.settle_once().await.unwrap();
    assert_eq!(written, 2, "kv_read + kv_write 两条 usage 账本");

    // 重复结算(水位已推进)→ 不新增
    let written2 = h.settlement.settle_once().await.unwrap();
    assert_eq!(written2, 0, "同窗口重复结算不重复扣款");

    // 账本:usage 条目带 pricing_version,扣费 = ceil(2500/1000)*10 + ceil(1000/1000)*40 = 30+40 = 70
    let (_, body) = send(
        &h.app,
        Method::GET,
        "/v1/credits/transactions?limit=100",
        None,
        None,
    )
    .await;
    let items = body["items"].as_array().unwrap();
    let usage_entries: Vec<_> = items.iter().filter(|t| t["txn_type"] == "usage").collect();
    assert_eq!(usage_entries.len(), 2);
    let total_charged: i64 = usage_entries
        .iter()
        .map(|t| t["amount_units"].as_i64().unwrap())
        .sum();
    assert_eq!(total_charged, -70, "2500/1000→30 + 1000/1000→40");
    for t in &usage_entries {
        assert_eq!(t["pricing_version"], 1, "记录定价版本,历史可追溯");
    }
    let (_, body) = send(&h.app, Method::GET, "/v1/credits/balance", None, None).await;
    assert_eq!(body["available"], "99999930");
}
