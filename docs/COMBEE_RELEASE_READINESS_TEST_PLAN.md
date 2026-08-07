# Combee Public Alpha 发布就绪测试计划

> 目标：验证 Combee 是否达到 **Public Alpha 可发布状态**。
>
> 本计划不是为了继续堆功能，而是主动寻找会阻止公开发布的缺陷，重点覆盖数据安全、隔离、故障恢复、资源安全、文档可用性和跨环境可部署性。

---

## 1. 发布标准定义

### Public Alpha 允许缺少

- 完整 Redis 协议兼容
- PostgreSQL wire protocol
- 多副本
- 多 API Server HA
- 完整计费系统
- capacity-aware scheduler
- rolling upgrade
- 企业级 SLO

### Public Alpha 不能存在

- 数据损坏
- 跨 Cell / 跨租户访问
- silent data loss
- split-brain write
- 备份无法恢复
- 明显资源泄漏
- README 无法完成首次部署
- 用户输入触发 Data Node panic / crash
- 故障后出现无法解释的数据状态

---

## 2. 统一 Release Gate

建议新增：

```text
tests/release/
```

以及统一执行入口：

```bash
cargo xtask release-test
```

或：

```bash
./scripts/release-test.sh
```

最终输出格式建议：

```text
COMBEE RELEASE GATE

Functional       PASS
Durability       PASS
Failure Recovery PASS
Isolation        PASS
Security         PASS
Resource Safety  PASS
Compatibility    PASS
Upgrade          PASS
Documentation    PASS
Performance      PASS

RESULT: RELEASEABLE / NOT RELEASEABLE
```

同时生成：

```text
docs/RELEASE_READINESS.md
```

每个问题按严重级别分类：

```text
BLOCKER
HIGH
MEDIUM
LOW
```

---

# 3. Fresh Install Test

模拟第一次接触 Combee 的用户，从完全干净环境开始。

禁止依赖：

- 旧 Docker volume
- 旧数据库
- 手工初始化
- 开发机已有环境变量
- 开发机已有目录状态

执行：

```bash
git clone ...
docker compose up -d --build
```

验证：

| 项目 | 预期 |
|---|---|
| PostgreSQL 自动初始化 | PASS |
| MinIO bucket 初始化 | PASS |
| Data Node 自动注册 | PASS |
| API Server readiness | PASS |
| 创建 Cell | PASS |
| 首次 SQL 自动 materialize | PASS |
| KV SET / GET | PASS |
| 重启所有容器后数据仍存在 | PASS |

关键场景：

```bash
docker compose down
docker compose up -d
```

随后读取此前写入的数据，必须成功。

---

# 4. Golden Path E2E

构造一个真实小型 Web App workload，而不是只测试单端点。

创建一个 Cell：

```text
SQL:
- users
- posts

KV:
- session
- rate-limit
- page-cache
```

执行顺序：

```text
create Cell

CREATE users
CREATE posts

INSERT user
INSERT 100 posts

SET session:user:1
SET cache:homepage TTL=5m
INCR pageviews

SELECT posts
GET session
GET cache

UPDATE user
DELETE post
```

最终校验：

```text
SQL state exactly correct
KV state exactly correct
TTL correct
counter correct
```

随后依次：

```text
restart Data Node
restart API Server
restart PostgreSQL
```

每次都重新读取并验证状态。

通过标准：

> Combee 能持续承载一个真实的小型 Web App，而不仅仅是若干独立 API。

---

# 5. Cell / Tenant Isolation

这是 P0 发布门槛。

创建：

```text
Cell A
Cell B
```

两边都写入同名数据：

```text
key = secret
table = users
```

但内容不同。

测试：

| 行为 | 预期 |
|---|---|
| A SQL 查询 B | 不可能 |
| A KV GET B | 不可能 |
| A 删除 B | 403 / 404 |
| A restore B backup | 拒绝 |
| A 设置 B replica | 拒绝 |
| API key A 操作 B | 拒绝 |
| 猜测 Cell UUID | 仍拒绝 |
| SQL 访问 `__sys_*` | 拒绝 |
| SQL ATTACH 外部 SQLite | 拒绝 |
| SQL 写任意 filesystem path | 不可能 |

建议新增：

```text
tests/release/sqlite_escape_test.rs
```

攻击样例：

```sql
ATTACH DATABASE '/tmp/foo.db' AS x;
PRAGMA ...;
VACUUM INTO ...;
.load ...;
```

目标：

> 用户 SQL 不能突破 Cell 的文件边界。

---

# 6. Crash Matrix

不要只测试 graceful shutdown，直接制造故障。

| 故障 | 故障发生时 | 预期 |
|---|---|---|
| kill API Server | GET | 重启后恢复 |
| kill API Server | SET | 数据不损坏 |
| kill Data Node | GET | failover / 明确失败 |
| kill Data Node | SET | 成功或明确失败，不允许 silent loss |
| kill Primary | transaction 中 | 数据保持合法事务状态 |
| kill Replica | replication 中 | Primary 不受影响 |
| kill PostgreSQL | 请求中 | Data Node 数据不损坏 |
| kill MinIO | backup 中 | 主数据不受影响 |
| kill Data Node | WAL archive 中 | restore 仍合法 |

