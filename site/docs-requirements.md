# Combee Docs Requirements · Combee 文档站构建指令

> This file is the single requirements source for the user-facing docs site at **docs.combee.dev**
> (a Fumadocs app). It is maintained bilingually (EN + 中文); keep both languages in sync when editing.
>
> 本文件是 docs.combee.dev 用户文档站的唯一需求来源(基于 Fumadocs)。本文件与 `FACTS.md` 配合使用:
> **改文档前先读 `FACTS.md` 核对事实,再按本文件的要求写作。**

---

## 1. Goal · 目标

Build a Fumadocs-based documentation site for **Combee users** — not for internal Combee developers.
The site separates **Cloud (managed) user docs** from **Self-hosted docs**, is bilingual (EN + 中文),
and every statement in it is verifiable against the codebase / SDKs.

用 Fumadocs 构建 **Combee 用户**的文档站(不面向 Combee 内部开发者)。**Cloud 托管用户文档**与
**Self-hosted 文档**分开;站点中英双文;文档里的每一个陈述都能在代码库 / SDK 中验证。

## 2. Sitemap · 站点结构

The page tree below is frozen; the sidebar navigation mirrors it. 以下页面树冻结,侧边栏导航与之对应。

```text
Getting Started · 快速开始
├── Introduction · 简介
├── Quickstart · 快速上手
├── Authentication · 认证
└── Create your first Cell · 创建你的第一个 Cell

Core Concepts · 核心概念
├── Cells · Cell
├── SQL + KV
├── Lifecycle · 生命周期
└── Limits · 限制

SQL
├── Query · 查询
├── Execute · 执行
├── Transactions · 事务
├── Parameters · 参数
└── Errors / Timeouts · 错误与超时

KV
├── GET / SET
├── TTL
├── NX / XX
├── MGET / MSET
└── Counters · 计数器

SDKs
├── TypeScript
└── Python

Platform · 平台            ← Cloud 托管文档
├── API Keys
├── Usage · 用量
├── Credits · 额度
└── Vouchers · 兑换券

Reliability · 可靠性       ← Cloud 托管文档
├── Backups · 备份
├── Restore · 恢复
└── Replication / Failover · 复制 / 故障转移

Reference · 参考
├── REST API
├── TypeScript API
├── Python API
└── Error Codes · 错误码

Self-hosting · 自托管      ← 独立章节,与 Cloud 文档分开(见 §4)
├── Docker Compose
├── Configuration · 配置
├── Object Storage · 对象存储
└── Operations · 运维
```

Grouping notes:
- `Platform` + `Reliability` 属于 Cloud 托管用户文档;`Self-hosting` 整组独立(见 §4)。
- `Reference` 全部从 OpenAPI / SDK 生成(见 §3 P3)。

## 3. Core Principles · 核心原则

**P1 — Docs face users, not internal developers.**
Users should understand Cell / SQL / KV / API Key / Usage / Credits — they should never be forced to
understand Data Node / NodeRegistry / generation fencing / WAL archival / internal RPC.
文档面向用户,不面向 Combee 开发者内部。用户应该理解 Cell / SQL / KV / API Key / Usage / Credits,
不应该被迫理解 Data Node / NodeRegistry / generation fencing / WAL archival / internal RPC。

**P2 — SDK_SPEC is an implementation contract, not user docs.**
User docs reorganize it into tutorials and reference; never copy MUST/SHOULD clauses verbatim.
SDK_SPEC 是实现契约,不是用户文档。用户文档要重新组织成教程和 reference,而不是把 MUST/SHOULD 原样搬过去。

**P3 — OpenAPI is the interface source of truth; hand-written MDX is the experience source.**
Parameters, responses, and error codes are generated from OpenAPI where possible; Quickstart, concept
explanations, and best practices are written by hand.
OpenAPI 是接口事实来源,手写 MDX 是体验来源。参数、响应、错误码尽量自动生成;Quickstart、概念解释、最佳实践自己写。

**P4 — Every example must run.**
Never ship an example that "looks good" but isn't implemented in the SDK yet.
所有示例必须能跑。不要出现"看起来很好,但 SDK 其实还没实现"的示例。

**P5 — Default to TypeScript and Python.**
Use language switchers for major examples instead of maintaining two duplicate tutorials.
文档默认围绕 TS 和 Python。最好每个主要例子都有语言切换,而不是维护两份重复教程。

## 4. Cloud vs Self-hosted · Cloud 与 Self-hosted 分离

Cloud user docs (API Keys, Usage, Credits, Vouchers, Backups, Restore, Replication/Failover) and
Self-hosted docs (Docker Compose, Configuration, Object Storage, Operations) are **separate sections**.
Material that necessarily touches internal concepts (Data Node, PostgreSQL, MinIO, `COMBEE_*` env vars)
belongs in the Self-hosting section only — it must not leak into Cloud user docs.

Cloud 用户文档(API Keys / Usage / Credits / Vouchers / Backups / Restore / Replication / Failover)与
Self-hosted 文档(Docker Compose / Configuration / Object Storage / Operations)**分开**。涉及内部概念
(Data Node、PostgreSQL、MinIO、`COMBEE_*` 环境变量)的内容只属于 Self-hosting 章节,不得混入 Cloud 用户文档。

## 5. No Hallucinated Features · 禁止脑补功能

