# Combee 设计概要

> **Combee**：面向 AI 生成应用与轻量级 Web 应用的 Serverless Data Runtime。
>
> 核心目标是让每个应用都能获得一个几乎零创建成本、无需独立容器、可按需激活的数据空间，同时提供 SQL 与 Redis-style KV 能力。

---

## 1. 项目背景

最初需求来自 AI 建站场景：

- 用户生成的网站往往需要数据库；
- 传统做法通常需要单独提供 PostgreSQL / MySQL / Redis；
- 大量测试站点、Preview、个人项目实际数据量和访问量极低；
- 为每个项目启动独立数据库容器，会产生明显的内存、进程、连接池、调度和运维开销；
- AI 生成代码也不应该被迫管理数据库地址、端口、用户名、密码、连接池、Redis 配置等基础设施细节。

因此 Combee 的方向不是传统意义上的“去中心化数据库”，而更接近：

> **面向海量 tiny database 的无实例、按需激活、多租户 Serverless 数据层。**

核心思想：

```text
一个物理集群
    ↓
大量逻辑数据库
    ↓
逻辑数据库不对应独立数据库进程
    ↓
有请求时才占用连接 / 内存 / IO
```

---

## 2. 产品定位

Combee 第一阶段定位为：

> **Database-per-app Serverless Data Runtime**

一个应用对应一个逻辑数据空间，同时提供：

```text
Database / Cell
├── SQL
└── KV + TTL
```

后续可以扩展：

```text
Cell
├── SQL
├── KV
├── Blob
├── Vector
└── Queue
```

目标不是第一天就成为 PostgreSQL 或 Redis 的完整替代品，而是优先覆盖 AI 建站、Preview、测试环境、小型 SaaS、小型 Agent 应用中最常见的数据需求。

---

## 3. 命名

项目正式名称：

# Combee

名称来源于：

- `comb`：蜂巢中的巢脾 / honeycomb；
- `bee`：蜂群；
- 同时保留“蜂巢由大量小 Cell 组成”的系统隐喻。

推荐概念映射：

```text
Combee Cluster
└── Data Node
    ├── Cell A
    ├── Cell B
    └── Cell C
```

其中：

- **Cell**：一个逻辑数据库 / 应用数据空间；
- **Data Node**：真正承载 SQLite、KV 热缓存和磁盘数据的节点；
- **Cluster**：整个 Combee 集群。

不建议为了蜂群主题强行给所有组件起拟物化名字，内部工程名称保持直白。

---

## 4. V0 总体架构

第一版架构保持极简：

```text
                 Internet
                    │
                    ▼
           ┌────────────────┐
           │   API Server   │
           │                │
           │ Auth           │
           │ Routing        │
           │ Quota          │
           │ Rate Limit     │
           └───────┬────────┘
                   │
          ┌────────┴────────┐
          │                 │
          ▼                 ▼
┌──────────────────┐   ┌──────────────────┐
│   Metadata DB    │   │    Data Node     │
│                  │   │                  │
│ db → node        │   │ SQLite Runtime   │
│ ownership        │   │ KV Runtime       │
│ quota            │   │ Memory Cache     │
│ state            │   │ TTL GC           │
└──────────────────┘   │ WAL              │
                       │ Local NVMe       │
                       └──────────────────┘
```

核心约束：

> **任意时刻，一个逻辑 Cell 只属于一个 Data Node。**

第一版不做跨节点 SQL、不做分布式事务，也不让一条 SQL 横跨多个节点。

这样可以将最复杂的数据库事务、锁、WAL 和 ACID 语义交给 SQLite。

---

## 5. API Server 职责

API Server 是整个系统的入口和 Control Plane Gateway。

主要职责：

```text
Authentication
Authorization
Tenant isolation
Database lifecycle
Cell routing
Quota
Rate limiting
Request validation
Node selection
```

典型接口：

```http
POST   /v1/databases
GET    /v1/databases
DELETE /v1/databases/:id

POST   /v1/databases/:id/sql

GET    /v1/databases/:id/kv/:key
PUT    /v1/databases/:id/kv/:key
DELETE /v1/databases/:id/kv/:key
```

创建 Cell 时，不需要启动新容器，也不一定立即创建 SQLite 文件。

例如：

```text
POST /v1/databases
        ↓
创建 metadata
        ↓
分配 database_id
        ↓
选择 Data Node
        ↓
返回
```

第一次真正写入数据时再 Lazy Create。

---

## 6. Metadata DB

Metadata DB 只存 Combee 自身的控制面数据，不存用户业务数据。

第一版直接使用 PostgreSQL。

核心信息：

```text
users
api_keys
databases
storage_nodes
database_locations
quotas
usage
status
```

基础表可以类似：

