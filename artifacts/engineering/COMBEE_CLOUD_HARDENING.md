# Combee Cloud 收口清单(Hardening Backlog)

> 语境:**Combee Cloud** —— 我们自己托管、持有外部用户数据的云端版本。
> 因此判断标准不是"功能做完没"(核心数据路径已基本完成,见 `RELEASE_READINESS.md` / `COMBEE_STABILITY_ROADMAP.md`),
> 而是:**"我敢不敢在第一天,就把别人的数据交给它。"**
>
> 作为**运营方**,出事的是我们、丢数据的是用户 —— 所以"安全面静止"与"生产可观测"是最高优先级,
> 不是可选项。本清单是进入 Cloud Alpha(对外收第一个真实用户)前的工作锚点。
>
> 创建:2026-08-14。状态图例:⬜ 未开始 / 🚧 进行中 / ✅ 完成。

---

## 0. 立即动作(工作区当前脏树)

- ✅ **提交待落地的 3 个安全补丁,并在其之上重跑一次 Release Gate。**(commit `521b562f`;gate 2026-08-14 复跑 21 PASS / 0 FAIL,记录见 RELEASE_READINESS)
  当前工作区未提交改动含三个真实修复(见 §1.1 / §1.2 / §1.3)。
  `RELEASE_READINESS.md` 里 "14 PASS / RELEASEABLE" 是在这些补丁**之前**跑的,不能代表当前树。
  - 验收:`git commit` 落地 → `./scripts/release-test.sh` 全绿 → 更新 RELEASE_READINESS 复跑记录。

---

## P0 —— 对外收真实用户之前必须闭合

> 这几条不闭合,就不该让任何外部用户把真实数据放进来。

### 1. 安全面尚未"静止":每被认真戳一次还在漏洞

**现象**:审计判定 `Security PASS / BLOCKER=0` 之后,又发现三个真实漏洞(现躺在未提交树里)。
说明安全面**没有到达静止态**,而是"探一次漏一个"。对云端持有用户数据,这是不可接受的状态。

#### 1.1 SQL 沙箱:黑名单机制本身是漏的(根因,最高优先) ✅
> 2026-08-14 已收口:`Connection::authorizer`(rusqlite `hooks` feature)在 prepare 阶段按动作授权 ——
> 拒绝 ATTACH/DETACH/事务控制/SAVEPOINT/危险 PRAGMA(白名单)/危险函数(`load_extension`/`readfile`/`writefile`)/`__sys_*` 表;
> 字符串层仅保留无 authorizer code 的 VACUUM 与多语句检查;thread-local 区分用户 SQL 与内部操作。
> 对抗性用例见 `sql.rs::authorizer_rejects_at_engine_level` + `sandbox::tests`。
- **问题**:`crates/data-node/src/sql.rs::check_statement` 用**字符串前缀/包含黑名单**做沙箱
  (`lower.contains("__sys")`、`FORBIDDEN_PREFIXES` + `starts_with`)。这是在和 SQLite 解析器打地鼠。
- **证据**:本次修复是"剥离前导注释再匹配前缀"(`skip_leading_trivia`)—— 补的是**一个**绕过,
  而这一类绕过(大小写/Unicode 空白/嵌套注释/`/*!*/` 变体/换行技巧……)会源源不断。
- **真正的收口(换机制,不是再补一个 case)**:
  1. 首选:启用 **SQLite authorizer 回调**(`rusqlite::Connection::authorizer`),在**引擎层**拒绝
     `ATTACH` / 危险 `PRAGMA` / `load_extension` / 指定函数 —— 这是 SQLite 为沙箱化设计的正道;
  2. 或:接入真正的 SQL parser,走**语句类型 AST 白名单**(只放行 SELECT/INSERT/UPDATE/DELETE/CREATE… 的受控集合)。
- **验收**:沙箱不再依赖 `starts_with`;新增一批对抗性用例(注释/空白/大小写/函数/pragma 变体)全部拒绝;
  authorizer 拒绝路径有测试。**在此之前,视 SQL 沙箱为"仍可被绕过"。**

#### 1.2 备份 key 跨租户越权(路径穿越) ✅
> commit `521b562f`:API 层 + RPC 层双重前缀校验 + `tests/tenancy.rs` 用例。
- **问题**:`/restore` 的 `version` 是对象存储 key,未校验前缀时可读/恢复任意对象(含他租户备份)。
- **修复(已在未提交树)**:API 层(`handlers/backup.rs`)+ RPC 层(`data-node/src/lib.rs`)双重校验
  key 必须落在 `backups/{id}/` 前缀内。已有测试 `tests/tenancy.rs`(`backups/0000...0001/evil.sqlite` → 拒绝)。
