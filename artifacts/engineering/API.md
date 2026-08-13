# Combee Public API — v0.1.0-alpha 冻结契约

> 对应 `plan/COMBEE_NEXT_PHASE_V0.1.0_BETA_PLAN.md` §9(Public API Freeze)。
>
> 本文档是 **Public API 的规范来源**;机器可读版本为 `GET /openapi.json`
> (由 `utoipa` 从 handler 注解生成,SDK / Console / Docs / contract tests 共用)。
> 冻结后变更需走 API 评审,并同步本文档 + `openapi.json` + SDK。

## 1. API 分层

```text
Public(用户请求,`x-api-key: cmb_sk_...`)
├── Data Plane
│   ├── SQL            /v1/databases/{id}/sql
│   │                  /v1/databases/{id}/transaction
│   └── KV             /v1/databases/{id}/kv/{key}        (GET/PUT/DELETE)
│                      /v1/databases/{id}/kv/ops/*        (exists/mget/mset/ttl/expire/incr)
└── User Control Plane
    ├── Cells          /v1/databases                      (GET/POST)
    │                  /v1/databases/{id}                 (DELETE)
    ├── API Keys       /v1/api-keys                       (GET/POST)
    │                  /v1/api-keys/{id}                  (DELETE)
    ├── Usage          /v1/usage/summary
    │                  /v1/usage/timeseries
    │                  /v1/cells/{id}/usage
    ├── Credits        /v1/credits/balance
    │                  /v1/credits/transactions
    │                  /v1/credits/redeem
    ├── Backups        /v1/databases/{id}/backup[/incr] /restore
    ├── Replication    /v1/databases/{id}/replication
    └── Pricing        /v1/pricing(只读)

Internal(禁止出现在 SDK / Console)
├── /internal/nodes/*         register / heartbeat / unregister / list / replicas
├── data-node /rpc/*          execute_sql / kv_* / backup / restore / fence / storage_bytes …
└── /admin/*                  grant / vouchers / pricing 管理

Operator(COMBEE_ADMIN_TOKEN)
└── /admin/*(见上)
```

**SDK 禁止暴露**:`/internal/*`、`/rpc/*`、`/admin/*`;NodeId、generation、
placement、fencing、WAL 归档等内部概念不出现在 SDK 表面(见 SDK_SPEC §22)。

## 2. 认证

```http
x-api-key: cmb_sk_...
```

- 密钥格式 `cmb_sk_…`,库中只存 sha256 哈希;
- 隔离在 repository 层强制:跨租户访问一律 404;
- 控制面:`COMBEE_CONTROL_PLANE_TOKEN`(内部);管理面:`COMBEE_ADMIN_TOKEN`(运营)。
  三者互不通用。

## 3. 请求与响应规范

### request-id

- 每个请求可带 `x-request-id`;缺失时服务端生成(UUID);
- **每个响应**(含错误)回显 `x-request-id`;SDK 错误对象暴露 `requestId`。

### 错误模型

统一错误响应:

```json
{ "code": "cell_not_found", "error": "database not found: <uuid>" }
```

| code | HTTP | 含义 |
|---|---|---|
| `database_not_found` | 404 | Cell 不存在(或跨租户,不泄露存在性) |
| `database_already_exists` | 409 | Cell 已存在 |
| `api_key_not_found` | 404 | API key 不存在/已撤销 |
| `unauthorized` | 401 | 未认证 / token 错误 |
| `invalid_request` | 400 | 参数或语句非法 |
| `forbidden` | 403 | 访问内部表等被拒 |
| `quota_exceeded` | 429 | 配额超限 |
| `sql` | 400 | SQL 语法/约束错误 |
| `internal` | 500 | 服务内部错误 |

SDK 映射:`AuthenticationError / PermissionDeniedError / CellNotFoundError /
ApiKeyNotFoundError / InvalidRequestError / SqlError / SqlTimeoutError /
RateLimitError / QuotaExceededError / InsufficientCreditsError / InternalServerError`。

### Pagination

- 游标式:`?limit=N&cursor=<opaque>`;响应 `{"items": [...], "next_cursor": <uuid|null>}`;
- `limit` 默认 100,上限 1000;
- 应用于:`/v1/credits/transactions`(及未来 list 端点)。

### Idempotency-Key

- `POST /v1/databases`(Cell 创建)支持 `Idempotency-Key` 头:同 key 重试返回
  首次创建的 Cell(201 → 后续 200 + 同一 id);
- `POST /v1/credits/redeem` 天然幂等(库内 reference 唯一;重试返回
  `already_redeemed: true`,不重复加钱);
- 写操作默认**不盲目重试**;SDK 重试策略见 SDK_SPEC §19。

## 4. 时间格式

- 时间参数(RFC3339):`from` / `to`(缺省最近 24h);
- 时间桶:`interval = minute | hour | day`(缺省 minute);
- 金额单位:**microcredits**(整数,`CREDIT_UNITS_PER_CREDIT = 1_000_000`,
  即 **1 Credit = 1,000,000 microcredits**);以 decimal string 返回,禁止浮点;
- 示例:发放 1000 Credits 的邀请码 = `amount_units: 1_000_000_000`;余额/账本
  字段(`available`/`amount_units`)单位同为 microcredits。

## 5. 版本与变更

- 版本:`0.1.0-alpha`(冻结);破坏性变更必须升 major(正式版前为 minor);
- 冻结范围:**新增**端点/字段需评审;`code` 语义与 `x-request-id` 永久稳定;
- 变更流程:改本文档 + `openapi.json`(重新生成)+ SDK contract tests。

## 6. OpenAPI

```bash
curl 127.0.0.1:8080/openapi.json
```

覆盖:Data Plane(SQL/KV)+ User Control Plane 核心(Cells/Usage/Credits/API Keys)。
Internal/Admin 明确不进 OpenAPI(有独立内部契约)。
