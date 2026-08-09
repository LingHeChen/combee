# Combee Logging 规范(Logging P0)

> 目标:日志生产端统一 —— stdout JSON,`docker logs | jq` 即可运维;不依赖 ELK/Loki。
> 生效范围:`combee-api`、`combee-data-node`(Rust/tracing)、`combee-bff`(Next.js)。

## 1. 输出

- **Rust**:`tracing_subscriber::fmt().json()`(默认);`COMBEE_LOG_FORMAT=text` 切人类可读;
  级别过滤 `RUST_LOG`(默认 `info,combee=debug`);
- **BFF**:单行 JSON(`console.log`),`service=combee-bff`;`COMBEE_BFF_DEBUG=1` 打开 DEBUG access 日志。
- 日志写 **stdout**(docker logs / 托管平台直接采集),不自建文件。

## 2. 公共字段

| 字段 | 说明 |
|---|---|
| `timestamp` | ISO8601 UTC |
| `level` | DEBUG / INFO / WARN / ERROR |
| `service` | `combee-api` / `combee-data-node` / `combee-bff`(root span 注入) |
| `request_id` | `req_<hex>`;入口(BFF/API 客户端)生成或透传;随 **RPC header** 贯穿到 DataNode |
| `tenant_id` / `cell_id` | 请求上下文(路由/认证后注入) |
| `operation` | 如 `v1.databases.{id}.sql`、`kv.set`、`auth.login` |
| `status` | HTTP 状态码(access 日志) |
| `latency_ms` | 耗时 |
| `event` | 事件名(见 §4),如 `cell.open`、`backup.completed` |
| `error_code` | 稳定错误码(如 `SQL_TIMEOUT`、`QUOTA_EXCEEDED`) |
| `job` | 后台任务标识(backup / replication / usage_flush / credit_settlement / failover) |

示例(正常):`{"timestamp":"…","level":"DEBUG","service":"combee-api","request_id":"req_e610…","tenant_id":"…","cell_id":"…","operation":"v1.databases.{id}.kv.{key}","status":200,"latency_ms":"1.82"}`
示例(失败):`{"level":"ERROR","service":"combee-api","request_id":"req_…","operation":"sql.query","error_code":"SQL_TIMEOUT","latency_ms":"5001"}`

## 3. 级别策略

| 级别 | 用途 |
|---|---|
| ERROR | 真错误(5xx、任务失败、backup/settlement/failover 失败) |
| WARN | degraded / retry / lag / 401 / 429(quota/rate_limit) |
| INFO | 生命周期与重要操作(节点注册、备份完成、settlement 轮次、failover 提升) |
| DEBUG | 单请求详细(access 日志、cell.open/sleep/evict、RPC 请求) |
| TRACE | 开发调试 |

**普通请求 access 日志为 DEBUG**(RUST_LOG=debug 才输出)——hot KV GET 不产生 INFO,
避免日志打爆磁盘;生产建议由 API gateway 单独记录 access,服务端只记异常事件。

## 4. 结构化事件清单

| event | 级别 | 位置 |
|---|---|---|
| `request.completed` | DEBUG | api-server logging 中间件 |
| `request.failed` | ERROR | 5xx |
| `auth.failed` | WARN | 401 |
| `quota.exceeded` / `rate_limit.exceeded` | WARN | 429 |
| `rpc.request` | DEBUG | data-node RPC 入口 |
| `cell.open` / `cell.sleep` / `cell.evict` / `cell.close` | DEBUG / INFO | Active DB Manager |
| `usage.flush.failed` | WARN | Usage Metering |
| `credits.settlement.success` / `.failed` | INFO / WARN | Settlement(job=credit_settlement) |
| `backup.completed` / `backup.failed` | INFO / ERROR | Backup(job=backup) |
| `replica.catchup` / `replica.lag_high` | INFO / WARN | Replication(job=replication) |
| `failover.started` / `failover.promoted` / `failover.failed` | INFO / ERROR | Failover(job=failover) |
| `node.registered` / `node.heartbeat_timeout` | INFO / WARN | Node Registry / agent |

## 5. 敏感数据禁止入日志(硬规则)

- ❌ API Key、密码/哈希、session id、voucher code、SQL 参数、KV value、请求/响应体全文;
- ❌ access 日志不记录 request body;SQL 至多记录截断/归一化后的语句(生产默认不记 SQL 原文);
- BFF `bffLog` 对 `password|api_key|access_code|session|voucher|secret` 字段名**整条丢弃**;
- 对象存储键(backup 的 `key`)为 `backups/{db}/{ts}-{uuid}.sqlite`,非敏感,可保留。

## 6. 运维示例

```bash
docker logs combee-api 2>&1 | jq 'select(.level == "ERROR")'
# 按 request_id 串起 BFF → API → DataNode 一条链:
grep 'req_8f2a…' bff.log api.log data-node.log
docker logs combee-data-node 2>&1 | jq 'select(.event == "cell.open")'
docker logs combee-api 2>&1 | jq 'select(.event == "quota.exceeded")'
```