Never document unimplemented features as existing capabilities. Currently **not implemented**:
`clone()`, `branch()`, Redis RESP, PG wire, multi-region, online payment.
Use explicit status labels — `Planned`, `Experimental`, `Not available in v0.1.0-beta` — the status must
always be stated. Every capability statement must map to a real implementation in code / SDK.

不要"脑补功能"。现在没做的:`clone()`、`branch()`、Redis RESP、PG wire、multi-region、online payment。
不能因为"未来可能支持"就写成现有能力。可以标 `Planned` / `Experimental` / `Not available in v0.1.0-beta`,
但必须明确状态。任何能力描述都能在代码 / SDK 中找到对应实现。

## 6. Benchmark Wording · 性能表述规范

No unverifiable marketing claims (e.g. "Combee supports millions of requests per second"). Performance
numbers must always carry their measurement conditions. Correct phrasing:

禁止未经验证的性能营销语句(例如 "Combee supports millions of requests per second")。性能数字必须带测量条件。正确表达类似:

```text
In-process hot KV GET microbenchmark:
~7.9M ops/s at concurrency 512

This result excludes HTTP/network overhead.
```

Similarly, "1M logical Cells / ~25MB RSS" must always carry:

同样,"1M logical Cells / ~25MB RSS" 也要带上:

```text
32 active Cells
PostgreSQL-backed metadata
specific benchmark environment
```

Verified numbers and their conditions live in `FACTS.md` §Benchmark, backed by
`artifacts/contention.md` and `artifacts/capacity*.md`. 实测数字与条件以 `FACTS.md` 与上述 artifacts 为准。

## 7. Frontmatter / Status · 页面 frontmatter 与状态

Every page carries frontmatter; `experimental` / `planned` pages render a status badge at the top.
每个页面都带 frontmatter;`experimental` / `planned` 页面在头部显示状态徽章。

```mdx
---
title: KV TTL
description: Expire keys automatically using Combee KV TTL.
since: 0.1.0-beta
status: stable
---
```

- `status` ∈ `stable` | `experimental` | `planned`
- `since`:首次引入该能力的版本。

## 8. Unified Error Model · 统一错误模型

The whole site uses **one frozen mapping table** — never mix styles like `404` / `Cell not found` /
`DATABASE_NOT_FOUND`. REST reference, SDK pages, and tutorials are all generated from the table below.

整套文档从一个冻结的映射表生成,禁止一会写 `404`、一会写 `Cell not found`、一会又写 `DATABASE_NOT_FOUND`。
REST / SDK / 教程都引用同一张表:

| HTTP | `error.code` | SDK exception(TS / Python) |
|---|---|---|
| 400 | `invalid_request` | `InvalidRequestError` |
| 400 | `sql` | `SqlError` |
| 400 | `invalid_cell_name` | `InvalidCellNameError` |
| 401 | `unauthorized` | `AuthenticationError` |
| 403 | `forbidden` | `PermissionDeniedError` |
| 404 | `database_not_found` | `CellNotFoundError` |
| 404 | `api_key_not_found` | `ApiKeyNotFoundError` |
| 409 | `database_already_exists` | `CombeeError`(基类;暂无专用子类) |
| 409 | `cell_already_exists` | `CellAlreadyExistsError` |
| 409 | `cell_name_conflict` | `CellNameConflictError` |
| 429 | `quota_exceeded` | `QuotaExceededError` |
| 500 | `internal` | `InternalServerError` |
| 500 | `cell_reset_failed` | `CellResetFailedError` |

Client-side SDK exceptions (not tied to a server code): `SqlTimeoutError`, `DataNodeUnavailableError`.
Sources: `crates/common/src/errors.rs`, `crates/api-server/src/lib.rs`, `combee-js/src/errors.ts`,
`combee-python/combee/errors.py`. Any change must land in the server + both SDKs + this table + `FACTS.md`.

客户端侧 SDK 异常(不映射 HTTP code):`SqlTimeoutError`、`DataNodeUnavailableError`。
来源:`crates/common/src/errors.rs`、`crates/api-server/src/lib.rs`、`combee-js/src/errors.ts`、
`combee-python/combee/errors.py`。变更必须同步服务端 + 两个 SDK + 本表 + `FACTS.md`。

## 9. FACTS.md · 事实检查表

`FACTS.md` is the maintainer/agent fact-check table (not user docs). Every agent edits docs reads it
first; anything in docs contradicting `FACTS.md` is a bug. Keep it in sync with the codebase.

`FACTS.md` 是给维护者 / Agent 的文档事实检查表,不是用户文档。Agent 每次改文档先看它;文档与
`FACTS.md` 冲突即视为错误。事实以代码库为准,代码变化时同步更新本表。

## 10. Bilingual Conventions · 中英双文约定

The site is bilingual (EN + 中文). Each page exists as an en/zh pair (Fumadocs i18n) and users switch
via the language switcher — do not maintain two duplicate tutorial bodies. Terms `Cell`, `SQL`, `KV`,
`API Key`, `REST`, `TTL` stay in English even in Chinese pages.

站点中英双文。每页 en/zh 成对(Fumadocs i18n),用语言切换;不维护两份重复教程。`Cell` / `SQL` / `KV` /
`API Key` / `REST` / `TTL` 等术语在中文页中也保持英文。
