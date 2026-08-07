//! 服务配置,全部通过环境变量注入(带合理默认值)。

use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

/// 元数据存储后端。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataMode {
    /// V0 默认:进程内存储,零外部依赖。
    InMemory,
    /// 预留:PostgreSQL(SQLx)。V0 尚未实现。
    Postgres,
}

/// KV 写入的持久化强度(设计文档第 14 节)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KvDurability {
    /// 快:SQLite `synchronous=OFF`,不 fsync,追求接近 Redis 的写延迟。
    #[default]
    Fast,
    /// 稳(默认推荐):`synchronous=NORMAL`,WAL 模式下每次 commit 落 WAL。
    Normal,
    /// 严格:`synchronous=FULL`,commit + checkpoint 均 fsync。
    Strict,
}

impl KvDurability {
    pub fn as_str(self) -> &'static str {
        match self {
            KvDurability::Fast => "fast",
            KvDurability::Normal => "normal",
            KvDurability::Strict => "strict",
        }
    }
}

impl fmt::Display for KvDurability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for KvDurability {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "fast" => Ok(KvDurability::Fast),
            "normal" | "wal" => Ok(KvDurability::Normal),
            "strict" | "full" => Ok(KvDurability::Strict),
            other => Err(format!("unknown kv durability: {other}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    /// HTTP 监听地址。
    pub bind_addr: SocketAddr,
    /// SQLite 数据目录。
    pub data_dir: PathBuf,
    /// 启用的 API key 列表;为空表示 dev 模式(不校验)。
    pub api_keys: Vec<String>,
    /// 同时打开 SQLite 连接的上限。
    pub max_active_dbs: usize,
    /// 空闲连接休眠超时。
    pub db_idle_timeout: Duration,
    /// 后台 TTL GC 周期。
    pub ttl_gc_interval: Duration,
    /// 元数据存储后端。
    pub metadata_mode: MetadataMode,
    /// 元数据 PostgreSQL 连接串(`COMBEE_METADATA=postgres` 时使用)。
    pub database_url: String,
    /// Data Node 内部 RPC 地址;为空表示使用进程内 LocalDataNodeClient。
    pub data_node_url: String,
    /// 对象存储(S3/MinIO)配置;endpoint 为空表示未启用 backup。
    pub s3_endpoint: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub s3_bucket: String,
    pub s3_region: String,
    /// 共享 KV 内存缓存的条目上限。
    pub kv_cache_capacity: usize,
    /// KV 写入持久化强度。
    pub kv_durability: KvDurability,
    /// 单条 SQL 执行超时(秒);0 = 不限。
    pub sql_timeout_secs: u64,
    /// 控制面令牌(`COMBEE_CONTROL_PLANE_TOKEN`)。
    /// `/internal/*` 与 data-node `/rpc/*` 的保护:
    /// - 未配置(dev):放行,但携带租户 `x-api-key` 的请求一律拒绝;
    /// - 配置:必须提供 `Authorization: Bearer <token>` 或 `x-control-token: <token>`。
    pub control_plane_token: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        let bind_addr = env_str("COMBEE_BIND_ADDR", "127.0.0.1:8080")
            .parse()
            .expect("invalid COMBEE_BIND_ADDR");
        let data_dir = PathBuf::from(env_str("COMBEE_DATA_DIR", "./data"));
        let api_keys = env_str("COMBEE_API_KEYS", "")
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        let max_active_dbs = env_parse("COMBEE_MAX_ACTIVE_DBS", 100);
        let db_idle_timeout = Duration::from_secs(env_parse("COMBEE_DB_IDLE_TIMEOUT_SECS", 60));
        let ttl_gc_interval = Duration::from_secs(env_parse("COMBEE_TTL_GC_INTERVAL_SECS", 5));
        let metadata_mode = match env_str("COMBEE_METADATA", "in-memory").as_str() {
            "postgres" | "postgresql" => MetadataMode::Postgres,
            _ => MetadataMode::InMemory,
        };
        let kv_cache_capacity = env_parse("COMBEE_KV_CACHE_CAPACITY", 100_000);
        let sql_timeout_secs = env_parse("COMBEE_SQL_TIMEOUT_SECS", 30);
        let control_plane_token = {
            let v = env_str("COMBEE_CONTROL_PLANE_TOKEN", "");
            if v.is_empty() { None } else { Some(v) }
        };
        let kv_durability = env_str("COMBEE_KV_DURABILITY", "normal")
            .parse()
            .unwrap_or_default();
        let database_url = env_str(
            "COMBEE_DATABASE_URL",
            "postgres://combee:combee@localhost:5432/combee",
        );
        let data_node_url = env_str("COMBEE_DATA_NODE_URL", "");
        let s3_endpoint = env_str("COMBEE_S3_ENDPOINT", "");
        let s3_access_key = env_str("COMBEE_S3_ACCESS_KEY", "");
        let s3_secret_key = env_str("COMBEE_S3_SECRET_KEY", "");
        let s3_bucket = env_str("COMBEE_S3_BUCKET", "combee-backups");
        let s3_region = env_str("COMBEE_S3_REGION", "us-east-1");

        Self {
            bind_addr,
            data_dir,
            api_keys,
            max_active_dbs,
            db_idle_timeout,
            ttl_gc_interval,
            metadata_mode,
            database_url,
            data_node_url,
            s3_endpoint,
            s3_access_key,
            s3_secret_key,
            s3_bucket,
            s3_region,
            kv_cache_capacity,
            kv_durability,
            sql_timeout_secs,
            control_plane_token,
        }
    }
}

fn env_str(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn env_parse<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durability_parse_and_display() {
        assert_eq!("fast".parse::<KvDurability>().unwrap(), KvDurability::Fast);
        assert_eq!("FAST".parse::<KvDurability>().unwrap(), KvDurability::Fast);
        assert_eq!(
            "normal".parse::<KvDurability>().unwrap(),
            KvDurability::Normal
        );
        assert_eq!("wal".parse::<KvDurability>().unwrap(), KvDurability::Normal);
        assert_eq!(
            "strict".parse::<KvDurability>().unwrap(),
            KvDurability::Strict
        );
        assert_eq!(
            "full".parse::<KvDurability>().unwrap(),
            KvDurability::Strict
        );
        assert!("unknown".parse::<KvDurability>().is_err());
        assert_eq!(KvDurability::Fast.as_str(), "fast");
        assert_eq!(KvDurability::Strict.to_string(), "strict");
        assert_eq!(KvDurability::default(), KvDurability::Fast);
    }

    #[test]
    fn env_parse_falls_back_to_default() {
        // 环境变量未设置时回退默认值
        // (不在测试中 set_var:edition 2024 下为 unsafe 且有并行测试竞争)
        let v: u64 = env_parse("COMBEE_THIS_VAR_SHOULD_NOT_EXIST_9f3k", 42);
        assert_eq!(v, 42);
    }
}
