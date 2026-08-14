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
    // 容器 HEALTHCHECK:`combee-data-node --healthcheck`(探活 127.0.0.1:9000/ready)
    combee_common::healthcheck::run_if_healthcheck(9000, "/ready");
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
                    sql_timeout: (cfg.sql_timeout_secs > 0)
                        .then(|| std::time::Duration::from_secs(cfg.sql_timeout_secs)),
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
            sql_timeout: (cfg.sql_timeout_secs > 0)
                .then(|| std::time::Duration::from_secs(cfg.sql_timeout_secs)),
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

    // 内存/缓存周期采样:每 COMBEE_MEM_SAMPLE_INTERVAL_SECS(默认 30,0 关闭)打一条 INFO
    // 记录 RSS、活跃 Cell 数、KV 缓存条目数 —— 内存异常上升可直接按 mem.sample 查曲线。
    let mem_sample_secs: u64 = std::env::var("COMBEE_MEM_SAMPLE_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    if mem_sample_secs > 0 {
        let node = node.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(mem_sample_secs));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                let rss_mb = current_rss_mb();
                tracing::info!(
                    service = "combee-data-node",
                    event = "mem.sample",
                    rss_mb = %rss_mb.unwrap_or(0),
                    active_cells = node.active_count(),
                    kv_cache_len = node.cache_len(),
                );
            }
        });
    }

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
                cfg.control_plane_token.clone(),
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
                node.spawn_replica_loop(interval, agent.clone(), cfg.control_plane_token.clone());
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
    let result = server::serve(node.clone(), addr, control_token).await;
    // 优雅关闭收尾:先向 API Server 注销节点(心跳停止由 unregister 主动通知),
    // 再对每个打开的 SQLite 连接做 WAL checkpoint(TRUNCATE)并关闭。
    if let Some(agent) = agent {
        tracing::info!("unregistering node from API server");
        agent.unregister().await;
    }
    tracing::info!("checkpointing active databases");
    node.shutdown().await;
    tracing::info!("data node shutdown complete");
    result.expect("data node server error");
}

/// 当前进程 RSS 字节数(仅 Linux 有效;非 Linux 返回 None)。
fn current_rss_mb() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/statm").ok()?;
    // statm 第 2 个字段 = resident pages(通常 4KB/页)
    let resident_pages: u64 = s.split_whitespace().nth(1)?.parse().ok()?;
    Some(resident_pages.saturating_mul(4096) / (1024 * 1024))
}
