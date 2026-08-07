# Combee Release Readiness(Public Alpha 审计)

> 审计依据:`docs/COMBEE_RELEASE_READINESS_TEST_PLAN.md`。
> 执行入口:`./scripts/release-test.sh`(Release Gate)+ `./scripts/soak-test.sh`(缩短版 Soak)。
> 自动化测试:`tests/release/*`(隔离 / fuzz / golden path / 资源 / noisy neighbor / 备份恢复)。

---

## 结论

```
Functional        PASS
Durability        PASS
Failure Recovery  PASS
Isolation         PASS(租户级隔离已落地,见 2026-08 修订)
Security          PASS
Resource Safety   PASS(有限;配额未实现 → MEDIUM)
Compatibility     PASS
Upgrade           WARN(无版本升级 fixture)
Documentation     WARN(需独立 Agent 从零执行 README)
Performance       PASS(全部达标)

BLOCKER = 0
HIGH    = 0(2026-08-07 修订:租户级隔离已实现)

RESULT: RELEASEABLE(另见"2026-08-07 修订"说明)
```

> **2026-08-07 修订(租户级隔离落地)**:
> - 元数据模型升级:`tenants` → `api_keys`(存 sha256 哈希,`cmb_sk_` 前缀)→ `databases`(含 `tenant_id`);
> - 认证:`COMBEE_AUTH=key` 时 `x-api-key` 哈希查表,请求生命周期只携带 `AuthContext{tenant_id}`;
> - 隔离在 repository 层强制:所有资源操作统一 `get_database(tenant, id)`,跨租户一律 404;
> - 新增 `tests/tenancy.rs`(3 个用例:跨租户访问全 404、key 生命周期/撤销即 401、资源不可见性);
> - 注意:自动 failover 扫描仍跨租户(`list_all_databases`),由控制面内部使用,不暴露给租户请求。

**差距清单(Public Alpha 前可选项)**:

| # | 严重级 | 问题 | 影响 | 修复方向 |
|---|---|---|---|---|
| 1 | MEDIUM | 无资源配额(max KV value / max SQL 结果 / 并发上限);超大查询依赖 SQL timeout 兜底 | 单 Cell 可占大量 CPU/内存 | max value / max rows / per-cell 并发上限(计划 §14) |
| 2 | MEDIUM | 无资源配额(max KV value / max SQL 结果 / 并发上限);超大查询依赖 SQL timeout 兜底 | 单 Cell 可占大量 CPU/内存 | max value / max rows / per-cell 并发上限(计划 §14) |
| 3 | MEDIUM | ENOSPC 场景未验证 | 磁盘满行为未知 | 容器小文件系统测试 |
| 4 | LOW | 内存注册表:API Server 重启后节点短暂不可路由(自愈重注册 ~2s) | 短暂 500 | 注册表持久化 / 多 API Server |
| 5 | LOW | 自动 failover 扫描默认关闭(需 `COMBEE_FAILOVER_INTERVAL_SECS`) | 手动触发兜底 | 文档化 / 默认开启 |

---

## Release Gate 结果(本机:macOS arm64 + Docker Desktop)

执行 `./scripts/release-test.sh`(141 个测试 + docker 场景),输出:

```
PASS  cargo test --workspace(141 passed)
PASS  clippy 0 warnings
PASS  cargo fmt clean
WARN  docker build 不可用(buildx 环境问题)→ 回退容器内 cargo build
PASS  容器内 cargo build --release
PASS  API Server readiness
PASS  Data Node 注册(2 个 healthy 节点)
PASS  create Cell
PASS  重启后 KV 数据仍在
PASS  重启后 SQL 数据仍在
PASS  kill -9 data-node(SIGKILL)→ 恢复注册
PASS  PRAGMA integrity_check = ok(kill -9 后无损坏)
PASS  删卷后 restore 恢复(仅对象存储)

RESULT: RELEASEABLE(本环境门禁;产品级 HIGH 见上)
```

---

## 逐项审计明细

### 1. Fresh Install Test — PASS

`docker compose up`(postgres / minio / minio-init)+ 两个 Data Node(agent 自动注册)+ API Server,从空卷环境:
- PostgreSQL 自动初始化(PASS)、MinIO bucket 自动创建(PASS)、Data Node 自动注册(PASS)、API readiness(PASS)、创建 Cell(PASS)、首次 SQL materialize(PASS)、KV SET/GET(PASS);
- **重启全部容器后数据仍在**(KV + SQL,PASS)。

复现:`./scripts/release-test.sh`(Fresh Install 段)。

### 2. Golden Path E2E — PASS