- **验收**:提交落地;确认 API 与 RPC **两层**都拦(防 `/rpc/*` 直连绕过 API 层)。

#### 1.3 Credits 并发双花(刷钱竞态) ✅
> commit `521b562f` + `postgres.rs` 并发回归测试(16 并发同 reference_id → 余额只加一次),已进 gate Postgres 段。
- **问题**:幂等入账在冲突时仍可能重复累加余额。
- **修复(已在未提交树)**:`metadata/src/postgres.rs` 改 `ON CONFLICT DO NOTHING ... RETURNING id` +
  `fetch_optional`,冲突则回滚返回既有条目。
- **验收**:提交落地;补一个**并发**回归测试(同 `reference_id` N 个并发请求 → 余额只加一次)。

### 2. 可观测性:现在是"计划",不是"代码" —— 生产里你是瞎的 ✅
> 2026-08-14:API Server 与 Data Node 均暴露 `/metrics`(Prometheus 文本,`combee-common::metrics`);
> 指标含请求量/延迟直方图/错误率、active cells、连接数、缓存命中率、usage/settlement/failover/backup 成败;
> `deploy/alert/check.py` 新增 Cell 只读(P0)/failover(P1)/后台任务失败(P1)告警;request-id 已贯穿日志。
- **问题**:代码里**没有 `/metrics` 端点、没有告警**(grep 无 `metrics`/`alert`)。
  `COMBEE_OBSERVABILITY_ALERTING_PLAN.md`(24KB)是纸面。
- **为什么 P0**:云端运营方一旦出事(Cell 损坏 / failover / 磁盘满 / 错误率飙升),
  没有指标和告警只能事后翻日志。**"看得见"是托管别人数据的前提。**
- **最小闭环(不用一步到位)**:
  1. `/metrics`(Prometheus 文本):请求量/延迟直方图、错误率、active DB 连接数、缓存命中率、
     usage flush 成败、settlement 成败;
  2. **关键告警**(先接一个渠道即可,邮件/webhook):Cell 损坏进只读、failover 触发、磁盘水位、
     节点心跳丢失、错误率阈值、usage/settlement 持续失败;
  3. request-id 已有 → 打通到结构化日志,便于事后定位。
- **验收**:能在一块面板上看到上述指标;人为触发损坏/磁盘满/failover 时收到告警。

### 3. 磁盘满(ENOSPC)行为未验证 ✅
> gate 新增 ENOSPC 场景(容器 24MB tmpfs):写被明确拒绝(SQL/KV 400 database or disk is full,不静默丢)、读可用、integrity_check ok、进程存活、清理后写恢复。
- **问题**:审计标注未验证;对存数据的服务,磁盘满是"何时"不是"会不会"。
- **收口**:用容器小文件系统跑 `scripts/fault/disk-full.sh` 场景,验证:
  写入被明确拒绝(不 panic、不静默丢)、触发告警(见 §2)、恢复流程可用、`integrity_check` 无损坏。
- **验收**:ENOSPC 下**无静默数据损失**,且有明确错误 + 告警。

### 4. 数据保护配置在云端必须默认打开 ✅
> `deploy/docker-compose.cloud.yml` 默认 WAL 归档 15s / 副本 30s / 自动 failover 30s;`DEPLOY.md §8.1` 写清 RPO(≤30s)与 RTO(秒级 failover / 分钟级 restore)。
- **问题**:`COMBEE_WAL_BACKUP_INTERVAL_SECS` / `COMBEE_REPLICA_INTERVAL_SECS` /
  `COMBEE_FAILOVER_INTERVAL_SECS` 默认都是 0(关)。开发默认合理,**但云端关着 = 没有增量归档、没有复制、没有自动 failover**。
- **收口**:Cloud 部署(`deploy/`)显式开启 WAL 增量归档 + 复制 + 自动 failover;
  写清 **RPO**(受 WAL 归档间隔约束)与 **RTO**。
- **验收**:生产配置里三者非 0;文档给出实测 RPO/RTO 数字。

---

## P1 —— 规模化 / 正式对外之前闭合

### 5. CI / 干净环境背书缺失 🚧
> 已落地 `.github/workflows/ci.yml`(clean Ubuntu 跑 release-test.sh)+ `soak.yml`(12h 定时 soak);
> 实际执行需推送到 GitHub 仓库并观察 badge。
- **问题**:一切验证都在作者的 macOS 上。无 GitHub Actions、无 clean Linux 全量 gate、
  无 12h soak、kill-9-during-writes 只跑过 1 次(计划要 100~1000 次)、README 未由"无上下文独立 Agent"从零执行。