```sql
CREATE TABLE databases (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    storage_node_id UUID NOT NULL,
    storage_key TEXT NOT NULL,
    size_bytes BIGINT NOT NULL DEFAULT 0,
    state TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    last_accessed_at TIMESTAMPTZ
);
```

关键原则：

> Metadata DB 是目录，不是用户数据库。

不要逐渐把用户数据塞回 Metadata PostgreSQL，否则系统会重新退化成“所有租户共享一个大 PostgreSQL”。

---

## 7. Data Node

Data Node 是 Combee 真正的数据面。

职责：

```text
SQLite database lifecycle
Connection management
SQL execution
KV execution
Shared memory cache
TTL handling
WAL
Disk IO
Active database lifecycle
```

第一版存储：

```text
/data/
  00/
  01/
  ...
  ff/
      <database-id>.sqlite
```

每个 Cell 对应一个 SQLite 文件。

例如：

```text
db_001.sqlite
db_002.sqlite
db_003.sqlite
```

但：

> **一个 SQLite 文件不等于一个常驻进程。**

这是整个架构能够支撑海量 tiny database 的核心。

---

## 8. SQL 模型

第一版 SQL 直接使用 SQLite。

不要自己实现：

```text
SQL parser
Query planner
B+ Tree
Transaction engine
WAL
Lock manager
```

这些全部交给 SQLite。

用户可以使用普通 SQL：

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);

SELECT * FROM users WHERE id = ?;
```

第一版目标：

- arbitrary SQLite SQL；
- parameter binding；
- transaction；
- WAL mode；
- 每个 Cell 独立事务域。

---

## 9. KV / Redis-style 模型

第一版计划同时覆盖大量 Redis 使用场景，但不真正运行 Redis Server。

不宣传：

> Redis Compatible

更准确的定位：

> **Serverless KV / Redis-style KV API**

第一版支持：

```text
GET
SET
DEL
EXISTS
MGET
MSET
TTL
EXPIRE
PERSIST
INCR
DECR
INCRBY
SET NX
SET XX
```

主要覆盖：

```text
session
OTP
cache
temporary token
rate limit
counter
idempotency key
simple lock
short-lived application state
```

第一版不做：

```text
Pub/Sub
Streams
Lua
Redis Functions
List
Set
Sorted Set
Cluster protocol
RESP compatibility
```

---

## 10. SQL 与 KV 的统一存储模型

第一版 SQL 和 KV 可以共享同一个 SQLite 文件。

例如：

```text
customer.sqlite

users
posts
orders
...

