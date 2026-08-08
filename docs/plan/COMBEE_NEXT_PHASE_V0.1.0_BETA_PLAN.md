# Combee 下一阶段目标与执行计划

> 目标版本：`v0.1.0-beta`
>
> 阶段定位：从“数据库内核 / Public Alpha 可发布”进入“可被真实用户自助使用、可计量、可计费、可通过 SDK 和 Web Console 接入”的产品化阶段。
>
> 本阶段原则：**冻结数据库核心能力，不继续扩张协议和存储特性；优先完成 Usage → Credits → Public API → SDK → Console → Cloud Alpha 的闭环。**

---

# 1. 阶段目标

Combee 当前已经具备：

- Cell 生命周期；
- SQLite SQL；
- Redis-style KV + TTL；
- 共享 KV Cache；
- 多租户隔离；
- 多 Data Node；
- placement / routing；
- snapshot / WAL incremental backup；
- replica；
- automatic failover；
- generation fencing；
- release readiness gate。

下一阶段不再以“新增数据库能力”为中心，而是回答：

> **一个陌生开发者能否注册 Combee、获得 API Key、创建 Cell、通过 SDK 使用 SQL/KV、查看 Usage 和 Credits，并且无需理解 Data Node / SQLite / WAL / placement 等内部细节？**

`v0.1.0-beta` 的目标 UX：

```text
Register
   ↓
Create API Key
   ↓
Create Cell
   ↓
Use TS / Python SDK
   ↓
SQL + KV
   ↓
View Usage
   ↓
Consume Credits
```

---

# 2. V0 Feature Freeze

在 `v0.1.0-beta` 完成前，以下能力暂不进入主线：

```text
Redis RESP compatibility
PostgreSQL wire protocol
Blob storage
Vector storage
Queue
multi-replica (>1)
distributed SQL sharding
page server
compute/storage separation
advanced scheduler
Kubernetes operator
```

这些能力进入 V1/V2 backlog。

当前最重要的不是扩大功能面，而是把现有能力做成可以真正交付的产品。

---

# 3. 总体实施顺序

```text
P0  Usage Metering
        ↓
P1  Pricing + Credits Ledger + Voucher
        ↓
P2  Public API Freeze + OpenAPI
        ↓
P3  TypeScript SDK + Python SDK
        ↓
P4  User Control Plane / Web Console
        ↓
P5  Cloud Alpha Deployment
        ↓
v0.1.0-beta
```

---

# 4. P0 — Usage Metering

Usage Metering 必须是下一阶段第一优先级。

Credits、Pricing、Console 和未来 Billing 都依赖可靠的 usage 数据。

## 4.1 目标

系统能够按：

```text
tenant
cell
operation type
time window
```

统计实际资源使用量。

至少覆盖：

```text
KV read operations
KV write operations

SQL read operations
SQL write operations

requests
bytes in
bytes out

current storage bytes
storage byte-hours（可在 beta 后完善）
```

---

## 4.2 Usage Event 模型

建议标准化为内部事件：

```text
tenant_id
cell_id
operation_type

request_count

bytes_in
bytes_out

timestamp
```

Operation Type 至少区分：

```text
kv_read
kv_write
sql_read
sql_write
```

以后可扩展：

```text
backup
restore
replication
storage
egress
```

---

## 4.3 不允许每请求同步写 PostgreSQL

禁止：

```text
KV GET
 ↓
INSERT usage_event
 ↓
return
```

Usage Metering 不得成为 Data Plane 热路径瓶颈。

推荐：

```text
Request
  ↓
local usage counter
  ↓
in-memory aggregation
  ↓
periodic batch flush
  ↓
PostgreSQL
```

例如：

```text
flush every 1–10 seconds
or
flush every N operations
```

聚合键：

```text
(tenant_id, cell_id, metric, time_bucket)
```

---

## 4.4 PostgreSQL 表建议

```text
usage_buckets
-------------
id
tenant_id
cell_id
metric
bucket_start
value
bytes_in
bytes_out
pricing_version nullable
created_at
updated_at
```

时间粒度第一版可以：

```text
1 minute
```

控制台再聚合为 hour/day/month。

---

## 4.5 Usage API

至少提供：

```text
GET /v1/usage/summary
GET /v1/usage/timeseries
GET /v1/cells/:id/usage
```

支持：

```text
from
to
interval
metric
```

---

## 4.6 P0 验收标准

