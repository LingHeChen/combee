# Combee 配置参考(roadmap §10:单一来源,避免散落)

> 完整 `COMBEE_*` 环境变量清单,定义处: `crates/common/src/config.rs`(Config::from_env)。
> 部署时统一写在 `deploy/.env`,compose 与 Caddyfile 引用同一组变量(见 deploy/README.md)。

## 运行时配置(Config::from_env,共 28 项)

| 变量 | 默认值 |
|---|---|
| `COMBEE_BFF_SERVICE_KEY` | `""` |
| `COMBEE_ADMIN_TOKEN` | `""` |
| `COMBEE_API_KEYS` | `""` |
| `COMBEE_BIND_ADDR` | `"127.0.0.1:8080"` |
| `COMBEE_CONTROL_PLANE_TOKEN` | `""` |
| `COMBEE_DATA_DIR` | `"./data"` |
| `COMBEE_DATA_NODE_URL` | `""` |
| `COMBEE_DB_IDLE_TIMEOUT_SECS` | `60` |
| `COMBEE_KV_CACHE_CAPACITY` | `100_000` |
| `COMBEE_KV_DURABILITY` | `"normal"` |
| `COMBEE_MAX_ACTIVE_DBS` | `100` |
| `COMBEE_MAX_CELLS_PER_TENANT` | `1_000` |
| `COMBEE_MAX_KV_KEY_BYTES` | `1024` |
| `COMBEE_MAX_KV_VALUE_BYTES` | `256 * 1024` |
| `COMBEE_MAX_PER_CELL_CONCURRENCY` | `0` |
| `COMBEE_MAX_PER_TENANT_CONCURRENCY` | `0` |
| `COMBEE_MAX_REQUEST_BODY_BYTES` | `5 * 1024 * 1024` |
| `COMBEE_MAX_SQL_RESULT_BYTES` | `5 * 1024 * 1024` |
| `COMBEE_MAX_SQL_ROWS` | `10_000` |
| `COMBEE_MAX_TTL_SECONDS` | `30 * 24 * 60 * 60` |
| `COMBEE_METADATA` | `"in-memory"` |
| `COMBEE_PRICING_REFRESH_INTERVAL_SECS` | `5` |
| `COMBEE_SETTLEMENT_INTERVAL_SECS` | `60` |
| `COMBEE_SQL_TIMEOUT_SECS` | `30` |
| `COMBEE_STORAGE_HARD_BYTES` | `0` |
| `COMBEE_STORAGE_SOFT_BYTES` | `0` |
| `COMBEE_TTL_GC_INTERVAL_SECS` | `5` |
| `COMBEE_USAGE_FLUSH_INTERVAL_SECS` | `5` |

## 附加 env(data-node/agent/main 直接读取,不走 Config)

| 变量 | 默认值 | 说明 |
|---|---|---|
| `COMBEE_DATA_NODE_ADDR` | `0.0.0.0:9000` | data-node 监听地址 |
| `COMBEE_API_SERVER_URL` | 空 | data-node 向 api-server 注册/心跳;配置后启用 agent |
| `COMBEE_NODE_ADVERTISE_URL` | `http://{addr}` | 对外通告地址(注册用;Swarm 内用服务名) |
| `COMBEE_WAL_BACKUP_INTERVAL_SECS` | `0` | WAL 增量备份周期(秒,>0 启用) |
| `COMBEE_REPLICA_INTERVAL_SECS` | `0` | 副本同步周期(秒,>0 启用) |
| `COMBEE_LOG_FORMAT` | json | `text` 时输出文本日志 |
| `RUST_LOG` | `info,combee=debug` | 日志级别 |

## 原则

1. **单一来源**:每个地址/凭证只在 `.env` 定义一次,compose/Caddyfile 引用变量;
2. **默认值仅适合本地**,生产必须显式设置(密钥、域名、COS 凭证);
3. 配额/限制类变量(见上表 `COMBEE_MAX_*` / `COMBEE_SQL_TIMEOUT_SECS` / `COMBEE_MAX_TTL_SECONDS`)
   在 api-server 与 data-node 两侧均生效(已接线);
4. 新增配置:先在 `Config::from_env` 定义,再透传到 compose env 与文档,避免散落。