原则：

> 请求可以失败，但数据不能进入无法解释的状态。

例如：

```text
SET
↓
node crashes
```

客户端可能得到：

```text
500
connection reset
```

恢复后 old value 或 new value 都可以接受，取决于 durability 语义。

不可接受：

```text
corrupted SQLite
silent partial write
unknown mixed state
```

---

# 7. Kill -9 During Writes

持续执行：

```text
BEGIN
INSERT
UPDATE
COMMIT
```

同时随机执行：

```bash
kill -9 <data-node-pid>
```

建议重复：

```text
100 ~ 1000 次
```

每次重启后执行：

```sql
PRAGMA integrity_check;
```

必须返回：

```text
ok
```

同时检查应用级 invariant，例如：

```text
accounts.balance >= 0
orders.user_id exists
counter within expected range
```

---

# 8. Network Partition + Failover Fencing

不要只做：

```text
docker stop primary
```

还要模拟：

```text
API Server 无法访问 Node A
但 Node A 自己仍然存活
```

场景：

```text
API ─X─ Node A

Node A 仍运行
```

然后：

```text
Node B promote
generation = N + 1
```

接着绕过 API，直接对旧 Node A 发内部写请求：

```text
write generation = N
```

必须：

```text
REJECT
```

还要测试：

```text
write without generation
write generation = N-1
write generation = N
write generation = N+100
```

全部验证。

最高级 invariant：

> 任意时刻绝不能存在两个同时接受写入的 Primary。

---

# 9. Backup Must Be Restorable

不要只测试：

```text
backup API returns 200
```

完整流程：

```text
create DB
↓
generate 100MB representative data
↓
snapshot
↓
continue writes A
↓
incremental backup
↓
continue writes B
↓
destroy primary volume
↓
destroy replica volume
↓
restore only from MinIO / S3-compatible storage
```

恢复后验证：

```text
SQL rows
KV values
TTL
schema
indexes
transactions
```

建议额外生成逻辑 dump 并比较：

```text
sha256(logical dump before)
==
sha256(logical dump after restore)
```

发布门槛：

> 删除全部 Data Node 本地数据后，只依赖对象存储仍能恢复到有效状态。

---

# 10. Soak Test

持续运行混合 workload：

```text
SQL read/write
KV GET/SET/INCR
TTL expiration
Cell create/delete
LRU eviction
backup
replication
failover scan
```

建议：

```text
本地第一轮：12h
Public Alpha 前：24~72h
```

持续记录：

```text
RSS
FD
threads
CPU
SQLite connections
cache entries
Postgres connections
object count
error count
p50 / p95 / p99
```

验收重点：

```text
RSS 不持续单调增长
FD 不持续增长
connection 不泄漏
cache 有界
p99 不随运行时间持续恶化
```

例如：

```text
Hour 1 RSS   500MB
Hour 12 RSS  510MB
```

可以接受。

如果：

```text
Hour 1 RSS   500MB
Hour 12 RSS  2.7GB
```

则阻止发布。

---

# 11. API Fuzz / Malformed Input

推荐：

```text
cargo-fuzz
proptest
```

重点攻击：

```text
SQL params
KV key
KV value
database ID
JSON body
TTL
transaction statements
```

边界输入：

```text
empty
1 byte
Unicode
NUL
emoji
1MB
10MB
超大整数
negative TTL
TTL = 0
TTL = u64::MAX
invalid UUID
malformed JSON
```

验收：

```text
不 panic
不 OOM
不 crash
不越权
返回确定的 4xx / 可解释错误
```

全局规则：

> 用户输入永远不应该导致 Data Node panic。

---

# 12. Resource Exhaustion

主动测试：

```text
too many Cells
too many active Cells
too many concurrent requests
huge KV value
huge SQL result
disk full
fd exhaustion
memory pressure
```

尤其必须测试：

```text
ENOSPC
```

预期：

```text
write returns explicit error
existing DB remains readable where possible
process does not crash
SQLite remains valid
```

同时验证：

> 单个用户不能通过一个超大 value 或超大 SQL result 轻易拖死整个 Data Node。

---

# 13. Noisy Neighbor Test

创建：

```text
Cell A = attacker / heavy workload
Cell B = normal user
```

Cell A：

```text
持续 SET
复杂 SQL
大量 concurrent writes
```

Cell B：

```text
KV GET
simple SELECT
```

记录 Cell B：

```text
baseline p99
vs
under Cell A load p99
```

Alpha 可先设较宽松门槛：

```text
B p99 degradation < 5x
```

Beta 再收紧：

```text
< 2x
```

该测试回答：

> 一个坏邻居是否能拖死整个共享 Data Node。

---

# 14. Quota / Abuse Guard

即使完整计费尚未实现，也建议至少验证资源边界。

测试：

```text
max KV value size
max request body
max SQL result rows / bytes
max transaction statements
SQL timeout
max active DB
max request concurrency
```

