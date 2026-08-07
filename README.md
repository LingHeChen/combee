# Combee

面向 AI 生成应用与轻量级 Web 应用的 Serverless Data Runtime。

> 每个应用获得一个几乎零创建成本、无需独立容器、可按需激活的数据空间，同时提供 SQL 与 Redis-style KV 能力。

设计文档见 [`docs/COMBEE_DESIGN.md`](docs/COMBEE_DESIGN.md),
**项目整体总结(架构 / 功能 / 测试 / 性能 / 可靠性实测)见 [`docs/PROJECT_SUMMARY.md`](docs/PROJECT_SUMMARY.md)**;
**Public Alpha 发布就绪审计(Release Gate + 缺陷清单)见 [`docs/RELEASE_READINESS.md`](docs/RELEASE_READINESS.md)**。

## 仓库结构

```text
combee/
├── crates/
│   ├── common/       # ids / errors / protocol / config
│   ├── metadata/     # 控制面目录数据(InMemory / PostgreSQL)
│   ├── data-node/    # SQLite Runtime + KV Runtime + Active DB Manager + TTL GC
│   └── api-server/   # Axum HTTP API(Auth / Lifecycle / SQL / KV)
├── tests/            # 集成测试
└── docs/
```

V0 支持三种部署形态:

```text
单进程(开发/测试):    API Server ── LocalDataNodeClient ── 进程内 DataNode ── SQLite
单节点三容器:         API Server ── RemoteDataNodeClient ── HTTP RPC ── Data Node ── SQLite
多节点(registry):     API Server ── RoutingProvider ── 按 storage_node_id 路由 ── Node N ── SQLite
                      Data Node agent 注册 + 心跳(placement:round-robin)
                      API Server ── PostgreSQL(metadata,记录 storage_node_id)
```

三容器一键部署(PostgreSQL + Data Node + API Server):

```bash
docker compose up -d --build        # API 监听 127.0.0.1:8080
cargo run --release -p combee-benchmark -- --e2e --url http://127.0.0.1:8080
```

多节点(2 个 Data Node)部署:起 PostgreSQL + API Server(`COMBEE_MULTI_NODE=1`),
再起 N 个 Data Node 容器,各设 `COMBEE_API_SERVER_URL=http://<api>:8080` 与
`COMBEE_NODE_ADVERTISE_URL=http://<自身服务名>:9000` —— agent 自动注册 + 心跳,
创建数据库时按 round-robin 放置,请求按 Cell 路由到对应节点。

## 快速开始

```bash
# 启动服务(默认监听 127.0.0.1:8080,数据目录 ./data,dev 模式不校验 API key)
cargo run -p combee-api-server
```

### 常用接口

```bash
# 创建数据库(懒创建:此刻不落盘 SQLite 文件)
curl -X POST 127.0.0.1:8080/v1/databases

# 列出 / 删除
curl 127.0.0.1:8080/v1/databases
curl -X DELETE 127.0.0.1:8080/v1/databases/<id>

# SQL(首次访问时按需创建 SQLite 文件)
curl -X POST 127.0.0.1:8080/v1/databases/<id>/sql \
  -H 'content-type: application/json' \
  -d '{"sql":"CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)"}'

curl -X POST 127.0.0.1:8080/v1/databases/<id>/sql \
  -H 'content-type: application/json' \
  -d '{"sql":"INSERT INTO users (name) VALUES (?)","params":["alice"]}'

curl -X POST 127.0.0.1:8080/v1/databases/<id>/sql \
  -H 'content-type: application/json' \
  -d '{"sql":"SELECT * FROM users"}'

# 事务:多条语句原子执行
curl -X POST 127.0.0.1:8080/v1/databases/<id>/transaction \
  -H 'content-type: application/json' \
  -d '{"statements":[{"sql":"INSERT INTO users (name) VALUES (?)","params":["bob"]},{"sql":"UPDATE users SET name=? WHERE name=?","params":["robert","bob"]}]}'

# KV
curl -X PUT 127.0.0.1:8080/v1/databases/<id>/kv/session:1 \
  -H 'content-type: application/json' \
  -d '{"value":"abc","ttl_seconds":60}'

curl 127.0.0.1:8080/v1/databases/<id>/kv/session:1
curl -X DELETE 127.0.0.1:8080/v1/databases/<id>/kv/session:1
curl -X POST 127.0.0.1:8080/v1/databases/<id>/kv/ops/incr \
  -H 'content-type: application/json' \
  -d '{"key":"counter","delta":1}'
```

