# Combee 项目总结

> 面向 AI 生成应用与轻量级 Web 应用的 **Serverless Data Runtime**:
> 每个应用获得一个几乎零创建成本、无需独立容器、可按需激活的数据空间,同时提供 SQL 与 Redis-style KV 能力。

本文件是 Combee V0 的**整体总结**:从设计文档到最终交付的完整演进、架构、功能、测试、性能与可靠性数据,以及关键决策与踩坑记录。设计依据见 [`COMBEE_DESIGN.md`](COMBEE_DESIGN.md),测试明细见 [`TESTING.md`](TESTING.md)。

---

## 1. 演进旅程

| 阶段 | 内容 | 关键产出 |
|---|---|---|
| 1. 骨架 + PoC | Cargo workspace 5 crate;create/delete/list database、SQL 执行、KV、TTL、SQLite 持久化、单进程 `LocalDataNodeClient` | 10 集成测试;懒创建;WAL |
| 2. 测试体系 | 单元 + 集成全面覆盖,`TESTING.md` 逐条说明目的与预期结果 | 84 → 127 测试;4 处测试驱动修复 |
| 3. 共享 KV 缓存 | moka 全局缓存 `(database_id, key)` + read-through fill + write-invalidate;durability(fast/normal/strict) | hot GET 纯内存;一致性由 per-db 串行保证 |
| 4. Benchmark | 性能/容量/并发/端到端四套基准(`combee-benchmark`) | 全部达标;RSS/fd/CPU 可观测 |
| 5. PostgreSQL metadata | SQLx `PostgresStore`(databases 表 + 迁移),MinIO 对象存储 | docker compose 四件套 |
| 6. 拆分 Data Node | `combee-data-node` 独立进程,HTTP JSON 内部 RPC(12+ 端点),`RemoteDataNodeClient` | 三容器部署(API + PG + Data Node) |
| 7. 多节点 | NodeRegistry(register/heartbeat/健康判定/round-robin placement)、agent 自愈注册、`RoutingProvider` 按 Cell 路由 | 双 Data Node 实测 |
| 8. 备份/恢复 | `VACUUM INTO` 快照 → 对象存储;restore 原子替换;节点炸毁可恢复 | MinIO 端到端 |
| 9. WAL 增量备份 | 周期归档"主库 + 当前 WAL"对,恢复 = 主库 + WAL 重放,RPO 缩短到秒级 | 自动周期 + 手动 API |
| 10. 单 replica | 副本节点周期从对象存储拉取主节点增量;主副本字节级一致 | `POST /replication` 设置 |
| 11. failover + fencing | 主节点心跳超时 → 副本提升(generation+1)→ 路由切换;写请求带 generation,旧主被 fence 后写被拒 | 自动扫描 + 手动 API |

---

## 2. 最终架构

```text
                        ┌─────────────────────────────┐
  client ──HTTP──▶ API Server(Axum)                   │
                        │  Auth / Lifecycle / SQL / KV │
                        │  NodeRegistry + failover     │
                        └──────┬──────────────┬────────┘
                    COMBEE_     │              │
              DATA_NODE_URL  HTTP RPC      SQLx
                        ▼              ▼
                ┌──────────────┐  ┌──────────────┐
                │  Data Node N │  │ PostgreSQL   │
                │  SQLite WAL  │  │ (metadata)   │
                │  KV Cache    │  │ storage_node  │
                │  backup/     │  │ replica_node  │
                │  replica/    │  │ generation    │
                │  fencing     │  └──────────────┘
                └──────┬───────┘
                       ▼
              MinIO / S3(对象存储:快照 + WAL 增量)
```

**部署形态**(`COMBEE_DATA_NODE_URL` / `COMBEE_MULTI_NODE` 切换):

```text
单进程(开发/测试):  API Server ── LocalDataNodeClient ── 进程内 DataNode
单节点三容器:       API Server ── RemoteDataNodeClient ── HTTP RPC ── Data Node
多节点:             API Server ── RoutingProvider ── 按 storage_node_id 路由 ── Node N
                    Data Node agent 注册 + 心跳(placement: round-robin)
```