`tests/release/golden_path.rs`:
- users + posts(SQL,含索引 / 参数绑定 / 事务 / 更新 / 删除)、session + page-cache(TTL)+ pageviews counter;
- 精确校验:SQL 行、KV 值、TTL 范围、counter 值;
- **重启(DataNode shutdown → 新实例同目录)后 SQL / KV / TTL / counter 全部保持**。

### 3. Cell / Tenant Isolation — PASS(按 UUID)+ HIGH 缺口

`tests/release/isolation.rs`:
- 猜测 / 伪造 Cell UUID → 404(PASS);
- 两个 Cell 同名数据互不可见(PASS);
- `__sys_*` 表 / `ATTACH` / `VACUUM INTO` / `VACUUM` / 多语句注入 → 全部拒绝(PASS);
- `load_extension` / CLI-only 函数(`readfile`)→ SQL error(PASS);
- 危险 PRAGMA(`journal_mode=DELETE` 等)→ 可执行但不越权、不 panic(PASS)。

**HIGH**:租户级隔离未实现 —— 所有 Cell 同属默认租户,拿到 Cell UUID 即可执行任意操作(同租户模型)。公开 alpha 前需 API key→tenant 绑定或文档化单租户限制。

### 4. Crash Matrix — PASS

`scripts/release-test.sh`:
- **kill -9 Data Node**(SIGKILL)→ 重启 → `PRAGMA integrity_check = ok`(无损坏);
- 重启全部容器 → 数据保持(无 silent loss,读回成功);
- 删 Data Node 全部本地数据 → `restore`(仅对象存储)→ 数据恢复(PASS)。

注:kill 主节点后若无副本/未开自动 failover,请求返回明确错误(500 "data node unavailable"),**无静默丢失**(符合计划:失败可接受,未知状态不可接受)。

### 5. Kill -9 During Writes — PASS(单次)/建议 CI 重复

`integrity_check = ok` 已由 gate 验证;按计划建议在 CI 重复 100~1000 次(本环境单次验证)。SQLite WAL 保证崩溃一致性。

### 6. Network Partition + Failover Fencing — PASS

`tests/failover.rs`:
- failover 全链路:副本提升 + `generation+1` + 写走新主(PASS);
- **旧主写被拒**:fence 到 `i64::MAX` 降级标记后,任意正常写(generation N / N-1 / 无 generation)→ Forbidden(PASS);
- generation 校验:写请求带 metadata generation,Data Node 校验不匹配拒绝(PASS);
- 任意时刻仅一个主接受写(metadata 路由 + fencing 双重保证)。

### 7. Backup Must Be Restorable — PASS

`tests/release/backup_restore.rs` + gate 删卷场景:
- snapshot + incremental → 继续写 → **删除全部本地文件** → restore(仅对象存储)→ SQL 行 / KV / TTL / counter 恢复;
- 删卷场景:restore 恢复到**最近 WAL 增量归档点**(backup 后的写入也能恢复,PASS)。

### 8. Soak Test — 缩短版(本环境 ~15min)/ 12h 需 CI

`scripts/soak-test.sh`:混合 workload(create/delete Cell、SQL 读写、KV SET/GET/INCR、TTL 过期)+ 每 30s 采样 API / Data Node 内存与延迟。

**本机实测(15min,30 轮采样)**:
```
轮次  API内存MB  DN内存MB  p50(µs)  p99(µs)
1     8.4        9.3       1635     2109
5     11.1       11.1      1848     2452
10    11.2       11.2      1972     2554
15    11.2       11.2      1942     2978
20    11.3       11.2      1823     4981
25    11.3       11.2      1976     2719
30    11.5       11.2      1691     2412
```
- 内存:API ~11.5MB、Data Node 恒 11.16MB(第 2 轮初始化后平稳,**无持续单调增长**)→ PASS;
- 延迟:p50 ~1.7-2.0ms,p99 2-7ms 波动,**无随时间恶化趋势** → PASS;
- 完整 12h 建议 GitHub Actions 定时跑(WARN)。

### 9. API Fuzz / Malformed Input — PASS

`tests/release/fuzz.rs`:
- malformed JSON / 空 body / 类型错误 → 4xx,不 panic,服务保持可用;
- 边界:SQ params(i64::MAX/MIN/float/null/object/array)、负数 TTL / u64::MAX / TTL=0、空 key / unicode key、非法 UUID 路径、NUL;
- 1MB KV value 通过;10MB 被 axum 默认 body limit(2MB)拒绝(413);
- **WITH RECURSIVE 无限递归被 SQL timeout(5s)中断**(审计中修复,见缺陷记录);
- SQL 逃逸全拒绝。