### 认证与租户

`COMBEE_AUTH=key` 时强制校验 `x-api-key`(默认 `off` 为开发放行):

```bash
COMBEE_AUTH=key cargo run -p combee-api-server
# 创建密钥(明文仅返回一次,库中只存 sha256 哈希):
curl -X POST 127.0.0.1:8080/v1/api-keys
# → {"key":"cmb_sk_...","record":{"id":"...","key_hash":"<sha256 hex>",...}}
curl -H 'x-api-key: cmb_sk_...' 127.0.0.1:8080/v1/databases
# 撤销:DELETE /v1/api-keys/{id}(撤销后立即 401)
```

- 密钥格式 `cmb_sk_…`,数据库只存 sha256 哈希(泄露库不泄露 key);
- 每个请求携带的只有 `AuthContext{tenant_id}`(认证中间件注入,不传原始 key);
- **租户隔离在 repository 层强制**:所有资源操作(`sql` / `transaction` / `kv` / `delete` / `backup` / `restore`)都调用 `get_database(tenant, id)`,跨租户访问一律 404(不泄露 Cell 存在性);
- 元数据模型:`tenants`(租户)→ `api_keys`(密钥,哈希)→ `databases`(Cell,含 `tenant_id`),天然对接后续计费(usage / balance / budgets)。

### Control Plane

`/internal/nodes/*`(register / heartbeat / unregister / list)与 data-node `/rpc/*` 是控制面接口,
用 **`COMBEE_CONTROL_PLANE_TOKEN`** 保护(API Server 与 Data Node 共享同一令牌):

```bash
export COMBEE_CONTROL_PLANE_TOKEN='<random-secret>'
# data-node 侧同样设置;API Server 发起的 RPC 自动携带该 token
```

- 未配置(dev):放行,但**携带租户 `x-api-key` 的请求一律 401**——租户 key 永远不能调用内部接口;
- 已配置:必须提供 `Authorization: Bearer <token>` 或 `x-control-token: <token>`;
- 租户 key 即使同时带上正确 token 也被拒绝(内部接口优先拒绝 `x-api-key`)。

## V0 范围冻结(v0.1.0-alpha)

**v0.1.0-alpha 不再新增功能**,以下明确排除在 V0 之外:

| 不做 | 说明 / 何时做 |
|---|---|
| RESP(Redis 协议) | 现有 HTTP API 保持;等真实客户需求再评估 |
| PG wire(PostgreSQL 线协议) | 同上,避免协议兼容面拖住发布 |
| Blob(大对象存储) | V0 存储以 SQL + KV 为准;对象存储仅用于备份/恢复 |
| 多副本(>1 replica) | 单 replica + failover 已够;multi-replica 是 V1 主题 |
| 更复杂 scheduler | 当前 `COMBEE_MAX_ACTIVE_DBS` LRU + 空闲休眠;调度器扩展留 V1 |

冻结纪律:V0 阶段只允许修 bug、补测试、做安全加固(如 control-plane auth);
新增产品能力一律进 backlog(999.x),发布后再评估。

## 架构速览

KV 采用 **Memory Serving Layer + Durable Storage Layer**(设计文档 §11/12):

```text
KV GET ──→ 全局共享缓存(moka,(database_id, key) 键)
              ├── hit → 纯内存返回
              └── miss → SQLite(权威数据)→ 读回并填充缓存
KV SET ──→ SQLite(先落盘,权威)→ 更新/失效缓存
```

- 整个 Data Node 共享一个有上限的缓存,冷 Cell 不占内存;
- 一致性模型:`read-through fill + write-update/write-invalidate`,SQLite 始终是权威;
- **无锁快路径**:读操作命中缓存时直接返回(不经过 per-db 锁,热点 Cell 读可并行,
  4+8 容器实测单 Cell 读 ~800 万 ops/s);写操作在 per-db 锁内串行执行(保证原子性)。
- 写入持久化强度由 `COMBEE_KV_DURABILITY` 控制(fast / normal / strict)。

## 环境变量