**代码规模**:约 10,500 行 Rust(5 个 crate + 集成测试),127 个测试。

---

## 3. 功能矩阵

### 数据面(Data Node)

| 能力 | 说明 |
|---|---|
| SQL | arbitrary SQLite SQL + 参数绑定 + 事务端点;`__sys_*` 表/事务控制语句/多语句注入防护 |
| KV | GET/SET/DEL/EXISTS/MGET/MSET/TTL/EXPIRE/PERSIST/INCR/DECR/SET NX/XX,基于 `__sys_kv` 表 |
| TTL | lazy expiration + 后台 GC(子查询规避 bundled SQLite 限制) |
| 缓存 | 全局共享 moka 缓存,`(database_id, key)` 键;无锁快路径(hot GET 读并行) |
| Durability | fast(OFF)/normal(NORMAL)/strict(FULL) 三档 `synchronous` |
| 活跃管理 | 连接上限 LRU 逐出、空闲休眠、per-db 串行 + 锁等待/排队可观测 |

### 控制面(API Server)

- Database lifecycle(`POST/GET/DELETE /v1/databases`)
- SQL / KV / 事务 HTTP 端点;`x-api-key` 认证(dev 模式放行)
- NodeRegistry + `/internal/nodes/*`(register / heartbeat / unregister / list / replicas)
- 路由:Local / 单远程 / 多节点(按 `storage_node_id` → registry → RPC)
- Replication:`POST/DELETE/GET /v1/databases/:id/replication`
- Failover:自动扫描(`COMBEE_FAILOVER_INTERVAL_SECS`)+ 手动 `POST /v1/databases/:id/failover`;generation fencing

### 可靠性

| 能力 | 机制 | 恢复点 |
|---|---|---|
| 快照备份 | `VACUUM INTO` → 对象存储 | 手动触发 |
| WAL 增量备份 | 周期归档"主库 + WAL"对 | ≤ 归档间隔(实测 3s) |
| 恢复 | 主库 + WAL 重放 / 指定版本 | — |
| 单 replica | 副本拉取主节点增量 | ≤ 归档间隔 + 拉取间隔 |
| failover | 心跳超时 → 副本提升 + generation+1 | 秒级 RTO(实测 ~18s 含探测) |
| fencing | 写带 generation,旧主 fence 到 `i64::MAX` 拒绝一切写 | 防脑裂 |

---

## 4. 测试与质量

**127 个测试**,全绿、clippy 0 警告、`cargo fmt` 干净:

```text
单元(内联)
├── combee-common      ids / protocol / config / rpc 序列化与错误分类 …… 15
├── combee-metadata    InMemory 目录语义(隔离/排序/副本/promote)………… 7
└── combee-data-node
    ├── kv             KV 全命令边界 ………………………………………… 18
    ├── sql            语句拦截 / 参数映射 / 事务 ………………………… 10
    ├── storage        分桶 / schema / WAL / durability …………………… 8
    ├── ttl            过期判定 / GC …………………………………………… 5
    ├── manager        并发 / LRU / 休眠 / 持久化 / 锁统计 ……………… 7
    ├── cache          缓存一致性 + 无锁快路径 + 并发线性化 ……………… 15
    └── backup         快照 + WAL 增量(多轮/跨 checkpoint/优先增量)… 5
集成(tests/)
├── integration.rs     14   lifecycle/SQL/KV/TTL/auth/懒创建/连接上限
├── concurrency.rs      3   并发 INCR 原子、last-writer-wins、跨 db 并行
├── kv_edge.rs          5   KV 边界/404/unicode/大 value/保留名
├── rpc.rs              3   内部 RPC 全链路 + 错误跨进程还原
├── multi_node.rs       3   注册/placement/路由/数据隔离/agent 全链路
├── replication.rs      3   单 replica 同步 + API
└── failover.rs         3   fencing + failover 全链路 + promote 语义
```

**测试驱动发现并修复的真实缺陷**(均有回归测试锁定):

