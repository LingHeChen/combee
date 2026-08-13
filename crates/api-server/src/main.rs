//! Combee API Server 入口。

use std::sync::Arc;

use combee_api_server::AppState;
use combee_api_server::app::build_app;
use combee_api_server::client::{
    DataNodeClient, DataNodeProvider, LocalDataNodeClient, LocalProvider, RemoteDataNodeClient,
    RoutingProvider,
};
use combee_api_server::nodes::NodeRegistry;
use combee_common::config::{Config, MetadataMode};
use combee_data_node::{DataNode, DataNodeConfig};
use combee_metadata::{InMemoryStore, MetadataStore, PostgresStore};
use tracing_subscriber::EnvFilter;

fn init_tracing() {
    // 结构化日志:默认 JSON(COMBEE_LOG_FORMAT=text 切回人类可读)
    let fmt = tracing_subscriber::fmt().with_env_filter(
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,combee=debug")),
    );
    // 日志时间戳:默认东八区(UTC+8)。可用 COMBEE_LOG_TZ_HOURS 覆盖(如 0 表示 UTC)。
    let tz_hours: i8 = std::env::var("COMBEE_LOG_TZ_HOURS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    let timer = tracing_subscriber::fmt::time::OffsetTime::new(
        time::UtcOffset::from_hms(tz_hours, 0, 0).unwrap_or(time::UtcOffset::UTC),
        time::format_description::well_known::Rfc3339,
    );
    if std::env::var("COMBEE_LOG_FORMAT").as_deref() == Ok("text") {
        fmt.with_timer(timer).init();
    } else {
        fmt.json().with_span_list(false).with_timer(timer).init();
    }
    // root span:所有事件自动带 service 字段
    let root = tracing::info_span!("service", service = "combee-api");
    let _root_guard = root.enter();
}

#[tokio::main]
async fn main() {
    // 容器 HEALTHCHECK:`combee-api-server --healthcheck`(探活 127.0.0.1:8080/ready)
    combee_common::healthcheck::run_if_healthcheck(8080, "/ready");

    init_tracing();
    let config = Config::from_env();

    let metadata: Arc<dyn MetadataStore> = match config.metadata_mode {
        MetadataMode::InMemory => Arc::new(InMemoryStore::new()),
        MetadataMode::Postgres => {
            let url = &config.database_url;
            tracing::info!("connecting metadata postgres: {url}");
            let store = PostgresStore::connect(url)
                .await
                .unwrap_or_else(|e| panic!("failed to connect metadata postgres: {e}"));
            Arc::new(store)
        }
    };

    // 启动时注入预配置的 API key(COMBEE_API_KEYS + COMBEE_ADMIN_API_KEY)
    let mut bootstrap_keys = config.api_keys.clone();
    if !config.admin_api_key.is_empty() {
        bootstrap_keys.push(config.admin_api_key.clone());
    }
    if !bootstrap_keys.is_empty() {
        tracing::info!(
            "bootstrapping {} preconfigured api keys",
            bootstrap_keys.len()
        );
        metadata
            .bootstrap_api_keys(&bootstrap_keys)
            .await
            .unwrap_or_else(|e| panic!("bootstrap api keys: {e}"));
    }

    // Data Node 客户端路由:
    // - COMBEE_MULTI_NODE=1:多节点模式,Data Node agent 注册 + 心跳,placement 全走 registry;
    // - COMBEE_DATA_NODE_URL 非空:单远程节点(未注册时兜底该地址);
    // - 默认:进程内本地 Data Node。
    // 注册表:Postgres 模式用 PG 作为共享 authority(多 API 副本一致),否则内存模式。
    tracing::info!(
        event = "service.started",
        service = "combee-api",
        version = env!("CARGO_PKG_VERSION"),
        "starting"
    );

    let registry = match config.metadata_mode {
        MetadataMode::Postgres => {
            tracing::info!("node registry: shared (postgres-backed)");
            Arc::new(NodeRegistry::with_pg(metadata.clone()))
        }
        MetadataMode::InMemory => {
            tracing::info!("node registry: in-memory");
            Arc::new(NodeRegistry::new())
        }
    };
    let local_shutdown: Option<Arc<LocalDataNodeClient>>;
    let multi_node = std::env::var("COMBEE_MULTI_NODE").as_deref() == Ok("1");
    let provider: Arc<dyn DataNodeProvider> = if multi_node {
        tracing::info!("multi-node mode: routing via node registry");
        local_shutdown = None;
        Arc::new(RoutingProvider::new(
            registry.clone(),
            metadata.clone(),
            None,
            config.control_plane_token.clone(),
        ))
    } else if config.data_node_url.is_empty() {
        let node = Arc::new(DataNode::new(DataNodeConfig::from_common(&config)));
        let local = Arc::new(LocalDataNodeClient::new(node));
        local_shutdown = Some(local.clone());
        Arc::new(LocalProvider::new(local))
    } else {
        tracing::info!("using remote data node: {}", config.data_node_url);
        local_shutdown = None;
        let remote: Arc<dyn DataNodeClient> = Arc::new(RemoteDataNodeClient::with_token(
            config.data_node_url.clone(),
            config.control_plane_token.clone(),
        ));
        Arc::new(RoutingProvider::new(
            registry.clone(),
            metadata.clone(),
            Some(remote),
            config.control_plane_token.clone(),
        ))
    };

    // 自动 failover 扫描(COMBEE_FAILOVER_INTERVAL_SECS > 0 时启用)
    let _scanner = combee_api_server::failover::spawn_failover_scanner(
        metadata.clone(),
        registry.clone(),
        provider.clone(),
        &config,
    );

    // Usage Metering:内存聚合 + 周期 flush(不进入请求热路径)
    let usage_meter =
        combee_api_server::usage::UsageMeter::new(metadata.clone(), config.usage_flush_interval);
    let _usage_flusher = usage_meter.spawn_flusher();
    let pricing_manager = combee_api_server::pricing::PricingManager::new(
        metadata.clone(),
        config.pricing_refresh_interval,
    );
    let _pricing_refresher = pricing_manager.spawn_refresher();
    let settlement = combee_api_server::settlement::Settlement::new(
        metadata.clone(),
        pricing_manager.clone(),
        config.settlement_interval,
    );
    let _settler = settlement.spawn();

    let state = AppState {
        metadata,
        data_node: provider,
        nodes: registry,
        auth_mode: combee_api_server::auth::AuthMode::from_env(),
        control_plane_token: config.control_plane_token.clone(),
        usage: usage_meter,
        pricing: pricing_manager,
        admin_token: config.admin_token.clone(),
        admin_api_key: (!config.admin_api_key.is_empty()).then_some(config.admin_api_key.clone()),
        quota: config.quota.clone(),
        concurrency: std::sync::Arc::new(combee_api_server::quota::ConcurrencyCounters::default()),
    };
    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .expect("failed to bind address");
    tracing::info!("combee listening on http://{}", config.bind_addr);

    let serve_result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;

    // 优雅关闭:本地模式 checkpoint 并关闭所有 SQLite 连接;远程模式由 Data Node 进程自行处理。
    if let Some(local) = local_shutdown {
        local.shutdown().await;
    }
    serve_result.expect("server error");
}

async fn shutdown_signal() {
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
    tracing::info!("shutdown signal received, draining");
}