| 变量 | 默认值 | 说明 |
|---|---|---|
| `COMBEE_BIND_ADDR` | `127.0.0.1:8080` | HTTP 监听地址 |
| `COMBEE_DATA_DIR` | `./data` | SQLite 数据目录(按 `xx/<id>.sqlite` 分桶) |
| `COMBEE_AUTH` | `off` | 认证模式:`off`(开发放行)或 `key`(强制 `x-api-key` 校验) |
| `COMBEE_MAX_ACTIVE_DBS` | `100` | 同时打开 SQLite 连接的上限(LRU 逐出) |
| `COMBEE_DB_IDLE_TIMEOUT_SECS` | `60` | 空闲连接休眠超时 |
| `COMBEE_TTL_GC_INTERVAL_SECS` | `5` | 后台 TTL GC 周期 |
| `COMBEE_KV_CACHE_CAPACITY` | `100000` | 共享 KV 内存缓存条目上限 |
| `COMBEE_KV_DURABILITY` | `normal` | KV 写入持久化:fast(OFF)/normal(WAL fsync)/strict(FULL fsync) |
| `COMBEE_METADATA` | `in-memory` | 元数据后端:`in-memory` 或 `postgres` |
| `COMBEE_DATA_NODE_URL` | 空(单进程) | Data Node 内部 RPC 地址;设置后 API Server 走独立进程 |
| `COMBEE_MULTI_NODE` | 空 | `1` 时启用多节点模式:placement 全走 NodeRegistry(需 Data Node agent) |
| `COMBEE_API_SERVER_URL` | 空 | Data Node agent 注册/心跳的 API Server 地址(设置后启用 agent) |
| `COMBEE_NODE_ADVERTISE_URL` | `http://<DATA_NODE_ADDR>` | agent 注册的对外 RPC 地址(容器内用服务名) |
| `COMBEE_DATABASE_URL` | `postgres://combee:combee@localhost:5432/combee` | PostgreSQL 连接串(`COMBEE_METADATA=postgres` 时使用) |
| `COMBEE_CONTROL_PLANE_TOKEN` | 空(dev 放行) | 控制面令牌;保护 `/internal/*` 与 data-node `/rpc/*`;租户 `x-api-key` 永不通过 |

### PostgreSQL 元数据

```bash
# 起 PostgreSQL
docker run -d --name combee-pg -e POSTGRES_USER=combee -e POSTGRES_PASSWORD=combee -e POSTGRES_DB=combee -p 5432:5432 postgres:17

# 用 PostgreSQL 元数据启动 Combee
COMBEE_METADATA=postgres COMBEE_DATABASE_URL=postgres://combee:combee@localhost:5432/combee cargo run -p combee-api-server
```

schema(`databases` 表)在连接时自动创建;目录数据与用户数据严格分离(设计文档 §6)。

## Benchmark

对照设计文档 §22 的性能目标(不含公网 RTT,单线程客户端,进程内 DataNode):

```bash
cargo run --release -p combee-benchmark            # 默认性能基准(含 mixed workload)
cargo run --release -p combee-benchmark -- --mixed # 仅 cache miss 梯度 + mixed workload
cargo run --release -p combee-benchmark -- --contention # 热点 Cell 并发瓶颈(1 Cell × 1/8/32/128/512)
cargo run --release -p combee-benchmark -- --e2e --url http://127.0.0.1:8080  # 端到端(三容器)
cargo run --release -p combee-benchmark -- --capacity                      # 容量基准(默认 10k/100k/1M × 32/100/500/1k/5k)
cargo run --release -p combee-benchmark -- --capacity --metadata postgres --total 1M --active 32,500,5000
```

- **capacity**:扫描 total × active Cell 组合,输出 `capacity.csv` / `capacity.md`(RSS/CPU/fd/p50/p95/p99/cache hit rate),`--metadata` 可切换元数据后端;
- **miss 梯度 / mixed**:缓存命中率 0%~100% 的延迟曲线 + 60% 热读/20% 写/10% 冷读/10% 过期读的混合负载;
- **contention**:单热点 Cell 的并发瓶颈分析 —— 并发度 1/8/32/128/512,输出 `contention.csv` / `contention.md`(throughput、p50/p95/p99、per-db 锁等待 avg/max、峰值排队深度)。

实测(Apple Silicon 本机):hot GET p99 ≈ 35µs、fast SET p99 ≈ 63µs、
strict SET p99 ≈ 125µs、Simple SQL p99 ≈ 41µs,20,000 个逻辑 Cell 创建仅 ~15ms(零 IO),
活跃连接数严格限制在上限内。4 CPU + 8GiB 容器内 1M total × 5k active:p99 ≈ 64µs、命中率 100%。