```text
[x] Usage 统计不进入 Data Node 同步写路径(API Server 内存聚合 + 周期 flush,热路径仅 ~30ns 锁累加)
[x] 支持 tenant 聚合(/v1/usage/summary)
[x] 支持 Cell 聚合(/v1/cells/:id/usage)
[x] SQL read/write 可区分(handler 启发式:SELECT/WITH/PRAGMA/EXPLAIN → read)
[x] KV read/write 可区分(GET/EXISTS/MGET/TTL → read;SET/DEL/MSET/EXPIRE/INCR → write)
[x] bytes_in / bytes_out 可统计(请求/响应 body 包装计数,不依赖 content-length)
[x] storage bytes 可查询(/v1/cells/:id/usage 与 summary 返回 current_storage_bytes,主库+WAL)
[x] 批量 flush 后数据不重复(usage_add ON CONFLICT 累加;失败回收重试;tests/usage.rs)
[x] API Server 重启不会产生明显 double counting(未 flush 窗口仅 under-count,不重复)
[x] 并发请求下计数准确(tests/usage.rs::usage_concurrent_records_are_accurate 8×250)
[x] release tests 覆盖 usage correctness(tests/usage.rs 4 用例)
[x] Usage API 可被 Console / SDK 使用(RFC3339 from/to/interval/metric;tenant 隔离)
```

> **2026-08-08 已实现**:见 `crates/api-server/src/usage.rs`(UsageMeter)、
> `crates/metadata`(usage_buckets 表)、`crates/api-server/src/handlers/usage.rs`(三个端点)、
> `tests/usage.rs`。配置 `COMBEE_USAGE_FLUSH_INTERVAL_SECS`(默认 5s)。

---

# 5. P1 — Pricing System

Pricing 必须是：

> **可配置、可版本化、无需重启、可热更新。**

---

## 5.1 Pricing 与 Usage 分离

必须保留两个独立概念：

```text
Metering:
发生了多少使用量？

Rating:
这些使用量按照当前价格值多少 Credits？
```

禁止直接在 Data Node 中写死：

```rust
const KV_READ_PRICE = ...
```

---

## 5.2 Pricing Version

推荐：

```text
pricing_versions
----------------
id
version
status
effective_at
created_at
created_by
```

规则：

```text
pricing_rules
-------------
pricing_version
metric
unit_size
price_units
```

例如：

```text
version = 7

kv_read
1,000 ops
10 credit units

kv_write
1,000 ops
40 credit units
```

---

## 5.3 热更新

第一版不需要 Pub/Sub。

可以：

```text
PricingManager
    ↓
poll PostgreSQL every 5s
    ↓
version changed?
    ↓
atomic replace Arc<PricingConfig>
```

要求：

```text
修改 Pricing 后无需重启
```

---

## 5.4 历史计价必须可重放

每次结算必须知道：

```text
pricing_version
```

否则价格发生变化后无法解释历史 Credits 消耗。

---

## 5.5 Pricing 验收标准

```text
[x] Pricing 存于 PostgreSQL(pricing_versions + pricing_rules;InMemory 同步)
[x] 支持多个 Pricing Version(create 自动 max+1,旧 active → inactive)
[x] active version 可切换(创建新版本即激活)
[x] 热更新无需重启(PricingManager 5s 轮询,Arc 原子替换;COMBEE_PRICING_REFRESH_INTERVAL_SECS)
[x] usage rating 记录 pricing_version(settlement 每条 usage 账本带版本)
[x] 历史账单不受新价格覆盖(账本 append-only,reference 幂等)
[x] invalid pricing config 不会替换当前有效配置(unit_size/price_units<=0 拒绝;admin API 直接 400)
[ ] pricing 变更有 audit metadata(admin_audit_log 待 P1 后续 §17)
```

---

# 6. P1 — Credits Ledger

Credits 应属于 Metadata / Control Plane PostgreSQL。

它不是 Data Node 的一部分。

---

## 6.1 Credits 不能只用一个 balance 字段

不要只：

```text
tenants.balance
```

正确模型：

```text
credit_accounts
credit_transactions
```

余额可以缓存，但**账本是事实来源**。

---

## 6.2 单位

所有 Credits 必须使用整数最小单位。

例如：

```text
1 Credit = 1,000,000 microcredits
```

数据库：

```text
BIGINT
```

不要用：

```text
FLOAT / DOUBLE
```

---

## 6.3 Ledger

```text
credit_transactions
-------------------
id
tenant_id

type
amount_units

pricing_version nullable
reference_id nullable

description
created_at
```