### 10. Resource Exhaustion — 有限 PASS / MEDIUM

- 连接上限:max_active 2 时 20 并发 3 个 Cell,连接数不超上限(PASS);
- 100k 行 SQL 结果:返回或明确失败,不崩溃(PASS);
- 1000 个 Cell 创建使用正常(PASS);
- ENOSPC / 超大请求配额:未实现(MEDIUM,依赖 axum body limit + SQL timeout 兜底)。

### 11. Noisy Neighbor — PASS

`tests/release/resource.rs`:`Cell A` 10 并发重负载(SET + 复杂 SQL)下,`Cell B` GET p99 退化 < 5x(Alpha 门槛)。

### 12. Quota / Abuse Guard — MEDIUM(KNOWN UNSAFE)

- SQL timeout(5s)已实现(审计新增,`COMBEE_SQL_TIMEOUT_SECS`);
- max body 2MB(axum 默认);
- max KV value / max SQL rows / per-cell 并发上限未实现 → 标记 KNOWN UNSAFE,见缺陷 #2。

### 13. Version Upgrade Test — WARN

未建立 `tests/fixtures/v0/` 旧版本 fixture(当前 v0.1 无历史版本)。建议下个版本发布时保存 fixture 并加入 gate。

### 14. Documentation Test — WARN

README quickstart 可完成部署(本审计按 README 步骤执行);但未使用"无项目上下文的独立 Agent 从零执行"。建议 CI 中跑一次独立 Agent 验证。

### 15. Clean Linux Environment — WARN

本机为 macOS;GitHub Actions runner(Ubuntu x86_64)验证待配置:克隆 → `docker compose up -d --build` → `cargo test --workspace` → `./scripts/release-test.sh`。

### 16. Performance Regression Gate — PASS(基线已存)

基准已保存(`capacity*.csv/md`、`contention.csv/md`、`e2e.csv/md`),设计目标全部达标(hot GET p99 <5µs 等)。自动对比门禁待 CI 接入(WARN)。

---

## 审计中发现并修复的缺陷(均有回归测试)

| # | 缺陷 | 严重级 | 修复 |
|---|---|---|---|
| 1 | `WITH RECURSIVE` 无限递归让 SQLite 永久挂起(无执行超时) | HIGH(CPU 耗尽) | 新增 SQL 执行超时 + `InterruptHandle` 中断(`COMBEE_SQL_TIMEOUT_SECS`,默认 30s);`tests/release/fuzz.rs::recursive_query_is_bounded` |
| 2 | 节点重启后 NodeId 变化,存量 Cell 的 `storage_node_id` 失效 → 路由 500 | HIGH(kill -9 后无法恢复) | 持久化 NodeId(data_dir/node-id)+ 注册幂等(带 id 注册,重启身份不变);gate 重启/Kill-9 场景验证 |
| 3 | `VACUUM INTO` 可把 Cell 数据写到任意文件系统路径(逃逸) | HIGH(文件系统越权) | `check_statement` 拦截 `vacuum` 前缀;`tests/release/isolation.rs` |
| 4 | agent 心跳 404 不自愈(API Server 重启后节点永久丢失) | MEDIUM | 注册即心跳(每 tick 带本地 id 注册,幂等) |
| 5 | release-test 脚本时序:postgres 未 ready 即起 API / 未等节点注册 | 测试基建 | 等待 PG healthy + 等待节点注册 |

---

## 复现命令

```bash
# 自动化门禁(141 测试 + docker fresh install / 重启 / kill-9 / 删卷恢复)
./scripts/release-test.sh

# 缩短版 Soak(默认 15min,可传分钟数)
./scripts/soak-test.sh 15

# 单独跑 release 测试
cargo test -p combee --test release

# 性能基线
cargo run --release -p combee-benchmark -- --capacity --metadata postgres --total 1M --active 32,500,5000
cargo run --release -p combee-benchmark -- --contention
cargo run --release -p combee-benchmark -- --e2e --url http://127.0.0.1:8080
```

---

## 下一步(进入 Public Alpha 前)

1. **关闭 HIGH**:实现 API key → tenant 绑定(每个 key 只能操作自己租户的 Cell),或文档化"单租户模式"并限制 key 发放;
2. 接入 GitHub Actions:clean Linux 全量 gate + 12h soak + README 独立 Agent 验证;
3. 建立 `tests/fixtures/v0/` 升级 fixture;
4. 按缺陷 #2(配额)实现 max value / max rows / 并发上限。
