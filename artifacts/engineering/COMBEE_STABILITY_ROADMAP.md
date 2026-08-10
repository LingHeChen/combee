# Combee Stability Engineering Roadmap

> Target: Combee Cloud Alpha / v0.1.x  
> Goal: Improve system reliability before opening the service to external users.

---

# 0. 实施进度(2026-08)

> 状态:✅ 已实施并验证 / 🚧 工具与文档就绪(持续实践项)/ ⬜ 远期未开始

| 章节 | 内容 | 状态 | 落地 |
|---|---|---|---|
| §3.1 | 备份/恢复自动化验证 | ✅ | `tests/release/backup_restore.rs`:节点炸毁恢复、删除 Cell 后恢复、checksum 断言、恢复前后 sha256 对比 |
| §4.1 | Cell Manifest / 只读保护 | ✅ | 打开即 `PRAGMA quick_check`;损坏 → 只读保护(写拒绝 + 告警,不静默修复);manifest 记录 format_version |
| §5 | Cell 生命周期状态机 | ✅ | `created → active`(create 后 ensure 落盘)+ delete 先置 `deleting` |
| §6 | 租户资源隔离 | ✅ | per-tenant/per-cell 并发配额、SQL 超时、KV key/value/TTL 上限、MSET 校验 |
| §7 | 优雅关闭 | ✅ | SIGTERM → drain → unregister → WAL checkpoint;`stop_grace_period 30s` |
| §8 | 稳定性测试/基准 | 🚧 | `crates/benchmark`(--mixed/--contention/--e2e/--capacity)+ 持续运行文档 |
| §9 | 故障注入 | ✅ | `scripts/fault/{kill-node,network-isolate,disk-full}.sh` |
| §10 | 配置管理 | ✅ | `deploy/CONFIG.md` 完整 env 单一来源清单 |
| §11 | Cell 迁移 | ✅ | `POST /admin/cells/{id}/migrate`(fence → 备份 → 恢复 → 切路由)|
| §12 | 版本兼容 | ✅ | `CELL_FORMAT_VERSION=1`,打开时校验,超版本拒绝 |
| §13 | 多副本 / 调度 / SLO | ⬜ | 远期(§15:不 rush,先保证 backup+restore+migration 可靠)|

> 对应提交:`019582c6`(Week 1)、`61961b30`(Week 2)、`d54f0df0`(版本兼容/恢复测试/配置)。

---

# 1. Reliability Philosophy

For infrastructure products, the core value is not only functionality.

Users care about:

> "If I put my data into Combee, will it still be there tomorrow?"

Therefore stability has higher priority than new features.

The reliability focus:

1. Failure isolation
2. Data safety
3. Recovery capability
4. Upgrade safety

---

# 2. Current Architecture

```
Caddy
 |
API Server
 |
NodeRegistry + Metadata PostgreSQL
 |
DataNode
 |
Cell Storage
 |
Backup/Object Storage
```

Current foundation:

- Node registration
- Heartbeat
- Placement
- Tenant isolation
- API Key authentication
- Credits design
- Docker Swarm deployment
- Release Gate testing

Next phase focuses on making failures predictable.

---

# 3. P0 Before Public Alpha

## 3.1 Backup and Restore Verification

A backup that has never been restored is not a real backup.

Required:

- Automated restore test
- Data checksum verification
- Disaster simulation

Recommended workflow:

```
Create test Cell
      |
Write random data
      |
Backup
      |
Delete Cell
      |
Restore
      |
Checksum comparison
      |
Destroy test Cell
```

Goal:

Verify:

```
Write
 ↓
Backup
 ↓
Failure
 ↓
Restore
 ↓
Data identical
```

---

# 4. Data Integrity Protection

## 4.1 Cell Manifest

Add metadata describing Cell state.

Example:

```json
{
  "cell_id": "cell_xxx",
  "generation": 42,
  "size": 12345678,
  "checksum": "sha256..."
}
```

Startup process:

```
DataNode starts

 ↓

Read manifest

 ↓

Verify checksum

 ↓

Load Cell
```

If verification fails:

- Enter read-only protection mode
- Trigger alert
- Do not silently repair

The biggest risk for databases is silent corruption.

---

# 5. Cell Lifecycle State Machine

Avoid simple:

```
exists / not exists
```

Recommended:

```
CREATING
    |
READY
    |
ACTIVE
    |
IDLE
    |
SLEEPING
    |
MIGRATING
    |
RESTORING
    |
FAILED
    |
DELETED
```

Reason:

Future operations are asynchronous:

- Migration
- Backup restore
- Failover
- Replication

Example:

```
create cell

202 Accepted

{
  status:"creating",
  cell_id:"xxx"
}

↓

READY
```