特别测试：

```sql
WITH RECURSIVE ...
```

以及巨大 Cartesian Join。

如果一个 Cell 可以无限占满 CPU，就应该标记为发布风险。

Release Gate 中至少明确输出：

```text
PASS
WARN
KNOWN UNSAFE
```

---

# 15. Version Upgrade Test

测试：

```text
Combee version N
↓
create Cells
write SQL / KV
backup
↓
stop
↓
upgrade to N+1
↓
start
```

验证：

```text
metadata migration works
old SQLite works
old __sys_kv schema works
backup remains restorable
replica remains usable
```

建议现在就建立：

```text
tests/fixtures/v0/
```

保存 release fixture。

以后每个版本都跑：

```text
old-version fixture
→ current binary
```

---

# 16. Documentation Test

使用一个没有项目上下文的 Agent，只允许读取 README，不准看源码。

任务：

> 按照 README 从空环境部署 Combee，创建一个 Cell，创建 users 表，插入一个用户，写一个 session KV，然后读取回来。

如果失败：

```text
README FAIL
```

这应作为正式 Release Gate，而不是人工感觉。

---

# 17. Clean Linux Environment

在不同于开发机的环境验证：

```text
Ubuntu x86_64
```

推荐 GitHub Actions runner。

只执行：

```bash
git clone
docker compose up -d --build
cargo test --workspace
./scripts/release-test.sh
```

发布前必须证明：

> Combee 不依赖开发者本机环境才能工作。

---

# 18. Performance Regression Gate

不是要求每次都刷新纪录，而是防止重大性能倒退。

建议保存当前 baseline：

```text
hot KV GET p99
SET p99
SQL SELECT p99
mixed workload p99
1M logical Cell RSS
5k active Cell RSS
end-to-end p99
```

每次 release 对比。

例如允许：

```text
p99 regression <= 20%
RSS regression <= 15%
```

超过则：

```text
WARN / FAIL
```

具体阈值可根据后续数据调整。

---

# 19. Release Levels

## Alpha

要求：

```text
✓ Fresh install
✓ Golden path
✓ Cell isolation
✓ Crash recovery
✓ Backup / restore
✓ Failover fencing
✓ Basic fuzz
✓ 12h soak
✓ Clean Linux
✓ README from scratch
```

达到后：

> 可以公开 GitHub，并邀请外部用户测试。

---

## Beta

额外要求：

```text
24~72h soak
noisy-neighbor isolation
resource quotas
upgrade test
stable API
metrics
persistent node identity
SDK
```

---

## Production

再讨论：

```text
multi API Server
multi replica
formal disaster recovery
SLO
security audit
billing correctness
TLS everywhere
rolling upgrades
load shedding
capacity scheduler
```

---

# 20. 建议给本地 Agent 的执行 Prompt

```text
对 Combee 执行一次 Public Alpha Release Readiness Audit。

不要继续新增产品功能，目标是主动寻找阻止公开发布的缺陷。

基于现有单元测试、集成测试、benchmark 和 Docker Compose 环境，新增 tests/release 与统一 release test runner。

必须测试：

- fresh install
- SQL + KV golden path
- 跨 Cell / tenant 隔离
- 进程 kill -9
- Data Node / API Server / PostgreSQL / MinIO 故障
- SQLite integrity_check
- network partition + failover fencing
- 删除所有 Data Node volume 后从对象存储恢复
- 资源泄漏 soak test
- 恶意 / 异常 API 输入
- 磁盘满 / 连接上限 / 超大请求等资源耗尽
- noisy-neighbor
- 旧版本数据升级兼容
- clean Linux 环境部署
- 严格按照 README 从零完成 quickstart

对每项输出 PASS / FAIL / WARN，并提供：

- 可复现命令
- 实际结果
- 预期结果
- 失败原因

不得为了让测试通过而降低断言或跳过失败场景。

发现产品缺陷时先记录 root cause，再修复并增加 regression test。

最终生成 docs/RELEASE_READINESS.md，给出：

BLOCKER / HIGH / MEDIUM / LOW

缺陷清单，以及明确结论：

NOT RELEASEABLE
PUBLIC ALPHA READY
BETA READY

Public Alpha 的原则是：允许缺少高级功能，但不能存在已知的数据损坏、跨租户访问、silent data loss、split-brain write、不可恢复备份、明显资源泄漏或 README 无法完成首次部署的问题。
```

---

# 21. 最终判定原则

如果 Release Gate 跑完以后：

```text
BLOCKER = 0
HIGH     = 0
```

且：

```text
Fresh install       PASS
Isolation           PASS
Durability          PASS
Failure recovery    PASS
Backup restore      PASS
Fencing             PASS
Soak                 PASS
Clean Linux         PASS
Documentation       PASS
```

那么可以认为：

> **Combee 已达到 `v0.1.0-alpha` / Public Alpha 的基本发布条件。**

此时剩余 MEDIUM / LOW 问题可以进入公开 issue / roadmap，而不必阻止首次发布。
