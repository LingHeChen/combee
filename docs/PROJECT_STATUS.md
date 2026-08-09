# Combee 项目状态总览

> 生成时间:2026-08-08 · 最新提交:`6544b92`(24 个提交)
> 范围:Combee 核心(Rust Serverless Data Runtime)+ 官方 SDK(TS/Python)+ Combee Cloud Console(Next.js BFF)

## 1. 一句话

Combee = **Serverless Data Runtime**(一个应用一个 Cell,SQL + KV,无数据库实例)
+ **Combee Cloud Console**(Next.js BFF:用户用用户名密码登录,整个前端的数据存储全部走 Combee 自身)。

## 2. 仓库与版本

| 仓库 | 内容 | 状态 |
|---|---|---|
| `LingHeChen/combee`(本仓库) | 核心 Rust runtime + Console(web/)+ 文档/设计稿 | v0.1.0-alpha,tag `v0.1.0-alpha.1` |
| `LingHeChen/combee-js` | TypeScript SDK(`@combee/sdk`) | 独立仓库,代码就绪(未发布 npm) |
| `LingHeChen/combee-python` | Python SDK(`combee`,同步+异步) | 独立仓库,代码就绪(未发布 PyPI) |

## 3. 核心能力(Combee Rust runtime)

### 已完成(v0.1.0-alpha,发布就绪审计:BLOCKER=0 / HIGH=0 / RELEASEABLE)

- **Cell 生命周期**:懒创建(目录记录零 IO,首次访问才落 SQLite)、列表/删除;
- **SQL**:单条执行 + 多语句原子事务;参数绑定;SQL 超时中断;`__sys_*` 表 / `ATTACH` / `VACUUM INTO` / 多语句注入全拦截;
- **KV**:Redis-style 全子集(GET/SET/DEL/EXISTS/MGET/MSET/TTL/EXPIRE/INCR),TTL 惰性过期 + 后台 GC;共享内存缓存(read-through + write-invalidate,SQLite 权威);持久化强度 fast/normal/strict;
- **Active DB Manager**:最多 N 个 SQLite 连接(LRU 逐出 + 空闲休眠),所有阻塞操作在 `spawn_blocking` —— 1M 逻辑 Cell 不等于 1M 连接;
- **多租户**:API key(cmb_sk_,只存 sha256)绑定 tenant,隔离在 repository 层强制(跨租户 404);`COMBEE_AUTH=off|key`;
- **资源配额**:KV key/value 大小、SQL 结果截断(truncated)、cells/tenant、per-tenant/per-Cell 并发(429)、storage soft/hard、request body —— 全部 env 可配;
- **Control plane**:`COMBEE_CONTROL_PLANE_TOKEN` 保护 `/internal/*` 与 `/rpc/*`,租户 key 永不进入内部接口;`COMBEE_ADMIN_TOKEN` 管理面(grant/voucher/pricing);
- **Usage Metering(P0)**:kv/sql read-write、requests、bytes in/out、storage bytes;内存聚合 + 周期 flush(不阻塞热路径);`/v1/usage/summary|timeseries|cells/:id/usage`;
- **Credits + Pricing + Voucher(P1)**:整数 microcredits 账本(append-only、余额可重建)、pricing 版本热更新(5s,无效配置拒绝)、voucher 单次/幂等/并发安全兑换、settlement 幂等结算(记录 pricing_version);
- **Public API Freeze(P2)**:`GET /openapi.json`(utoipa)+ request-id + 稳定错误码 `{code,error}` + Idempotency-Key + 游标分页;`docs/API.md` 冻结契约;
- **Cell Identity & Lifecycle(β)**:Cell 增加租户内唯一 `name`(不可变 `id` + 可变 `name`);
  `PUT /v1/databases/by-name/{name}` 幂等 ensure、`GET by-name` 查询、`PATCH {id}` 重命名(保 id)、
  `POST {id}/reset` 重置(保 id + generation+1 清数据);重名 409 / 无效名 400;
  SDK(TS/Python)新增 `ensure/getByName/rename/reset`;Console 以 name 为主要标签;
