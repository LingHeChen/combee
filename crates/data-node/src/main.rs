//! Combee Data Node 独立进程入口。
//!
//! 提供内部 RPC 服务(HTTP JSON),由 API Server 通过 `RemoteDataNodeClient` 调用。
//! 环境变量:
//! - `COMBEE_DATA_NODE_ADDR` 监听地址(默认 `0.0.0.0:9000`)
//! - 其余 `COMBEE_*`(数据目录 / 连接上限 / 缓存 / durability)与 API Server 共用

use std::sync::Arc;

use combee_common::config::{Config, KvDurability};
use combee_data_node::server;
use combee_data_node::{DataNode, DataNodeConfig};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let fmt = tracing_subscriber::fmt().with_env_filter(
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,combee=debug")),
    );
    if std::env::var("COMBEE_LOG_FORMAT").as_deref() == Ok("text") {
        fmt.init();
    } else {
        fmt.json().with_span_list(false).init();
    }
    let root = tracing::info_span!("service", service = "combee-data-node");
    let _root_guard = root.enter();

    let cfg = Config::from_env();
    let addr = std::env::var("COMBEE_DATA_NODE_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:9000".to_string())
        .parse::<std::net::SocketAddr>()
        .expect("invalid COMBEE_DATA_NODE_ADDR");

    // 对象存储(备份/恢复):配置了 COMBEE_S3_ENDPOINT 时启用
    let node = match combee_data_node::backup::build_s3_store(
        &cfg.s3_endpoint,
        &cfg.s3_access_key,
        &cfg.s3_secret_key,
        &cfg.s3_bucket,
        &cfg.s3_region,
        cfg.s3_virtual_hosted,
    ) {
        Ok(store) => {
            tracing::info!("object storage enabled: {}", cfg.s3_endpoint);
            Arc::new(
                DataNode::new(DataNodeConfig {
                    data_dir: cfg.data_dir.clone(),
                    max_active_dbs: cfg.max_active_dbs,
                    db_idle_timeout: cfg.db_idle_timeout,
                    ttl_gc_interval: cfg.ttl_gc_interval,
                    kv_cache_capacity: cfg.kv_cache_capacity,
                    kv_durability: cfg.kv_durability,
                    sql_timeout: Some(std::time::Duration::from_secs(30)),
                    quota: cfg.quota.clone(),
                })
                .with_object_store(store),
            )
        }
        Err(_) => Arc::new(DataNode::new(DataNodeConfig {
            data_dir: cfg.data_dir.clone(),
            max_active_dbs: cfg.max_active_dbs,
            db_idle_timeout: cfg.db_idle_timeout,
            ttl_gc_interval: cfg.ttl_gc_interval,
            kv_cache_capacity: cfg.kv_cache_capacity,
            kv_durability: cfg.kv_durability,
            sql_timeout: Some(std::time::Duration::from_secs(30)),
            quota: cfg.quota.clone(),
        })),
    };

    // WAL 增量备份周期任务:COMBEE_WAL_BACKUP_INTERVAL_SECS > 0 时启用,
    // 每隔该间隔对当前活跃 Cell 归档一轮"主库 + WAL"(恢复 = 主库 + WAL 重放)。
    let wal_interval_secs: u64 = std::env::var("COMBEE_WAL_BACKUP_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if wal_interval_secs > 0 {
        let interval = std::time::Duration::from_secs(wal_interval_secs);
        if node.store_enabled() {
            node.spawn_incremental_backup(interval);
            tracing::info!("WAL incremental backup every {wal_interval_secs}s");
        } else {
            tracing::warn!("WAL incremental backup requested but object storage not configured");
        }
    }

    // 副本同步周期任务:COMBEE_REPLICA_INTERVAL_SECS > 0 时启用,
    // 周期拉取本节点作为副本的 Cell 增量并应用到本地。
    let replica_interval_secs: u64 = std::env::var("COMBEE_REPLICA_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    // 节点代理:配置了 COMBEE_API_SERVER_URL 时向 API Server 注册 + 周期心跳。
    let agent = match std::env::var("COMBEE_API_SERVER_URL") {
        Ok(api_url) => {
            let advertise = std::env::var("COMBEE_NODE_ADVERTISE_URL")
                .unwrap_or_else(|_| format!("http://{addr}"));
            let id_file = std::path::PathBuf::from(&cfg.data_dir).join("node-id");
            let (agent, _heartbeat) = combee_data_node::agent::NodeAgent::start(
                &api_url,
                &advertise,
                cfg.max_active_dbs,
                Some(&id_file),
            )
            .await;
            Some(agent)
        }
        Err(_) => None,
    };
    if replica_interval_secs > 0 {
        if let Some(agent) = &agent {
            if node.store_enabled() {
                let interval = std::time::Duration::from_secs(replica_interval_secs);
                node.spawn_replica_loop(interval, agent.clone());
                tracing::info!("replica sync every {replica_interval_secs}s");
            } else {
                tracing::warn!("replica sync requested but object storage not configured");
            }
        } else {
            tracing::warn!("replica sync requested but COMBEE_API_SERVER_URL not set");
        }
    }

    tracing::info!(
        "combee data node starting (durability={})",
        match cfg.kv_durability {
            KvDurability::Fast => "fast",
            KvDurability::Normal => "normal",
            KvDurability::Strict => "strict",
        }
    );

    let control_token = {
        let v = std::env::var("COMBEE_CONTROL_PLANE_TOKEN").unwrap_or_default();
        if v.is_empty() { None } else { Some(v) }
    };
    let result = server::serve(node, addr, control_token).await;
    // 退出前注销节点
    if let Some(agent) = agent {
        agent.unregister().await;
    }
    result.expect("data node server error");
}
