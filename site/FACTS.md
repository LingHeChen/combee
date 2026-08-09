# Combee Docs Fact-Check Table · 文档事实检查表

> Maintainer / Agent checklist — **not** user docs. 给维护者 / Agent 用,不是用户文档。
> Read before editing any docs page; anything in docs contradicting this table is a bug.
> 每次改文档先看本表;文档与本表冲突即视为错误。
> Facts are verified against the codebase (`crates/`, `combee-js/`, `combee-python/`); update this
> table whenever the code changes. 事实以代码库为准,代码变化时同步更新本表。
> 中英双文,保持同步。

---

## Current Release · 当前版本

- **v0.1.0-beta**(文档统一标注的版本;`since` 字段基准)
- Rust workspace 版本号:`0.1.0`(`Cargo.toml`)
- 待对齐:`artifacts/engineering/API.md` 的冻结文本仍写 `v0.1.0-alpha`,发布前需统一

## Auth · 认证

| 层 | 机制 | 来源 |
|---|---|---|
| Public 用户 | `x-api-key` header,key 前缀 `cmb_sk_…`,库中只存 sha256 | `crates/api-server/src/auth.rs` |
| Internal 控制面 | `COMBEE_CONTROL_PLANE_TOKEN` | `crates/common/src/config.rs` |
| Operator / Admin | `COMBEE_ADMIN_TOKEN` | `crates/api-server/src/auth.rs` |

三者互不通用;SDK 只暴露 `x-api-key`。用户文档只写 Public 认证,不写后两者。

## Public Resource Name · 公共资源名

- 公共术语:**Cell**(SQL + KV 的逻辑数据空间);用户文档统一用 "Cell"。
- REST 路径前缀仍为 `/v1/databases/…`(历史命名);用户文档不向用户暴露 "database" 术语。

## Supported KV · 支持的 KV

`GET` / `SET`(ttl_seconds、nx、xx)/ `DEL` / `EXISTS` / `MGET` / `MSET` / `TTL` / `EXPIRE`
(缺省 ttl 即 `PERSIST`)/ `INCR`(delta 负数即 `DECR`)

实现:`crates/api-server/src/handlers/kv.rs` + `crates/data-node/src/kv.rs`。端点:

- `GET | PUT | DELETE /v1/databases/{id}/kv/{key}`
- `POST /v1/databases/{id}/kv/{exists | mget | mset | ttl | expire | incr}`

## Unsupported · 不支持(不得写成现有能力)

RESP / PG wire / Blob / Vector / Queue / Sharding;另有未实现:`clone()`、`branch()`、
multi-region、online payment。状态只能标 `Planned` / `Experimental` / `Not available in v0.1.0-beta`。

## Error Model · 错误模型(全站唯一来源)

响应体 `{"code": <kind>, "error": <message>}`;每个响应(含错误)回显 `x-request-id`。

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

Client-side SDK exceptions(不映射 HTTP code):`SqlTimeoutError`、`DataNodeUnavailableError`。
来源:`crates/common/src/errors.rs`、`crates/api-server/src/lib.rs`、`combee-js/src/errors.ts`、
`combee-python/combee/errors.py`。

## Benchmark · 性能出处与条件

- `artifacts/contention.md`(进程内 hot-cache,不含 HTTP / 网络开销):GET(cache hit)峰值
  ≈ 3.3–3.6M ops/s(concurrency 128–512);SET ≈ 21–22k ops/s(sqlite 写)。
- `artifacts/capacity.md`(PostgreSQL-backed metadata):1M total cells / 32 active → RSS ≈ 24.9MB。
- `artifacts/capacity-inmemory.md`(InMemoryStore 对照):1M total / 32 active → RSS ≈ 492MB。
- 发布任何性能数字必须附:benchmark 条件 + 出处 + 环境(见 `docs-requirements.md` §6);数字必须能
  追溯到上述 artifacts 或具体 benchmark 运行,否则先核对再写。

## SDK Implementation Status · SDK 实现状态

- `combee-js/` + `combee-python/`:与真实服务 contract-tested,行为等价。
- 示例只准使用这两个 SDK **已实现**的方法;新方法需先在 SDK 落地再写进文档(P4)。