类型至少：

```text
recharge
usage
grant
voucher
refund
adjustment
```

Ledger 规则：

```text
append-only
```

已经落账的 transaction 不 UPDATE。

修正使用新的 compensating transaction。

---

## 6.4 用户余额

API：

```text
GET /v1/credits/balance
GET /v1/credits/transactions
```

返回：

```text
available
reserved
updated_at
```

---

## 6.5 Admin Grant

需要支持运营者主动给特定 Tenant 增加 Credits。

例如：

```text
POST /admin/tenants/:tenant_id/credits/grant
```

Payload：

```json
{
  "amount_units": 50000000,
  "reason": "alpha tester"
}
```

用途：

```text
Alpha tester grant
bug compensation
promotion
partnership
manual adjustment
```

此接口不得使用普通 tenant `cmb_sk_*` key。

推荐单独：

```text
COMBEE_ADMIN_TOKEN
```

以后再演进到 operator IAM。

---

# 7. P1 — Credit Voucher / 兑换卡

早期阶段非常推荐。

它可以暂时代替完整支付系统。

例如：

```text
CMB-84PA-X2KD-7MQR
```

用户：

```text
Redeem Code
    ↓
+ 50 Credits
```

---

## 7.1 Voucher Schema

```text
credit_vouchers
---------------
id
code_hash
amount_units

status

campaign nullable

created_at
expires_at nullable

redeemed_by nullable
redeemed_at nullable
```

不要存完整兑换码明文。

---

## 7.2 Redeem API

```text
POST /v1/credits/redeem
```

Payload：

```json
{
  "code": "CMB-..."
}
```

必须保证：

```text
single-use
transactional
idempotent
concurrency safe
```

---

## 7.3 Voucher 验收标准

```text
[x] voucher code 存 hash(CMB-XXXX-XXXX-XXXX → sha256 hex 前 16 字节)
[x] 过期 voucher 不可使用(redeem 校验 expires_at)
[x] 同一 voucher 无法并发重复兑换(原子 UPDATE status='active'→'used';InMemory 单锁 / Postgres 事务)
[x] redeem 同一请求重试不会重复加 Credits(reference_id=voucher:{hash} 幂等,重试返回 already_redeemed)
[x] 兑换成功写 Credits Ledger(type=voucher)
[x] 可以记录 campaign(create_vouchers campaign 字段)
[x] admin 可批量生成 voucher(POST /admin/vouchers/generate,count<=1000)
```

---

# 8. Credits 扣费策略

第一版建议：

```text
Usage
  ↓
bucket
  ↓
Rating
  ↓
Credits Ledger
```

不要：

```text
每次请求
↓
查 balance
↓
同步扣 credits
↓
执行数据库请求
```

---

## 8.1 Settlement

可以每：

```text
10s
1m
```

结算 usage bucket。

产生：

```text
credit_transaction(type=usage)
```

---

## 8.2 Credits Exhaustion

Beta 建议先支持：

```text
soft limit
hard limit
```

Alpha/Beta 初期可以默认 soft：

```text
balance <= 0
↓
warning
```

以后正式 Cloud：

```text
balance <= hard limit
↓
rate limit / suspend writes / suspend requests
```

不要一开始因为 accounting bug 直接切断用户数据服务。

---

# 9. P2 — Public API Freeze

在 SDK 开始实现前，必须冻结 Beta Public API。

---

## 9.1 Public API 分层

```text
User Data Plane
---------------
SQL
KV

User Control Plane
------------------
Cells
API Keys
Usage
Credits
Backups
Replication

Internal Control Plane
----------------------
Node Registry
Heartbeat
Placement
Failover internals
Fencing
RPC
```

---

## 9.2 内部接口禁止出现在 SDK

禁止：

```text
/internal/*
/rpc/*
Node registration
heartbeat
storage_node_id reassignment
raw generation mutation
internal WAL archive
internal replica pull
```

---

## 9.3 OpenAPI

`v0.1.0-beta` 前推荐生成：

```text
openapi.json
```

作为：

```text
SDK
Console
Docs
contract tests
```

共同依据。

OpenAPI 可以生成 DTO / low-level transport，但：

> 高层 SDK API 不应直接使用自动生成的丑陋 endpoint 命名。

---

## 9.4 API Freeze 验收标准