- **结构化日志(Logging P0)**:tracing JSON 输出(service/request_id/tenant/cell/operation/status/latency_ms/error_code);
  request_id 贯穿 BFF→API→DataNode RPC(header+span);事件化 cell.open/usage.flush/settlement/backup/replica/failover/node;
  敏感数据硬禁止(password/api_key/session/voucher/SQL 参数/KV value);规范见 `docs/LOGGING.md`;
- **备份/恢复**:snapshot(VACUUM INTO)+ WAL 增量 → S3/MinIO;restore 优先增量回退全量;
- **单 replica + 自动 failover**:复制复用 WAL 归档,generation fencing 防脑裂;
- **多节点**:agent 注册/心跳、round-robin placement、按 Cell 路由;NodeId 持久化。

### 性能(设计文档 §22 目标全部达标,Apple Silicon 本机)

KV hot GET p50/p99 ≈ 10µs/35µs;fast SET p99 ≈ 63µs;strict SET p99 ≈ 125µs;
Simple SQL p99 ≈ 41µs;20k 逻辑 Cell 创建 ≈ 15ms;4+8 容器 1M×5k active p99 ≈ 64µs、命中率 100%。

## 4. Combee Cloud Console(web/,Next.js 16 + Tailwind v4 + shadcn)

### 架构:BFF(Next.js 后端)

```text
Browser ── httpOnly cookie(combee_session) ──▶ Next.js BFF
   ├── Auth     用户名+密码(scrypt 加盐哈希,console_users 表存 Combee)
   ├── Session  会话存 Combee KV(bff:session:{sid},TTL 24h)
   ├── Proxy    /api/bff/v1/* → Combee /v1/*(用户专属 key,401 拦截)
   ├── Aggregation /api/bff/overview(cells+usage+credits+storage)
   └── 页面     server components 经服务层 / client 经 /api/bff/*
   ▼
Combee API Server(所有数据存储)
   Cell(SQLite):前端业务数据 + 会话 + console_users/profiles/snippets/recent/history
   PostgreSQL:租户/API keys/usage/credits/pricing
   对象存储:备份/复制归档
```

- **登录 = 用户名+密码**(修掉"用 API key 登录 Console"的死锁);注册时 BFF 服务账号自动为用户签发专属 Combee key;
- **Closed Alpha 邀请制**:signup 默认需 Alpha access code(=voucher),注册即兑换并获 1000 Credits(复用 voucher 系统);
  `COMBEE_CONSOLE_SIGNUP=code|open|off`;邀请码生成:`scripts/generate-invites.sh`;
- **用户数据全存 Combee**:Profile(display_name/avatar/locale/timezone)、Console Preferences(默认时间范围/region/page size/UI)、Onboarding(从 Combee 实际数据推断)、saved SQL snippets、最近访问 Cells、Query history(仅截断 SQL,**不含参数**);
- **设计稿还原**:15 页原型(overview/cells/cell-detail 7-tabs+7-bento/SQL workspace/KV browser/usage 图表/credits/api-keys modal/account 5-tabs/welcome 等)按 `design/stitch_combee_landing_page_design` 全量对齐(DESIGN_GAP 已关闭);
- **代码高亮**:SQL 编辑器(overlay 实时 prism 高亮)、JSON(KV 编辑器/Diagnostics)、TS/Python/HTTP 代码卡;
- 品牌:design/combee.png 作 favicon + 站内 logo。

## 5. 测试状态

