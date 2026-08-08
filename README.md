# Combee

> **One app, one Cell. SQL + KV included. No database instances.**

Combee 是面向 AI 生成应用与轻量级 Web 应用的 Serverless Data Runtime:
每个应用获得一个**几乎零创建成本、无需独立容器、可按需激活**的数据空间(Cell),
同一个空间里同时提供 **SQL** 与 **Redis-style KV**。

```text
app ──▶ POST /v1/databases        → 一个 Cell(此刻不落盘任何文件)
app ──▶ POST /v1/databases/:id/sql      → SQLite(SQL)
app ──▶ PUT  /v1/databases/:id/kv/:key  → SQLite-backed KV(TTL / counter / mget)
```

## 30 秒 Quickstart

```bash
cargo run -p combee-api-server        # dev 模式:127.0.0.1:8080,免 key
```

```bash
# 1) 创建 Cell(懒创建,零 IO)
curl -X POST 127.0.0.1:8080/v1/databases          # → {"id":"<cell-id>"}

# 2) 写 SQL
curl -X POST 127.0.0.1:8080/v1/databases/<cell-id>/sql \
  -H 'content-type: application/json' \
  -d '{"sql":"CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)"}'

# 3) 写 KV(带 TTL)
curl -X PUT 127.0.0.1:8080/v1/databases/<cell-id>/kv/session:1 \
  -H 'content-type: application/json' -d '{"value":"abc","ttl_seconds":60}'
```