```text
[ ] Public API 列表冻结
[ ] Internal API 明确标记
[ ] Error code 稳定
[ ] Pagination 规范确定
[ ] request-id 规范确定
[ ] Idempotency-Key 规范确定
[ ] Usage API 完成
[ ] Credits API 完成
[ ] Voucher API 完成
[ ] OpenAPI 可生成
```

---

# 10. P3 — TypeScript SDK

独立仓库：

```text
combee-js
```

包：

```text
@combee/sdk
```

核心 UX：

```ts
const combee = new Combee({
  baseUrl,
  apiKey,
});

const cell = await combee.cells.create({
  name: "blog-prod",
});

await cell.sql.query(...);
await cell.sql.execute(...);

await cell.kv.get(...);
await cell.kv.set(...);
```

详细接口见：

```text
COMBEE_V0.1.0_BETA_SDK_SPEC.md
```

---

# 11. P3 — Python SDK

独立仓库：

```text
combee-python
```

PyPI：

```text
combee
```

必须支持：

```text
Combee
AsyncCombee
```

推荐：

```text
httpx
```

UX：

```python
from combee import Combee

client = Combee(
    base_url=COMBEE_URL,
    api_key=COMBEE_API_KEY,
)

cell = client.cells.create(name="blog")

cell.sql.execute(...)
cell.kv.set(...)
```

---

## 11.1 SDK Release Gate

TypeScript 和 Python 必须拥有相同的 required feature matrix。

至少测试：

```text
Cell CRUD
SQL CRUD
SQL transaction
KV full subset
TTL
backup / restore
API Key lifecycle
tenant isolation
Usage
Credits
Voucher
typed errors
retry
timeout
pagination
```

SDK contract tests 必须跑真实本地 Combee Server。

---

# 12. P4 — User Control Plane / Web Console

等 Usage、Credits 和 Public API 稳定后再正式接前端。

Console 必须使用：

```text
与 SDK 相同的 Public API
```

禁止给前端单独开一套绕过权限模型的私有接口。

---

## 12.1 页面

Beta 最小范围：

```text
Overview
Cells
Cell Detail
API Keys
Usage
Credits
Redeem Code
```

---

## 12.2 Overview

至少：

```text
Cells
Active Cells
Requests
Storage
Credits Balance
Recent Cells
```

---

## 12.3 Cells

```text
Create Cell
List
Delete
Status
Storage
Usage
```

Create Cell 必须简单：

```text
Name
Region

Create
```

不要暴露：

```text
CPU
RAM
Redis memory
Postgres version
connection pool
Data Node
```

---

## 12.4 Cell Detail

Tabs：

```text
Overview
SQL
KV
Backups
Replication
Usage
Settings
```

---

## 12.5 API Keys

支持：

```text
create
list
revoke
```

明文 key 只出现一次。

---

## 12.6 Usage

至少图表：

```text
Requests
KV reads/writes
SQL reads/writes
Storage
Bandwidth
Credits consumed
```

---

## 12.7 Credits

至少：

```text
Current balance
Transaction history
Redeem voucher
Low balance status
```

真正在线支付可以晚于 Beta。

---

# 13. P5 — Cloud Alpha Deployment

Cloud Alpha 不要求生产级 GA。

目标：

> 让第一批陌生开发者可以自己注册、拿 Key、创建 Cell，并通过 SDK 使用。

---

## 13.1 推荐初期基础设施

一台或少量 Linux 云机即可：

```text
Reverse Proxy / TLS
API Server
PostgreSQL
Data Node A
Data Node B
S3-compatible object storage
Web Console
```

不需要：

```text
Kubernetes
large cluster
multi-region
physical servers
```

---

## 13.2 Cloud Alpha 必须验证

```text
clean Linux deployment
TLS
restart
machine reboot
real filesystem behavior
disk pressure
long-running RSS
logs
object storage
backup restore
failover
control-plane auth
tenant auth
Usage accuracy
Credits accuracy
SDK connectivity
```

---

# 14. Observability

Beta 前至少加入：

```text
request_id
structured logs
basic Prometheus metrics
```

重点：

```text
request count
latency
errors
active Cells
open SQLite connections
KV cache hit rate
usage flush lag
usage flush errors
credit settlement lag
credit settlement errors
Data Node health
backup failures
replication lag
```

---

# 15. Security Boundaries

必须明确三种权限域：

```text
Tenant API Key
--------------
cmb_sk_...
仅操作自己的 Tenant / Cells

Internal Control Plane
----------------------
COMBEE_CONTROL_PLANE_TOKEN
Node registration / heartbeat / internal RPC

Operator / Admin
----------------
COMBEE_ADMIN_TOKEN
pricing change
credit grants
voucher generation
operator actions
```