- **收口**:GitHub Actions:clean Ubuntu → `docker compose up -d --build` → `cargo test --workspace` →
  `release-test.sh`;定时 12h soak;kill-9×N;独立 Agent 跑 README quickstart。
- **验收**:主分支绿色 gate 徽章;soak 无内存单调增长 / 无延迟恶化。

### 6. 元数据注册表重启抖动 ✅(审计 LOW #3)
> 代码已落地:`NodeRegistry::with_pg`(PG 为 authority,本地 TTL 缓存),API Server 重启从 PG 恢复节点状态;2026-08-14 在 RELEASE_READINESS 差距清单中标记已解决。
- **问题**:内存 NodeRegistry,API Server 重启后节点短暂不可路由(~2s 自愈,期间 500)。
- **收口**:注册表持久化,或多 API Server 实例;或至少把窗口内错误做成可重试的明确错误。

### 7. SLO 定义 ✅
> 新增 `artifacts/engineering/COMBEE_SLO.md`:可用性 99.9%、Backup RPO ≤30s、Restore RTO ≤30min、Failover RTO ≤60s(诚实口径,含"明确不做"与故障语义)。
- **问题**:云端要对用户有承诺,但当前无 SLO。
- **收口**:定义并公开:可用性目标、Backup RPO、Restore RTO(参考 STABILITY_ROADMAP §13)。
  诚实即可,别过度承诺(见 §9 可靠性天花板)。

### 8. 文档 / 实现漂移对齐 🚧
> STABILITY_ROADMAP §5 已标注"异步创建为 Future / Non-Goal,当前同步 201";
> RELEASE_READINESS 差距清单 #3/#4/#2 已更新为已解决;
> 剩余:gate/soak 脚本与代码的认证漂移已在本次修正(Key 模式 + control token);README 独立 Agent 验证待 CI 环境执行。
- **问题**:
  - STABILITY_ROADMAP §5 画了异步 Cell 生命周期(`202 Accepted` / `status:"creating"`),
    实际 `handlers/database.rs` 创建是**同步**的;
  - 可观测性有文档无代码(见 §2)。
- **收口**:要么实现,要么把文档改成与实现一致 —— **别让文档比实现"讲得满"**,
  用户按文档预期落空比没文档更伤信任。

---

## P2 —— 刻意推迟(记录在案,别现在做)

> 这些是 STABILITY_ROADMAP §13/§15 明确的 Non-Goals。列在此处是为了**防止竞争焦虑下反悔去做**。

### 9. 可靠性天花板(架构级,已知取舍) 🔒
- 单 Cell 写串行 + 单副本 + failover 窗口内有丢写风险(RPO 受 WAL 归档间隔约束)。
- **取舍是对的**:"可靠的 backup+restore+migration" 优于 "不完整的多副本"。
- **要做的不是现在上多副本,而是**:对用户**讲清楚**这是"能容忍少量数据新鲜度损失"的模型;
  在 SLO(§7)里如实反映。

### 10. 多副本(>1)、capacity-aware scheduler、K8s/Raft/多区 🔒
- 全部 Non-Goal before Alpha。等有真实商用负载再说。

---

## 优先级速览

| 优先级 | 条目 | 一句话 |
|---|---|---|
| **立即** | §0 | ✅ 已提交(521b562f)+ gate 21 PASS |
| **P0** | §1.1 | ✅ authorizer 引擎层沙箱(黑名单仅剩 VACUUM/多语句) |
| **P0** | §1.2 §1.3 | ✅ 补丁 + 并发测试进 gate |
| **P0** | §2 | ✅ /metrics(api + data-node)+ 告警(check.py 3 项) |
| **P0** | §3 | ✅ gate ENOSPC 场景(400 明确拒绝 + 无损坏) |
| **P0** | §4 | ✅ Cloud 默认开启 + RPO/RTO 文档 |
| **P1** | §5 | 🚧 CI workflow 已写,待推仓库执行 |
| **P1** | §6 §7 §8 | ✅§6(已落地)/ ✅§7(SLO 文档)/ 🚧§8(roadmap 已对齐,README 验证待 CI) |
| **P2** | §9 §10 | 多副本等,刻意推迟,别反悔 |

---

## 一句话原则

对 Combee Cloud,挡在"能对外收用户"之间的只有两件事:
**(1) 安全面真正静止(尤其 SQL 沙箱从黑名单换成引擎层 authorizer);
(2) 生产里看得见它(指标 + 告警)。**
其余要么已刻意推迟并文档化,要么是可排期的 P1。先闭 P0,再谈增长。