1. 后台 TTL GC `DELETE ... LIMIT` 语法错误(bundled SQLite 限制)→ 子查询;
2. 多语句注入被 `rusqlite::prepare` 静默忽略 → 引号/注释感知的分号扫描器显式拒绝;
3. 保留名 key 与静态 KV 操作端点 405 冲突 → 端点移至 `/kv/ops/*`;
4. 列表排序不确定(HashMap 迭代序)→ `(created_at, id)` 排序;
5. 缓存驱逐断言依赖 moka 异步计数 → 轮询 + 行为断言;
6. per-db 锁把 hot GET 钉成常数吞吐 → **无锁快路径**(读并行,290×);
7. `KvDurability::Fast.as_str()` 返回 `"fast"` 而 SQLite pragma 需要 `"OFF"` → 显式映射;
8. Postgres 迁移整块丢失(python 脚本中途断言失败)→ 迁移/手动 ALTER 修复;
9. agent 心跳 404 不自愈(API Server 重启后节点永久丢失)→ 404 清空本地 id 重注册;
10. failover 把旧主 fence 到新 generation 导致旧主接受写 → fence 到 `i64::MAX` 降级标记。

---

## 5. 性能数据(Apple Silicon 本机 / 4 CPU + 8GiB 容器)

### 设计目标(§22)vs 实测(进程内直连 DataNode)

| 指标 | 目标 | 实测(本机) | 实测(4+8 容器) |
|---|---:|---:|---:|
| KV hot GET p50 / p99 | <1ms / <5ms | 10.1µs / 34.6µs | 0.5µs / ≤5.5µs(无锁快路径后) |
| Fast SET p99 | <5ms | 62.7µs | ~63µs |
| Strict SET p99 | <20ms | 125.2µs | ~125µs |
| Cold GET p99 | <20ms | 46.8µs | ~47µs |
| Simple SQL p99 | <20ms | 40.9µs | ~41µs |

### 容量(1M total Cells × active,4+8 容器)

- 1M 逻辑 Cell 创建 ~15ms(零 IO,不落盘);RSS 随 **active** 线性(~130KB/连接),与 total 几乎无关;
- `--metadata postgres` 时 1M 目录记录在 PG,进程内存仅 ~25MB(对比 in-memory 后端 ~490MB);
- 活跃连接严格限制在上限(fd = active×3,5000 active → 15012 fd)。

### 并发热点(1 Cell × 1–512 并发)

- 优化前:per-db 锁把单 Cell 吞吐钉成常数(GET ~27k ops/s),p99 随并发线性涨到 22.5ms;
- **无锁快路径后**:GET 命中吞吐 ~790 万 ops/s(290×),p99 ≤5.5µs,锁统计全零;写保持串行(~21k ops/s 稳定)。

### 端到端(三容器:client → HTTP → API Server → RPC → Data Node)

| 操作 | 并发 1 p50 | 并发 32 p99 |
|---|---:|---:|
| GET (cache hit) | 195µs | 1.45ms |
| SET | 252µs | 6.4ms |
| SQL SELECT | 242µs | 3.9ms |

单请求 p50 ≈ 200µs(HTTP + RPC 两跳 + JSON 序列化),全部落在设计目标内。

---

## 6. 可靠性端到端实测(MinIO)

| 场景 | 结果 |
|---|---|
| 快照备份 → 节点炸毁 → restore | 数据精确回到备份点(MinIO 对象) |
| WAL 增量(3s 周期)→ 炸毁 → restore | 恢复到最近归档点(比手动快照更新 = RPO 缩短) |
| 单 replica(主写 → 归档 → 副本拉取) | 主副本 WAL 文件 **md5 完全一致**;主更新后副本自动追赶 |
| 主节点 `docker stop` → 自动 failover | PG 中 primary=副本、generation=1;写自动走新主 |
| 旧主复活 | API 路由已切新主;generation fencing 拒绝旧主写(单测/集成锁定) |

---

## 7. 关键设计决策