三种 token 不得互相替代。

---

# 16. Metadata PostgreSQL 模块划分

推荐：

```text
Identity
--------
tenants
api_keys

Cell Directory
--------------
databases / cells
nodes
replication metadata

Metering
--------
usage_buckets

Pricing
-------
pricing_versions
pricing_rules

Credits
-------
credit_accounts
credit_transactions
credit_vouchers

Operator Audit
--------------
admin_audit_log
```

逻辑上属于同一 PostgreSQL 实例没有问题。

以后规模变大再考虑拆独立服务/数据库。

---

# 17. 必须新增的 Operator Audit

涉及 money-like state 的 admin 操作必须有审计记录。

```text
admin_audit_log
---------------
id
actor
action
tenant_id nullable
resource_id nullable
payload
created_at
```

至少记录：

```text
pricing changes
credit grants
credit adjustments
voucher generation
voucher invalidation
manual account suspension
```

---

# 18. Beta 发布前的测试新增

在现有 Release Gate 基础上新增：

## Usage

```text
concurrent counter correctness
restart correctness
no double count
batch retry
flush failure recovery
```

## Pricing

```text
hot reload
invalid config rollback
historical version correctness
```

## Credits

```text
integer accounting
concurrent settlement
no double charge
grant
refund
adjustment
balance reconstruction from ledger
```

## Voucher

```text
single redeem
concurrent redeem
expired
invalid
idempotent retry
```

## SDK

```text
TS contract suite
Python contract suite
sync/async parity
error parity
```

## Console

```text
tenant isolation
API key lifecycle
credits display
usage display
```

---

# 19. v0.1.0-beta 发布条件

满足以下条件后进入 Beta：

```text
[ ] Usage Metering 可用并通过 correctness tests
[ ] Usage 不阻塞 Data Plane hot path
[ ] Pricing 可配置并支持热更新
[ ] Pricing Version 可追溯
[ ] Credits Ledger append-only
[ ] Admin Grant 可用
[ ] Voucher 可生成/兑换
[ ] Credits 使用整数 base units
[ ] Public API Freeze
[ ] OpenAPI / API schema 可用
[ ] TypeScript SDK 发布
[ ] Python SDK 发布
[ ] TS/Python contract tests 全通过
[ ] Console 支持 Cell / API Key / Usage / Credits
[ ] Cloud Alpha Linux 环境稳定运行
[ ] BLOCKER = 0
[ ] HIGH = 0
[ ] Release Readiness 重新通过
```

---

# 20. 推荐阶段时间划分

在 AI Agent 强辅助情况下，可按以下顺序执行：

```text
Milestone A
Usage Metering

Milestone B
Pricing + Credits + Voucher

Milestone C
Public API Freeze + OpenAPI

Milestone D
TS SDK + Python SDK

Milestone E
Console

Milestone F
Cloud Alpha

Milestone G
v0.1.0-beta
```

不要严格以自然日作为门槛，以每个 Milestone 的验收条件为准。

---

# 21. Beta 后再考虑

Beta 发布并获得真实用户反馈后，再决定优先级：

```text
RESP
PG wire
Cell migration
capacity-aware scheduler
request packs
online recharge
budget limits
multi-region
multi-replica
cold storage
clone / branch
Blob
Vector
```

这些能力必须由真实 usage / user feedback 驱动。

---

# 22. 最终产品目标

下一阶段完成后，Combee 应该从：

```text
一个可以部署和测试的 Serverless Data Runtime
```

变成：

```text
一个陌生开发者可以自行注册并使用的 Cloud Product
```

理想用户路径：

```text
打开 Combee
   ↓
注册
   ↓
获得 / 创建 API Key
   ↓
创建 Cell
   ↓
npm install @combee/sdk
or
pip install combee
   ↓
SQL + KV
   ↓
查看 Usage
   ↓
查看 Credits
   ↓
兑换 Alpha Credits
```

用户无需理解：

```text
SQLite 文件
Data Node
Node ID
WAL
generation
placement
fencing
internal RPC
```

这就是 `v0.1.0-beta` 的核心完成标准。

---

# 23. 一句话执行原则

> **先把使用量算准，再把钱算准；先把 Public API 冻结，再做 SDK；SDK 和 Console 共用同一套 API；在真实用户出现之前，不继续扩张数据库内核。**