完整 API 示例见下方[快速开始(完整)](#快速开始完整)。

## 架构

```text
                    ┌─────────────────────────────────────────────┐
   client ──HTTP──▶ │ API Server (Axum)                           │
                    │   Auth → TenantContext → get_database(tenant,id) │
                    └───────────────┬─────────────────────────────┘
                          ┌─────────┴───────────┐
                          │  Local (单进程) / RPC │  RemoteDataNodeClient(HTTP RPC)
                          └─────────┬───────────┘
                    ┌───────────────▼─────────────────────────────┐
                    │ Data Node                                    │
                    │   SQL Runtime (SQLite, WAL)                  │
                    │   KV Runtime  ── moka 共享缓存 ── SQLite(权威)│
                    │   Active DB Manager: ≤ N 个连接, LRU 逐出/空闲休眠 │
                    │   TTL GC / SQL timeout / backup / replica     │
                    └───────────────┬─────────────────────────────┘
                        ┌───────────┴──────────────┐
                        │ PostgreSQL (metadata)    │  tenants / api_keys / databases
                        │ S3/MinIO (backup/WAL)    │  snapshot / incr / restore
                        └──────────────────────────┘
```

核心设计:100,000 个逻辑 Cell ≠ 100,000 个 SQLite 连接——
`COMBEE_MAX_ACTIVE_DBS` 上限 + LRU 逐出 + 空闲休眠,冷 Cell 不占内存、不占 fd。

## 为什么是 Combee

| | 传统方案 | Combee |
|---|---|---|
| 一个应用一个库 | 创建库 / 分配实例 / 等就绪 | `POST /v1/databases` → 立即可用(懒创建,零 IO) |
| SQL + KV | 两套系统、两种运维 | 一个 Cell,SQLite SQL + KV(TTL/counter)一起给 |
| 规模 | 实例数 = 库数 | 逻辑 Cell 任意多,连接上限固定 |

## 性能

对照设计文档 §22 目标,**全部达标**(Apple Silicon 本机,进程内 DataNode):

| 场景 | 实测 | 目标 |
|---|---|---|
| KV hot GET p50 / p99 | 10µs / 35µs | <1ms / <5ms |
| KV fast SET p99(durability=fast) | 63µs | <5ms |
| KV strict SET p99(durability=strict) | 125µs | <20ms |
| Simple SQL p99 | 41µs | <20ms |
| 创建 20,000 个逻辑 Cell | ~15ms | 零 IO |

**1M logical Cells**(4 CPU + 8GiB 容器,`--capacity --total 1M --active 5k`):
p99 ≈ 64µs、缓存命中率 100%、活跃 SQLite 连接数严格 ≤ 上限。
完整 15 组扫描数据见 `artifacts/capacity.csv` / `artifacts/capacity.md`(benchmark 运行产物,本地生成、不入库;`--capacity` 会重新生成)。

## 核心能力

- **SQL**:单条执行 + 多语句原子事务;参数绑定;`__sys_*` 内部表 / 事务控制语句 / 多语句注入全部拦截;SQL 超时中断(`COMBEE_SQL_TIMEOUT_SECS`)。
- **KV**:GET / SET / DEL / EXISTS / MGET / MSET / TTL / EXPIRE / INCR;TTL 惰性过期 + 后台 GC;共享内存缓存(hit 无锁快路径,miss read-through)。
- **备份/恢复**:一致性快照(`VACUUM INTO`)+ WAL 增量归档 → S3/MinIO;节点炸毁后可恢复。
- **单 replica + 自动 failover**:复制通道复用 WAL 归档;主节点心跳超时自动提升副本,generation fencing 防脑裂。
- **多租户**:API key(仅存 sha256 哈希)绑定 tenant;隔离在 repository 层强制,跨租户一律 404。
- **Usage Metering**:按 (tenant, cell, metric, 分钟桶) 统计 KV/SQL read/write、requests、bytes in/out、storage bytes;内存聚合 + 周期 flush(不进入热路径),`GET /v1/usage/summary` / `/v1/usage/timeseries` / `/v1/cells/{id}/usage`。
- **Public API 契约**:`GET /openapi.json` 机器契约 + [docs/API.md](docs/API.md) 冻结规范(request-id / 稳定错误 code / Idempotency-Key / 游标分页)。
- **Credits + Pricing**:整数 microcredits 账本(append-only,余额可从账本重建)、admin grant、voucher 兑换(哈希存库、单次/幂等/并发安全)、pricing 版本热更新(5s 生效,无效配置拒绝);settlement 周期把 usage → credits(记录 pricing_version,soft limit 告警不切断)。
- **Control plane**:`/internal/*` 与 data-node `/rpc/*` 由 `COMBEE_CONTROL_PLANE_TOKEN` 保护,租户 key 永不进入内部接口。

## Known Limitations

- **单 Cell 写串行**:per-db 锁保证一致性,热点 Cell 写并发受限(读可并行,实测单 Cell 读 ~800 万 ops/s);多个小 Cell 各自并行。
- **无资源配额**:max KV value / max SQL 结果 / 并发上限未实现(依赖 axum body limit + SQL timeout 兜底)。
- **默认元数据为 in-memory**:生产请用 `COMBEE_METADATA=postgres`(重启不丢 Cell 目录)。
- **failover 依赖对象存储**:副本通过 S3 拉取归档;未配置 S3 时无复制/failover。
- **V0 明确不做**:RESP / PG wire / Blob / 多副本(>1 replica)/ 复杂 scheduler —— 见 [V0 范围冻结](#v0-范围冻结v01-alpha)。

## 文档

| 文档 | 内容 |
|---|---|
| [docs/COMBEE_DESIGN.md](docs/COMBEE_DESIGN.md) | 完整设计(§1–§22:架构、存储、一致性、性能目标) |
| [docs/PROJECT_SUMMARY.md](docs/PROJECT_SUMMARY.md) | 项目总结(架构/功能/测试/性能/可靠性实测) |
| [docs/RELEASE_READINESS.md](docs/RELEASE_READINESS.md) | Public Alpha 审计:Release Gate + 缺陷清单(BLOCKER=0 / HIGH=0) |
| [docs/TESTING.md](docs/TESTING.md) | 全部测试的目的与预期结果 |
| [docs/COMBEE_RELEASE_READINESS_TEST_PLAN.md](docs/COMBEE_RELEASE_READINESS_TEST_PLAN.md) | Release Gate 测试计划 |
| [docs/API.md](docs/API.md) | **Public API 冻结契约**(分层 / 错误模型 / request-id / Idempotency / Pagination) |
| [CHANGELOG.md](CHANGELOG.md) / [SECURITY.md](SECURITY.md) / [CONTRIBUTING.md](CONTRIBUTING.md) | 变更记录 / 安全 / 贡献 |

---

## 快速开始(完整)

```bash
# 创建 / 列出 / 删除 Cell
curl -X POST 127.0.0.1:8080/v1/databases
curl 127.0.0.1:8080/v1/databases
curl -X DELETE 127.0.0.1:8080/v1/databases/<id>

# SQL
curl -X POST 127.0.0.1:8080/v1/databases/<id>/sql \
  -H 'content-type: application/json' \
  -d '{"sql":"INSERT INTO users (name) VALUES (?)","params":["alice"]}'

# 事务:多条语句原子执行
curl -X POST 127.0.0.1:8080/v1/databases/<id>/transaction \
  -H 'content-type: application/json' \
  -d '{"statements":[{"sql":"INSERT INTO users (name) VALUES (?)","params":["bob"]}]}'

# KV
curl -X PUT 127.0.0.1:8080/v1/databases/<id>/kv/session:1 \
  -H 'content-type: application/json' -d '{"value":"abc","ttl_seconds":60}'
curl 127.0.0.1:8080/v1/databases/<id>/kv/session:1
curl -X POST 127.0.0.1:8080/v1/databases/<id>/kv/ops/incr \
  -H 'content-type: application/json' -d '{"key":"counter","delta":1}'
```

### 认证与租户

`COMBEE_AUTH=key` 时强制校验 `x-api-key`(默认 `off` 为开发放行):

```bash
COMBEE_AUTH=key cargo run -p combee-api-server
curl -X POST 127.0.0.1:8080/v1/api-keys        # 创建密钥:明文仅返回一次
curl -H 'x-api-key: <cmb_sk_...>' 127.0.0.1:8080/v1/databases
curl -X DELETE 127.0.0.1:8080/v1/api-keys/<id> # 撤销后立即 401
```

- 密钥 `cmb_sk_…`,库中只存 sha256 哈希;
- 请求生命周期只携带 `AuthContext{tenant_id}`;跨租户访问一律 404(不泄露 Cell 存在性)。

### Control Plane

```bash
export COMBEE_CONTROL_PLANE_TOKEN='<random-secret>'   # API Server 与 Data Node 共享
```

`/internal/nodes/*` 与 data-node `/rpc/*` 需 `Authorization: Bearer <token>` 或 `x-control-token`;
**携带租户 `x-api-key` 的请求永远 401**(即使 dev 模式、即使同时带对 token)。

### 部署形态

```text
单进程(开发):    API Server ── LocalDataNodeClient ── 进程内 DataNode ── SQLite
三容器:          API Server ── RemoteDataNodeClient ── HTTP RPC ── Data Node ── SQLite
                                                                  └── PostgreSQL(metadata)
多节点:          API Server ── RoutingProvider ── 按 storage_node_id 路由 ── Node N ── SQLite
                 Data Node agent 注册 + 心跳(round-robin placement)
```

```bash
docker compose up -d --build        # PostgreSQL + MinIO + Data Node + API Server
cargo run --release -p combee-benchmark -- --e2e --url http://127.0.0.1:8080
```

多节点:起 PostgreSQL + API Server(`COMBEE_MULTI_NODE=1`),再起 N 个 Data Node
(各设 `COMBEE_API_SERVER_URL` 与 `COMBEE_NODE_ADVERTISE_URL`),agent 自动注册 + 心跳。

### 备份 / 恢复 / 复制 / failover

```bash
curl -X POST 127.0.0.1:8080/v1/databases/<id>/backup           # 全量快照(VACUUM INTO)
curl -X POST 127.0.0.1:8080/v1/databases/<id>/backup/incr      # WAL 增量归档
curl -X POST 127.0.0.1:8080/v1/databases/<id>/restore          # 优先增量,回退全量
curl -X POST 127.0.0.1:8080/v1/databases/<id>/replication \
  -H 'content-type: application/json' -d '{"replica_node": "<node-id>"}'
curl -X POST 127.0.0.1:8080/v1/databases/<id>/failover         # 手动提升副本
```

- 对象布局:`backups/{db_id}/{unix_ms}-{uuid}.sqlite` + `incr/snapshot-{rev}.sqlite` + `wal-{rev}.sqlite-wal`;
- 自动归档:`COMBEE_WAL_BACKUP_INTERVAL_SECS`;复制周期:`COMBEE_REPLICA_INTERVAL_SECS`;自动 failover:`COMBEE_FAILOVER_INTERVAL_SECS`;
- failover 流程:副本追平 → `storage_node_id=副本, generation+=1` → fence 新主/旧主 → 写路由新主,旧主复活写被拒(防脑裂)。

## 环境变量

| 变量 | 默认值 | 说明 |
|---|---|---|
| `COMBEE_BIND_ADDR` | `127.0.0.1:8080` | HTTP 监听地址 |
| `COMBEE_DATA_DIR` | `./data` | SQLite 数据目录(按 `xx/<id>.sqlite` 分桶) |
| `COMBEE_AUTH` | `off` | 认证:`off`(开发放行)或 `key`(强制 `x-api-key`) |
| `COMBEE_CONTROL_PLANE_TOKEN` | 空(dev 放行) | 控制面令牌;`/internal/*` 与 `/rpc/*`;租户 key 永不通过 |
| `COMBEE_MAX_ACTIVE_DBS` | `100` | 同时打开 SQLite 连接上限(LRU 逐出) |
| `COMBEE_DB_IDLE_TIMEOUT_SECS` | `60` | 空闲连接休眠超时 |
| `COMBEE_TTL_GC_INTERVAL_SECS` | `5` | 后台 TTL GC 周期 |
| `COMBEE_KV_CACHE_CAPACITY` | `100000` | KV 共享缓存条目上限 |
| `COMBEE_KV_DURABILITY` | `normal` | fast(OFF)/normal(WAL fsync)/strict(FULL fsync) |
| `COMBEE_SQL_TIMEOUT_SECS` | `30` | 单条 SQL 超时(0=不限);超时中断 |
| `COMBEE_USAGE_FLUSH_INTERVAL_SECS` | `5` | Usage 聚合 flush 周期(内存 → metadata) |
| `COMBEE_PRICING_REFRESH_INTERVAL_SECS` | `5` | Pricing 热更新轮询周期 |
| `COMBEE_SETTLEMENT_INTERVAL_SECS` | `60` | Usage → Credits 结算周期 |
| `COMBEE_ADMIN_TOKEN` | 空(admin 接口 401) | Operator/Admin 令牌:grant / voucher / pricing 管理 |
| `COMBEE_METADATA` | `in-memory` | 元数据后端:`in-memory` 或 `postgres` |
| `COMBEE_DATABASE_URL` | `postgres://combee:combee@localhost:5432/combee` | PostgreSQL 连接串 |
| `COMBEE_DATA_NODE_URL` | 空(单进程) | Data Node RPC 地址;设置后走独立进程 |
| `COMBEE_MULTI_NODE` | 空 | `1` 时启用多节点(placement 走 NodeRegistry) |
| `COMBEE_API_SERVER_URL` | 空 | Data Node agent 注册/心跳地址(设置后启用 agent) |
| `COMBEE_NODE_ADVERTISE_URL` | `http://<DATA_NODE_ADDR>` | agent 对外 RPC 地址(容器内用服务名) |
| `COMBEE_S3_ENDPOINT` / `COMBEE_S3_ACCESS_KEY` / `COMBEE_S3_SECRET_KEY` / `COMBEE_S3_BUCKET` | 空 | 对象存储(备份/恢复/复制);未配置时测试可用本地 fs |
| `COMBEE_WAL_BACKUP_INTERVAL_SECS` | `0`(关) | WAL 增量自动归档周期 |
| `COMBEE_REPLICA_INTERVAL_SECS` | `0`(关) | 副本拉取归档周期 |
| `COMBEE_FAILOVER_INTERVAL_SECS` | `0`(关) | 自动 failover 扫描周期 |

### PostgreSQL 元数据

```bash
docker run -d --name combee-pg -e POSTGRES_USER=combee -e POSTGRES_PASSWORD=combee \
  -e POSTGRES_DB=combee -p 5432:5432 postgres:17
COMBEE_METADATA=postgres COMBEE_DATABASE_URL=postgres://combee:combee@localhost:5432/combee \
  cargo run -p combee-api-server
```

schema 在连接时自动创建;目录数据与用户数据严格分离(设计文档 §6)。

## Benchmark 复现

```bash
cargo run --release -p combee-benchmark            # 默认性能基准(含 mixed workload)
cargo run --release -p combee-benchmark -- --mixed # cache miss 梯度 + mixed workload
cargo run --release -p combee-benchmark -- --contention # 热点 Cell 并发(1/8/32/128/512)
cargo run --release -p combee-benchmark -- --capacity   # 容量扫描 → artifacts/capacity.csv|md(本地产物)
cargo run --release -p combee-benchmark -- --e2e --url http://127.0.0.1:8080
```

## 测试

```bash
cargo test --workspace
```

全部测试(单元 + 集成)的**目的**与**预期结果**逐条说明见 [`docs/TESTING.md`](docs/TESTING.md)。

## V0 范围冻结(v0.1-alpha)

**v0.1.0-alpha 不再新增功能**,明确排除:RESP、PG wire、Blob、多副本(>1 replica)、更复杂 scheduler。
冻结纪律:V0 只允许修 bug、补测试、安全加固;新能力进 backlog(999.x)。