1. **一个 Cell = 一个 SQLite 文件 ≠ 一个进程/连接**:ActiveDbManager 按需开连接、上限 LRU、空闲休眠——1M 逻辑 Cell 只占少量活跃资源;
2. **per-db 串行化保证一致性,无锁快路径解决热点读**:写操作在 per-db 锁内先落 SQLite 再更新缓存(read-your-writes);缓存命中的读走无锁快路径(已提交快照,线性化可证);
3. **对象存储是备份与复制的统一通道**:快照 / WAL 增量 / replica 拉取全部复用"主库 + WAL"归档,副本 = 主节点归档点的字节级副本;
4. **generation fencing**:写请求带 generation,Data Node 校验;failover 递增 generation,fence 旧主到 `i64::MAX` 降级标记——旧主复活后一切写被拒;
5. **DataNodeClient trait 抽象**:Local → Remote(HTTP RPC)→ 按节点路由,传输层可换 gRPC;
6. **metadata 是目录不是数据库**:只存控制面(`storage_node_id` / `replica_node_id` / `generation`),用户数据永远在 Data Node。

---

## 7.5 稳定性工程进度(2026-08,按 COMBEE_STABILITY_ROADMAP.md)

| 领域 | 状态 | 说明 |
|---|---|---|
| 优雅关闭 | ✅ | data-node SIGTERM → drain 在途 → unregister → WAL checkpoint;api-server 已有;`stop_grace_period 30s` |
| SQL 超时 | ✅ | `COMBEE_SQL_TIMEOUT_SECS` 两侧生效(InterruptHandle 中止,默认 30s) |
| 资源限制 | ✅ | per-tenant/per-cell 并发配额(429)、KV key/value/TTL 上限(`COMBEE_MAX_TTL_SECONDS` 默认 30 天)、MSET 校验 |
| Cell 生命周期 | ✅ | `created → active`(create 后 ensure 落盘,失败回滚)、delete 先置 `deleting` |
| 数据完整性 | ✅ | 备份 sha256 checksum;恢复后 `PRAGMA integrity_check`;打开即 `quick_check`,损坏 → 只读保护(写拒绝 + 告警) |
| Cell 格式版本 | ✅ | `CELL_FORMAT_VERSION=1` manifest,超版本拒绝打开 |
| Cell 迁移 | ✅ | `POST /admin/cells/{id}/migrate`:fence → 源备份 → 目标恢复 → 切路由 |
| 故障注入 | ✅ | `scripts/fault/`:kill-9 / 网络隔离 / 磁盘满 |
| 备份恢复验证 | ✅ | release 测试:节点炸毁恢复、删除 Cell 后恢复、恢复前后 sha256 对比 |
| 配置管理 | ✅ | `deploy/CONFIG.md` env 单一来源清单 |
| Benchmark | 🚧 | `crates/benchmark`(--mixed/--contention/--e2e/--capacity)+ 持续运行文档;数字须带环境条件 |
| 多副本 / 容量调度 / SLO | ⬜ | 远期(roadmap §13/§15:不 rush) |

---

## 8. 已知限制与下一步

- 自动 failover 的 RTO 含心跳超时探测(10s)+ 扫描周期,秒级;多副本(>1)未做;
- 旧主复活后不自动回迁(需人工 `POST /failover` 或重新设置副本);
- 节点注册表在 API Server 内存,重启后 agent 自愈重注册(新 NodeId,存量 Cell 指向旧 id 需迁移);
- gRPC 传输、capacity scheduler(按负载放置)、Prometheus metrics 导出未实现;
- 单 API Server(控制面单点);多副本 / 迁移 / 存储计算分离(V2–V4 路线)见设计文档 §24。

---

## 9. 快速上手

```bash
# 单进程(开发)
cargo run -p combee-api-server
cargo test --workspace                      # 127 个测试

# 三容器(PostgreSQL + MinIO + Data Node + API Server)
docker compose up -d --build
cargo run --release -p combee-benchmark -- --e2e --url http://127.0.0.1:8080

# 多节点 + 可靠性验证(见 README 对应章节)
#   COMBEE_MULTI_NODE=1 / COMBEE_WAL_BACKUP_INTERVAL_SECS / COMBEE_REPLICA_INTERVAL_SECS
#   COMBEE_FAILOVER_INTERVAL_SECS / COMBEE_S3_* / COMBEE_API_SERVER_URL / COMBEE_NODE_ADVERTISE_URL
```