## 备份 / 恢复(对象存储)

每个 Cell 可做一致性快照(SQLite `VACUUM INTO`)到 S3 兼容对象存储,并可随时恢复:

```bash
# 全量快照(VACUUM INTO)
curl -X POST http://127.0.0.1:8080/v1/databases/<id>/backup
# WAL 增量备份(主库 + 当前 WAL 归档,可手动或周期自动)
curl -X POST http://127.0.0.1:8080/v1/databases/<id>/backup/incr
# 恢复(优先取最新增量备份 = 主库 + WAL 重放;无增量则回退全量快照;
# 也可传 {"version": "<对象 key>"} 恢复指定版本)
curl -X POST http://127.0.0.1:8080/v1/databases/<id>/restore -H 'content-type: application/json' -d '{}'
```

- 对象布局:全量快照 `backups/{db_id}/{unix_ms}-{uuid}.sqlite`;增量备份
  `backups/{db_id}/incr/snapshot-{rev}.sqlite` + `wal-{rev}.sqlite-wal`(rev=unix 毫秒);
- 恢复会关闭该 Cell 连接、清缓存、原子替换本地文件,下次访问重新打开;
- 配置(`docker-compose.yml` 四件套内置 MinIO):`COMBEE_S3_ENDPOINT` / `COMBEE_S3_ACCESS_KEY` / `COMBEE_S3_SECRET_KEY` / `COMBEE_S3_BUCKET`;
- 未配置 S3 时测试可用本地文件系统后端(`object_store::LocalFileSystem`)。
- **自动周期归档**:Data Node 设 `COMBEE_WAL_BACKUP_INTERVAL_SECS`(秒)后,每隔该间隔
  对活跃 Cell 自动归档一轮"主库 + WAL"—— RPO 缩短到该间隔(恢复点 = 最近归档时刻)。

## 单 replica(复制)

Cell 级复制:设置副本节点后,副本 Data Node 周期从对象存储拉取主节点的
WAL 增量归档并应用到本地(复制通道复用增量备份):

```bash
# 设置副本(节点 id 见 GET /internal/nodes)
curl -X POST http://127.0.0.1:8080/v1/databases/<id>/replication   -H 'content-type: application/json' -d '{"replica_node": "<node-id>"}'
# 查询 / 取消
curl http://127.0.0.1:8080/v1/databases/<id>/replication
curl -X DELETE http://127.0.0.1:8080/v1/databases/<id>/replication
```

- 副本 Data Node 需配置 `COMBEE_REPLICA_INTERVAL_SECS`(拉取周期)+ 对象存储;
  主节点需 `COMBEE_WAL_BACKUP_INTERVAL_SECS`(归档周期),两者可同一节点同时开启;
- 复制延迟 ≈ 归档周期 + 拉取周期;副本与主节点归档点字节一致(实测 WAL md5 相同);
- 读请求仍走主节点,副本作为热备份(failover 下一步)。

## 自动 failover + generation fencing

主节点心跳超时(10s)且配置了副本时,自动把副本提升为主:

```text
主节点失效 ──▶ API Server 扫描(COMBEE_FAILOVER_INTERVAL_SECS)
                │ 1. 副本追平(拉取最新归档)
                │ 2. metadata:storage_node_id = 副本,generation += 1
                │ 3. fence 新主(通知新 generation)
                │ 4. fence 旧主(i64::MAX 降级标记)
                ▼
写请求自动路由到新主;旧主复活后写被 generation 拒绝(防脑裂)
```

- 手动触发:`POST /v1/databases/<id>/failover`;自动:API Server 设 `COMBEE_FAILOVER_INTERVAL_SECS`;
- 写请求带 generation(Data Node 校验),旧主被 fence 后任何写都被拒。

恢复路线图(进度):✅ snapshot backup → ✅ restore(节点炸毁可恢复)→ ✅ WAL incremental backup
→ ✅ 单 replica → ✅ 自动 failover + generation fencing。

## 测试

```bash
cargo test --workspace
```

全部测试(单元 + 集成)的**目的**与**预期结果**逐条说明见 [`docs/TESTING.md`](docs/TESTING.md)。