| 层 | 数量 | 说明 |
|---|---|---|
| Rust 单元+集成 | **174** | `cargo test --workspace` 全绿 |
| Rust 质量门禁 | 0 warnings / fmt clean | `cargo clippy --workspace --all-targets` |
| Release Gate | **RELEASEABLE** | 14 PASS / 0 FAIL / 1 WARN(buildx 环境,回退容器构建);真实 Postgres/MinIO 场景(重启持久性、kill -9+integrity、删卷仅对象存储恢复);发现并修复 2 个真实缺陷(见 git log `2055b44`) |
| Web 单元(vitest) | **10** | utils / mock 完整性 / 表单 / KV Browser |
| Web E2E(Playwright chromium) | **22** | BFF 模式:注册→登录→聚合/代理 CRUD→Account(profile/onboarding/snippets/activity)→登出→re-login,零 console 错误 |
| SDK contract(TS) | 4 unit + 7 contract | 跑真实 Combee server |
| SDK contract(Python) | 7 | 同步/异步等价,跑真实 server |

## 6. 运行方式

```bash
# 核心(单进程 dev)
cargo run -p combee-api-server

# Console(BFF)
cd web && COMBEE_API_URL=http://127.0.0.1:8080 npm run build
COMBEE_API_URL=http://127.0.0.1:8080 npx next start -p 3100
# 打开 :3100 → 注册/登录(用户名+密码)

# SDK(独立仓库)
# combee-js: npm install && npm run test:contract
# combee-python: pip install httpx pytest && pytest tests/
```

关键 env:`COMBEE_API_URL`(BFF 必填)、`COMBEE_BFF_API_KEY`、`COMBEE_BFF_CELL`、
`COMBEE_CONSOLE_SIGNUP`、`COMBEE_AUTH`、`COMBEE_CONTROL_PLANE_TOKEN`、`COMBEE_ADMIN_TOKEN`。

## 7. 路线图进度

```text
✅ P0 Usage Metering
✅ P1 Pricing + Credits Ledger + Voucher
✅ P2 Public API Freeze + OpenAPI
✅ P3 TypeScript + Python SDK(独立仓库,发布动作待做)
✅ P4 Web Console(Next.js BFF + 设计稿还原)← 当前
⬜ P5 Cloud Alpha 部署(三件套 + TLS + Linux 验证)
⬜ SDK 发布:npm `@combee/sdk` / PyPI `combee`
⬜ CI 接入(gate 本地脚本就绪:web/e2e-qa.mjs、scripts/release-test.sh)
⬜ Beta 发布条件:SDK_SPEC §27 剩余 5 项、v0.1.0-beta 发布清单(见 docs/plan/)
```

## 8. 已知限制 / 注意

- Rust 核心:单 Cell 写串行(读并行 ~800 万 ops/s);默认 metadata in-memory(生产用 postgres);failover 依赖对象存储;
- **资源配额(安全护栏,已实现)**:max request body / KV key+value / SQL rows+result bytes(截断+truncated)/
  cells per tenant / per-tenant+per-Cell 并发(429)/ storage soft+hard —— 见 README `COMBEE_MAX_*` env;
  公网 signup 前建议显式设置 storage 与并发上限;
- Console:BFF 单实例(会话存 Combee KV,多实例可共享);signup 默认 `code`(需 Alpha access code 邀请码,`COMBEE_CONSOLE_SIGNUP=code|open|off` 可调,`open` 为无门槛开放);
- 本机环境限制:Chrome headless / docker buildx 在沙箱下受限,测试用 Playwright 自包含 chromium 与容器内 cargo build 绕过;
- 用户 workspace 有未提交的文档整理(docs → artifacts/engineering/),与本仓库 git 历史无关。

## 9. 关键文档索引

- `docs/API.md` — Public API 冻结契约;
- `docs/plan/COMBEE_NEXT_PHASE_V0.1.0_BETA_PLAN.md` / `..._SDK_SPEC.md` — beta 计划与 SDK 规范;
- `docs/RELEASE_READINESS.md` — 发布就绪审计;
- `web/DESIGN_GAP.md` — 设计稿对比与补全记录;
- `web/README.md` — Console(BFF)架构与运行;
- `docs/COMBEE_DESIGN.md` / `docs/PROJECT_SUMMARY.md` — 设计文档与项目总结。