---

# 6. Tenant Resource Isolation

A single user must not be able to damage other users.

Example failure:

```
User application bug

while(true){
  database requests
}

↓

CPU exhaustion

↓

Other tenants affected
```

Required protections:

## Request limits

```
Maximum concurrent requests per Cell
```

Example:

```
100 concurrent requests / Cell
```

Return:

```
429 CELL_BUSY
```

---

## SQL Limits

Required:

```
statement timeout
```

Example:

```
COMBEE_SQL_TIMEOUT=5s
```

Long queries should be cancelled.

---

## KV Limits

Define:

```
Maximum key size
Maximum value size
TTL range
```

Example:

```
value <= 1MB
key <= 512 bytes
TTL <= 30 days
```

---

# 7. Graceful Shutdown

Do not terminate DataNode immediately.

Required SIGTERM workflow:

```
Receive SIGTERM

↓

Stop accepting new requests

↓

Wait existing requests

↓

Flush WAL

↓

Write checkpoint

↓

Exit
```

Configure:

```
terminationGracePeriod >= 30s
```

---

# 8. Stability Testing

## 8.1 Continuous Benchmark

Maintain fixed workloads.

## Small Cell Test

Example:

```
1000 Cells
10MB each
100 req/s
24h
```

Observe:

- Memory
- File count
- FD usage
- Latency


## Large Cell Test

Example:

```
1 Cell
50GB
```

Test:

- Backup
- Restore
- Migration

---

# 9. Fault Injection

Create:

```
scripts/fault/
```

Examples:

## Process failure

```
kill -9 DataNode
```

Verify:

- Restart
- Recovery
- Data consistency


## Network failure

Simulate:

```
DataNode unreachable
```

Verify:

- Heartbeat timeout
- Route update


## Disk failure

Simulate:

```
Disk full
```

Verify:

- Write rejection
- Alert
- Recovery procedure

---

# 10. Configuration Management

Avoid scattered environment configuration.

Recommended:

```
config/

production.yaml
staging.yaml
development.yaml
```

Example:

```yaml
cell:
  max_size: 10GB

sql:
  timeout: 5s

backup:
  interval: 1h
```

Future systems requiring dynamic configuration:

- Credits
- Pricing
- Quota
- Resource limits

should follow the same model.

---

# 11. Cell Migration System

Migration should exist before sharding.

Example:

```
Cell A

 |

Migration

 |

Node B
```

Minimal command:

```
combee migrate cell_xxx --from node1 --to node2
```

Process:

```
Freeze writes

↓

Copy snapshot

↓

Sync WAL

↓

Switch route

↓

Unfreeze
```

This becomes the foundation for:

- Rebalancing
- Sharding
- Capacity scheduling

---

# 12. Version Compatibility

Infrastructure systems must support evolution.

Required versions:

```
Metadata schema version

Cell format version

API version
```

Example:

Cell manifest:

```yaml
format_version: 1
```

Upgrade:

```
Old format

↓

Migration

↓

New format
```

Never rely on:

"we will never change the schema"

---

# 13. Later Improvements

## Multi Replica

Future:

```
Primary
 |
Replica
 |
Replica
```

Do not rush this.

A reliable:

```
backup + restore + migration
```

is better than incomplete replication.

---

## Capacity-aware Scheduler

Current:

```
round-robin
```

Future:

consider:

- CPU
- Memory
- Disk
- IO
- Active Cells

---

## SLO Definition

When commercial users arrive:

Example:

```
Availability:
99.9%

Backup RPO:
<1 hour

Restore RTO:
<30 minutes
```

---

# 14. Recommended Implementation Order

## Stability Sprint Week 1(✅ 2026-08 已完成)

```
[x] Automated backup restore test
[x] Cell lifecycle state machine
[x] Graceful shutdown
[x] SQL timeout
[x] Resource limits
```

---

## Stability Sprint Week 2(✅ 2026-08 已完成)

```
[x] Cell checksum/manifest
[x] Migration tool
[x] Fault injection framework
[x] Benchmark suite
```

---

## Stability Sprint Week 3

```
[ ] SDK
[ ] Console
[ ] Cloud Alpha release
```

---

# 15. Non-Goals Before Alpha

Do not block release on:

- Kubernetes
- Raft
- Multi-region deployment
- Complex replication
- Service mesh
- Large-scale distributed tracing

The Alpha goal is:

> Predictable failure behavior.

---

# Final Principle

Combee does not need to become Aurora on day one.

The first reliability milestone is:

A developer can trust:

```
Create Cell
 ↓
Store data
 ↓
System failure happens
 ↓
Combee recovers correctly
 ↓
Data remains available
```

Predictable recovery is the foundation of trust.