__sys_kv
__sys_meta
```

内部 KV 表：

```sql
CREATE TABLE __sys_kv (
    key BLOB PRIMARY KEY,
    value BLOB NOT NULL,
    expires_at INTEGER,
    version INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX __sys_kv_expires_at
ON __sys_kv(expires_at);
```

用户不能直接访问 `__sys_*` 内部表。

这样 SQL 与 KV 可以共享：

```text
Persistence
Backup
Snapshot
Quota
Migration
Branching（未来）
```

同时也保留未来提供 SQL + KV 原子事务的可能性。

例如未来：

```ts
await db.transaction(async tx => {
    await tx.sql(...)
    await tx.kv.set(...)
})
```

---

## 11. KV 性能模型

单纯把 KV 直接映射到 SQLite 磁盘，会和用户对 Redis 的延迟预期产生差距。

因此 KV 采用：

> **Memory Serving Layer + Durable Storage Layer**

架构：

```text
KV Request
    │
    ▼
Shared Memory Cache
    │
    ├── hit → return
    │
    └── miss
          ↓
       SQLite
          ↓
        NVMe
```

热 key：

```text
network
→ API
→ Data Node
→ memory lookup
→ return
```

不访问磁盘。

冷 key 才访问 SQLite。

---

## 12. Shared KV Cache

不要为每个 Cell 创建一个独立 HashMap。

采用全局共享 KV Cache：

```text
Global Cache
```

实际 key：

```text
(database_id, key)
```

概念结构：

```rust
struct CacheKey {
    database_id: DatabaseId,
    key: Bytes,
}
```

整个 Data Node 共享一个 memory budget：

```text
64 GB RAM
   ↓
all active databases share cache
```

而不是：

```text
100000 databases
×
64 MB dedicated memory
```

这允许大量冷 Cell 几乎不占 RAM。

---

## 13. TTL

第一版采用两层策略。

### Lazy expiration

```text
GET key
   ↓
检查 expires_at
   ↓
过期 → 当不存在
```

### Background GC

后台周期删除已过期 key：

```sql
DELETE FROM __sys_kv
WHERE expires_at < unixepoch()
LIMIT 1000;
```

不需要第一版就实现复杂的精确定时器系统。

---

## 14. Durability

KV 写性能与 durability 需要明确区分。

可以预留两种模式：

### Fast / eventual durable

```text
SET
 ↓
memory
 ↓
WAL / buffer
 ↓
ACK
 ↓
background flush
```

目标是接近 Redis-style latency。

### Strict durable

```text
SET
 ↓
WAL
 ↓
fsync
 ↓
ACK
```

第一版可以只实现一种稳定语义，但内部 API 应避免把 durability 写死，为以后扩展留下空间。

---

## 15. Active Database Manager

这是 V0 中最值得认真设计的核心组件之一。

目标：

> 逻辑数据库可以非常多，但真正活跃的数据库连接和内存资源必须有上限。

状态模型：

```text
COLD
 ↓ request
ACTIVE
 ↓ idle timeout
COLD
```

激活：

```text
request
  ↓
lookup database
  ↓
open SQLite connection
  ↓
load runtime state
  ↓
execute
```

休眠：

```text
idle timeout
  ↓
checkpoint
  ↓
close connection
  ↓
evict runtime
```

目标：

```text
100000 logical databases
≠
100000 open SQLite connections
```

---

## 16. SQLite 与 Tokio

`rusqlite` 是同步接口，不能直接把 SQLite 阻塞操作扔进 Tokio async executor。

第一版建议：

```text
async request
    ↓
connection manager
    ↓
blocking worker / spawn_blocking
    ↓
SQLite
```

后续可以升级为：

```text
Active Cell
   ↓
DB Worker / Actor
   ↓
SQLite Connection
```

这样同一 Cell 内的操作更容易串行化和管理事务状态。

---

## 17. 推荐技术栈

### Language

```text
Rust
```

理由：

- 适合长期运行的基础设施；
- 并发控制能力强；
- 内存开销可控；
- SQLite / Tokio / HTTP / gRPC 生态成熟；
- API Server 和 Data Node 可以共享大量类型和 crate。

### API Server

```text
Axum
Tokio
Tower
Serde
```

### Metadata

```text
PostgreSQL
SQLx
```

### Data Node

```text
Rust
rusqlite
Tokio
moka
```

### Internal RPC

V0：

```text
Local trait / HTTP JSON
```

V1：

```text
tonic / gRPC
```

### Logging / Observability

```text
tracing
tracing-subscriber
```

### Deployment

V0：

```text
Docker Compose
local NVMe
```

暂时不使用 Kubernetes。

---

## 18. 推荐仓库结构

```text
combee/
├── Cargo.toml
├── crates/
│   ├── common/
│   │   ├── ids
│   │   ├── errors
│   │   ├── protocol
│   │   └── config
│   │
│   ├── api-server/
│   │   ├── auth
│   │   ├── database
│   │   ├── sql
│   │   ├── kv
│   │   └── router
│   │
│   ├── data-node/
│   │   ├── sqlite
│   │   ├── kv
│   │   ├── cache
│   │   ├── ttl
│   │   ├── connection
│   │   └── storage
│   │
│   └── metadata/
│       └── postgres
│
├── migrations/
├── tests/
├── benchmark/
├── docker-compose.yml
└── README.md
```

---

## 19. V0 可以先不拆 Data Node 进程

虽然逻辑架构是：

```text
API Server
Metadata DB
Data Node
```

但第一版开发时可以先运行成：

```text
┌────────────────────────────┐
│       server process       │
│                            │
│ API Server                 │
│      │                     │
│ DataNodeClient Trait       │
│      │                     │
│ Local Data Node            │
│      │                     │
│ SQLite                     │
└────────────┬───────────────┘
             │
          PostgreSQL
```

先定义抽象：

```rust
trait DataNodeClient {
    async fn execute_sql(...);
    async fn kv_get(...);
    async fn kv_set(...);
}
```

V0：

```text
LocalDataNodeClient
```

后续：

```text
GrpcDataNodeClient
```

这样可以极大降低第一周的分布式调试成本。

---

## 20. V0 Scope

第一版必须完成：

### Database lifecycle

```text
CREATE DATABASE
DELETE DATABASE
LIST DATABASE
```

### SQL

```text
arbitrary SQLite SQL
parameter binding
transaction
WAL
```

### KV

```text
GET
SET
DEL
EXISTS
MGET
MSET
TTL
EXPIRE
INCR
DECR
SET NX
```

### Platform

```text
API key
tenant isolation
quota
connection manager
shared cache
cache eviction
lazy TTL expiration
background TTL GC
graceful shutdown
integration tests
benchmark
Docker Compose
```

---

## 21. V0 明确不做

```text
Redis RESP protocol
Redis Pub/Sub
Redis Streams
List / Set / ZSet
Lua

PostgreSQL compatibility
MySQL compatibility

cross-node SQL
distributed transaction
replication
automatic failover
cross-node migration

branching
snapshot
PITR
cold object storage
S3

billing
Web dashboard
Kubernetes
```

控制 Scope 是第一版能否在一周左右完成的关键。

---

## 22. 第一版性能目标

不追求和裸 Redis 做极限吞吐竞争。

Combee 的目标场景是：

```text
AI generated app
small SaaS
preview deployment
test environment
personal project
```

主要价值是：

```text
zero instance
zero manual provisioning
near-zero idle resource usage
millisecond-level access
```

建议第一阶段内部目标：

| 操作 | 初始目标 |
|---|---:|
| KV hot GET p50 | < 1 ms |
| KV hot GET p99 | < 5 ms |
| Fast SET p99 | < 5 ms |
| Strict durable SET p99 | < 20 ms |
| Cold GET | < 20 ms |
| Simple SQL p99 | < 20 ms |

以上不包含公网 RTT。

更重要的 Benchmark：

> **一台机器可创建 100,000 个逻辑 Cell，但只有少量活跃 Cell 真正占用连接和内存。**

第一阶段更应该证明：

```text
100000 logical databases
≠
100000 database instances
```

而不是追求 Redis Benchmark 世界纪录。

---

## 23. 预计开发周期

在开发者负责架构、Review 和测试，Coding Agent 大量承担 boilerplate、CRUD、测试代码的前提下：

### 2～3 天

可完成 PoC：

```text
create database
SQL execution
GET / SET / DEL
TTL
SQLite persistence
basic metadata
```

### 5～7 个有效开发日

可完成 MVP：

```text
API Server
Metadata PostgreSQL
Local Data Node
SQL
KV
TTL
cache
quota
auth
WAL
connection lifecycle
integration tests
benchmark
Docker Compose
```

### 10～14 天

可以进一步拆出真正独立 Data Node：

```text
API Server
    │
Metadata DB
    │
┌───┼───┐
│   │   │
N1  N2  N3
```

并加入：

```text
node registration
heartbeat
capacity reporting
database placement
health check
internal RPC
metrics
```

---

## 24. 后续演进路线

### V1 — Multi-node

```text
API
 ↓
Metadata
 ↓
Data Nodes
```

增加：

```text
node heartbeat
node discovery
placement
capacity scheduler
```

### V2 — Replication / Migration

```text
Cell
 ↓
Primary Data Node
 ↓
Replica
```

增加：

```text
migration
replication
failure recovery
```

### V3 — Storage / Compute Separation

```text
API
 ↓
Query Router
 ↓
Compute Nodes
 ↓
Page / Storage Layer
 ↓
NVMe + Object Storage
```

### V4 — Branching

利用 page/chunk + copy-on-write：

```text
production
├── page A
├── page B
├── page C

preview
├── page A
├── page X
├── page C
```

支持：

```text
snapshot
clone
branch
preview database
auto-expire
time travel
```

这对 AI 生成应用非常有价值：

```text
AI 修改应用
    ↓
clone / branch database
    ↓
preview deployment
    ↓
test
    ↓
merge or delete
```

---

## 25. 核心差异化方向

Combee 不应该只是：

> SQLite + HTTP API

真正值得做的部分是：

```text
massive tiny database lifecycle
multi-tenant scheduling
active database management
shared KV memory layer
SQL + KV unified runtime
zero-instance provisioning
preview / ephemeral database
```

长期产品定义可以是：

> **A serverless data runtime for AI-generated applications.**

或者：

> **Every app gets a Cell.**

---

## 26. 当前最重要的设计原则

1. **一个 Cell 任意时刻只属于一个 Data Node。**
2. **一个 Cell 不对应一个进程。**
3. **冷 Cell 不应该常驻连接和内存。**
4. **SQL 交给 SQLite，不自己重造数据库内核。**
5. **KV 是 Memory Serving Layer + Durable Storage。**
6. **SQL 与 KV 共用生命周期、配额、持久化和未来的快照能力。**
7. **第一版不追求 Redis / PostgreSQL 全兼容。**
8. **优先证明 many-small-database 模型成立。**
9. **第一版先单机逻辑分层，再拆物理节点。**
10. **AI 建站集成体验优先于传统 DBA 体验。**

---

## 27. V0 成功标准

如果第一版能做到：

```text
1 台普通服务器

创建 100,000 个逻辑 Cell

少量活跃 Cell 按需打开 SQLite

共享一个有上限的 KV Memory Cache

提供 SQL + KV + TTL API

无需为每个应用启动数据库 / Redis 容器
```

并能够稳定展示：

```text
create → use → idle → release → reactivate
```

那么 Combee 的核心架构假设就已经得到验证。

之后再决定是否投入到真正的多节点调度、复制、branching 和对象存储层。

