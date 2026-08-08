# Combee v0.1.0-beta SDK Specification

> Status: Draft for `v0.1.0-beta`
>
> Scope: Public SDK surface for TypeScript and Python.
>
> Goal: TypeScript and Python SDKs remain behaviorally equivalent and expose only public user/data-plane capabilities. Internal node/control-plane APIs are excluded.

## 1. Product model

```text
Tenant
├── API Keys
├── Usage / Credits
└── Cells
    ├── SQL
    ├── KV + TTL
    ├── Backup / Restore
    └── Replication status
```

A **Cell** is one logical application data space. It is not a dedicated database process.

The SDK MUST use the product term `Cell`. Avoid exposing the historical internal term `database` in normal SDK method names unless required for raw HTTP compatibility.

## 2. Repository strategy

Recommended:

```text
combee/          Core Rust server/runtime
combee-js/       TypeScript / JavaScript SDK
combee-python/   Python SDK
```

Packages:

```text
npm:  @combee/sdk
PyPI: combee
```

Reasons for separate repositories:

- independent SDK/server release cadence;
- independent npm/PyPI publishing;
- smaller, language-native contributor surface;
- clear compatibility matrices;
- future `combee-go`, `combee-rust`, etc. can follow the same model.

The core repository SHOULD keep `docs/SDK_SPEC.md` as the canonical cross-language contract.

## 3. Version compatibility

For beta:

```text
Combee Server: >= 0.1.0-beta
TypeScript SDK: 0.1.x
Python SDK: 0.1.x
```

Every request SHOULD identify the SDK:

```http
User-Agent: combee-js/0.1.x
```

or:

```http
User-Agent: combee-python/0.1.x
```

## 4. Authentication

Public user requests:

```http
x-api-key: cmb_sk_...
```

The key resolves to a tenant.

The public SDK MUST NEVER expose or use:

```text
COMBEE_CONTROL_PLANE_TOKEN
/internal/*
/rpc/*
```

## 5. Client construction

### TypeScript

```ts
import { Combee } from "@combee/sdk";

const combee = new Combee({
  baseUrl: "https://api.combee.example",
  apiKey: process.env.COMBEE_API_KEY!,
});
```

```ts
export interface CombeeOptions {
  baseUrl: string;
  apiKey: string;
  timeoutMs?: number;
  userAgent?: string;
  retry?: {
    maxAttempts?: number;
    baseDelayMs?: number;
    maxDelayMs?: number;
  };
}
```

Recommended defaults:

```text
timeoutMs     30_000
maxAttempts   3
baseDelayMs   100
maxDelayMs    2_000
```

Retries MUST be conservative.

### Python

Sync:

```python
from combee import Combee

combee = Combee(
    base_url="https://api.combee.example",
    api_key="cmb_sk_...",
)
```

Async:

```python
from combee import AsyncCombee

combee = AsyncCombee(
    base_url="https://api.combee.example",
    api_key="cmb_sk_...",
)
```

Python SHOULD use `httpx`.

## 6. Client hierarchy

```text
Combee
├── cells
├── api_keys
├── usage
└── credits

Cell
├── sql
├── kv
├── backups
└── replication
```

Example:

```ts
const cell = await combee.cells.create({ name: "blog-prod" });
await cell.sql.execute(...);
await cell.kv.set(...);
const usage = await combee.usage.summary();
```

# 7. Cells API

## 7.1 Create Cell

TypeScript:

```ts
const cell = await combee.cells.create({
  name: "blog-prod",
  region: "auto",
});
```

Python:

```python
cell = combee.cells.create(
    name="blog-prod",
    region="auto",
)
```

Types:

```ts
interface CreateCellInput {
  name?: string;
  region?: string;
}

interface CellInfo {
  id: string;
  name: string | null;
  region: string | null;
  status: "idle" | "active" | "unavailable";
  createdAt: string;
  storageBytes?: number;
}
```

Creation MUST remain lazy/lightweight.

## 7.2 Get Cell

```ts
const cell = await combee.cells.get("cell-id");
```

```python
cell = combee.cells.get("cell-id")
```

Unknown or cross-tenant Cell -> `CellNotFoundError`.

## 7.3 List Cells

```ts
const result = await combee.cells.list({
  limit: 100,
  cursor,
});
```

```ts
interface Page<T> {
  items: T[];
  nextCursor: string | null;
}
```

Python:

```python
result = combee.cells.list(limit=100, cursor=cursor)
```

Pagination SHOULD be supported before hosted beta.

## 7.4 Delete Cell

```ts
await combee.cells.delete(cellId);
```

```python
combee.cells.delete(cell_id)
```

Destructive operations MUST NOT be blindly retried.

## 7.5 Cell handle

```ts
const cell = combee.cell(cellId);
```

```python
cell = combee.cell(cell_id)
```

This MAY create a local handle without a network request.

The handle exposes:

```text
cell.info()
cell.delete()
cell.sql
cell.kv
cell.backups
cell.replication
```

# 8. SQL API

## 8.1 Query

```ts
const result = await cell.sql.query<User>(
  "SELECT id, name FROM users WHERE id = ?",
  [1],
);
```

```python
result = cell.sql.query(
    "SELECT id, name FROM users WHERE id = ?",
    [1],
)
```

```ts
interface SqlQueryResult<T = Record<string, unknown>> {
  columns: string[];
  rows: T[];
  elapsedMs?: number;
}
```

## 8.2 Execute

```ts
const result = await cell.sql.execute(
  "UPDATE users SET name = ? WHERE id = ?",
  ["Alice", 1],
);
```

```ts
interface SqlExecuteResult {
  rowsAffected: number;
  lastInsertRowId?: number | string | null;
}
```

## 8.3 Transaction

```ts
const result = await cell.sql.transaction([
  {
    sql: "INSERT INTO users (name) VALUES (?)",
    params: ["Alice"],
  },
  {
    sql: "UPDATE stats SET value = value + 1 WHERE key = ?",
    params: ["users"],
  },
]);
```

```python
result = cell.sql.transaction([
    {
        "sql": "INSERT INTO users (name) VALUES (?)",
        "params": ["Alice"],
    },
    {
        "sql": "UPDATE stats SET value = value + 1 WHERE key = ?",
        "params": ["users"],
    },
])
```

```ts
interface SqlStatement {
  sql: string;
  params?: SqlParam[];
}
```

Supported parameter values:

```text
null
boolean
integer
float
string
bytes/blob (only if the HTTP API supports it)
```

## 8.4 SQL timeout

```ts
await cell.sql.query(sql, params, {
  timeoutMs: 5000,
});
```

The server remains authoritative and MAY enforce a stricter maximum.

# 9. KV API

The beta SDK MUST expose the full currently-supported public KV subset.

## 9.1 Get

```ts
const value = await cell.kv.get<string>("session:abc");
```

```python
value = cell.kv.get("session:abc")
```

Missing key SHOULD return `null` / `None`.

## 9.2 Set

```ts
await cell.kv.set("session:abc", value, {
  ttl: 3600,
  condition: "nx",
});
```

```ts
interface KvSetOptions {
  ttl?: number;
  condition?: "nx" | "xx";
}

interface KvSetResult {
  applied: boolean;
}
```

Python:

```python
result = cell.kv.set(
    "session:abc",
    value,
    ttl=3600,
    condition="nx",
)
```

## 9.3 Delete

```ts
const deleted = await cell.kv.delete("session:abc");
```

Return `boolean`.

## 9.4 Exists

```ts
const exists = await cell.kv.exists("session:abc");
```

## 9.5 MGET

```ts
const values = await cell.kv.mget([
  "user:1",
  "user:2",
]);
```

Result preserves order:

```text
Array<KvValue | null>
```

## 9.6 MSET

```ts
await cell.kv.mset({
  "a": "1",
  "b": "2",
});
```

## 9.7 TTL

```ts
const ttl = await cell.kv.ttl("session:abc");
```

Recommended normalized result:

```ts
type KvTtl =
  | { state: "expires"; seconds: number }
  | { state: "persistent" }
  | { state: "missing" };
```

Do not leak Redis sentinel values into the high-level SDK.

## 9.8 Expire

```ts
const changed = await cell.kv.expire("session:abc", 60);
```

## 9.9 Persist

```ts
const changed = await cell.kv.persist("session:abc");
```

## 9.10 Increment / decrement

```ts
const n1 = await cell.kv.incr("pageviews");
const n2 = await cell.kv.incr("pageviews", 5);

const n3 = await cell.kv.decr("quota");
const n4 = await cell.kv.decr("quota", 2);
```

Must remain atomic.

# 10. Backup API

Backup/restore SHOULD be public Cell-management functionality in beta.

## 10.1 Full snapshot

```ts
const backup = await cell.backups.create();
```

```python
backup = cell.backups.create()
```

```ts
interface BackupInfo {
  id: string;
  type: "snapshot" | "incremental";
  createdAt: string;
  sizeBytes?: number;
}
```

## 10.2 Incremental backup

```ts
const backup = await cell.backups.createIncremental();
```

May be documented as advanced.

## 10.3 List backups

```ts
const backups = await cell.backups.list();
```

## 10.4 Restore

Latest:

```ts
await cell.backups.restoreLatest();
```

Specific:

```ts
await cell.backups.restore(backupId);
```

Restore is destructive and MUST NOT be silently retried.

# 11. Replication API

## 11.1 Status

```ts
const status = await cell.replication.get();
```

```ts
interface ReplicationStatus {
  enabled: boolean;
  primaryNode?: string;
  replicaNode?: string;
  generation?: number;
  state?:
    | "healthy"
    | "catching_up"
    | "degraded"
    | "unavailable";
  lagMs?: number | null;
}
```

Hosted Combee SHOULD hide physical node IDs when possible.

## 11.2 Enable replica

Self-hosted/admin-style:

```ts
await cell.replication.enable({
  replicaNode: nodeId,
});
```

Hosted Combee preferred:

```ts
await cell.replication.enable({
  region: "auto",
});
```

## 11.3 Disable replica

```ts
await cell.replication.disable();
```

## 11.4 Manual failover

Not required in the default public SDK.

If exposed:

```ts
await cell.replication.failover();
```

mark it advanced/destructive.

# 12. API Key management

## 12.1 Create key

```ts
const created = await combee.apiKeys.create({
  name: "production",
});
```

```ts
interface CreatedApiKey {
  id: string;
  name: string | null;
  key: string; // returned once
  prefix: string;
  createdAt: string;
}
```

Plaintext key MUST only be returned once.

## 12.2 List keys

```ts
const keys = await combee.apiKeys.list();
```

```ts
interface ApiKeyInfo {
  id: string;
  name: string | null;
  prefix: string;
  createdAt: string;
  lastUsedAt?: string | null;
  revokedAt?: string | null;
}
```

## 12.3 Revoke key

```ts
await combee.apiKeys.revoke(keyId);
```

Revocation MUST take effect immediately.

# 13. Usage API

Usage metering is required before credits/billing.

SDKs expose summaries, not raw internal events by default.

## 13.1 Tenant summary

```ts
const usage = await combee.usage.summary({
  from: "2026-08-01T00:00:00Z",
  to: "2026-08-08T00:00:00Z",
});
```

```ts
interface UsageSummary {
  period: {
    from: string;
    to: string;
  };

  operations: {
    kvReads: number;
    kvWrites: number;
    sqlReads: number;
    sqlWrites: number;
  };

  requestCount: number;

  bytesIn: number;
  bytesOut: number;

  storageByteHours?: number;
  currentStorageBytes: number;

  billableUnits?: number;
}
```

## 13.2 Per-Cell usage

Preferred:

```ts
const usage = await cell.usage.summary({
  from,
  to,
});
```

## 13.3 Timeseries

```ts
const series = await combee.usage.timeseries({
  metric: "requests",
  interval: "hour",
  from,
  to,
});
```

Beta metrics MAY include:

```text
requests
kv_reads
kv_writes
sql_reads
sql_writes
storage_bytes
bytes_out
billable_units
```

# 14. Credits API

Credits belong to the **user control plane**, not the Data Node hot path.

## 14.1 Balance

```ts
const credits = await combee.credits.balance();
```

```ts
interface CreditBalance {
  available: string;
  reserved?: string;
  currency: "CREDIT";
  updatedAt: string;
}
```

Use decimal strings or integer base units. Never use IEEE floating-point for balances.

## 14.2 Ledger

```ts
const entries = await combee.credits.transactions({
  limit: 100,
  cursor,
});
```

```ts
interface CreditTransaction {
  id: string;
  type:
    | "recharge"
    | "usage"
    | "grant"
    | "voucher"
    | "refund"
    | "adjustment";
  amount: string;
  balanceAfter?: string;
  description?: string;
  createdAt: string;
}
```

Ledger entries MUST be immutable. Corrections create compensating entries.

## 14.3 Redeem voucher

```ts
const result = await combee.credits.redeem(
  "CMB-XXXX-XXXX-XXXX"
);
```

```ts
interface RedeemResult {
  creditsAdded: string;
  balance: string;
}
```

Redemption MUST be idempotent. A voucher MUST not be double-spendable.

# 15. Administrative credits API

Admin grants MUST NOT use tenant API keys.

Possible operator endpoint:

```text
POST /admin/tenants/:tenant_id/credits/grant
```

Payload:

```json
{
  "amount_units": 100000000,
  "reason": "alpha tester grant"
}
```

Protect separately using e.g.:

```text
COMBEE_ADMIN_TOKEN
```

or a future operator IAM system.

The default public SDK SHOULD NOT expose this.

If needed later, expose a separate:

```text
CombeeAdmin
```

client.

# 16. Pricing/configuration API

Pricing rules SHOULD be server-side and hot-reloadable.

Read-only public endpoint MAY exist:

```ts
const pricing = await combee.pricing.get();
```

```ts
interface PricingConfig {
  version: number;
  effectiveAt: string;

  units: {
    kvRead: string;
    kvWrite: string;
    sqlRead: string;
    sqlWrite: string;
    storageByteHour: string;
    egressByte: string;
  };
}
```

The SDK MUST NOT calculate authoritative billing locally.

# 17. Error model

TypeScript:

```ts
class CombeeError extends Error {
  code: string;
  status?: number;
  requestId?: string;
}
```

Python:

```python
class CombeeError(Exception):
    code: str
    status: int | None
    request_id: str | None
```

Required normalized errors:

```text
AuthenticationError
PermissionDeniedError
CellNotFoundError
ApiKeyNotFoundError
InvalidRequestError
SqlError
SqlTimeoutError
KvTypeError
ConflictError
RateLimitError
QuotaExceededError
InsufficientCreditsError
DataNodeUnavailableError
RestoreError
ReplicationError
InternalServerError
```

# 18. Request IDs

Every response SHOULD include:

```http
x-request-id
```

SDK errors SHOULD expose it.

# 19. Retry policy

Safe automatic retries generally include:

```text
GET
list
usage reads
credit balance reads
KV GET
KV EXISTS
KV TTL
```

Writes SHOULD only be retried when server-side idempotency is guaranteed.

Beta SHOULD support `Idempotency-Key` for:

```text
Cell creation
API key creation
voucher redemption
future recharge requests
```

Do NOT blindly retry:

```text
SQL transaction
restore
manual failover
arbitrary SQL writes
INCR
```

# 20. Cancellation

TypeScript SHOULD support `AbortSignal`.

Python async SHOULD support task cancellation.

Server SQL timeout remains authoritative.

# 21. KV serialization

KV representation MUST be explicit.

Recommended supported high-level values:

```text
string
bytes
JSON-compatible values
```

If the current server only stores strings, SDKs MUST NOT pretend arbitrary objects are natively supported.

Prefer explicit helpers:

```ts
cell.kv.setJson(...)
cell.kv.getJson(...)
```

if JSON encoding is client-side.

# 22. Interfaces explicitly NOT exposed

Normal SDKs MUST NOT expose:

```text
/internal/nodes/*
/rpc/*
Node registration
Node heartbeat
Node unregister
raw NodeRegistry mutation
raw storage_node_id reassignment
generation fencing mutation
internal WAL archival
internal replica pull
control-plane token handling
```

Users operate on Cells, not physical nodes.

# 23. TypeScript minimum export surface

```ts
export {
  Combee,
  CombeeError,

  AuthenticationError,
  PermissionDeniedError,
  CellNotFoundError,
  InvalidRequestError,
  SqlError,
  SqlTimeoutError,
  RateLimitError,
  QuotaExceededError,
  InsufficientCreditsError,
};

export type {
  CombeeOptions,
  CellInfo,
  CreateCellInput,
  SqlStatement,
  SqlQueryResult,
  SqlExecuteResult,
  KvSetOptions,
  KvSetResult,
  KvTtl,
  BackupInfo,
  ReplicationStatus,
  ApiKeyInfo,
  CreatedApiKey,
  UsageSummary,
  CreditBalance,
  CreditTransaction,
  Page,
};
```

# 24. Python minimum export surface

```python
from combee import (
    Combee,
    AsyncCombee,

    CombeeError,
    AuthenticationError,
    PermissionDeniedError,
    CellNotFoundError,
    InvalidRequestError,
    SqlError,
    SqlTimeoutError,
    RateLimitError,
    QuotaExceededError,
    InsufficientCreditsError,

    CellInfo,
    SqlQueryResult,
    SqlExecuteResult,
    BackupInfo,
    ReplicationStatus,
    UsageSummary,
    CreditBalance,
)
```

# 25. Required examples

Each SDK repo MUST include:

```text
01_create_cell
02_sql_crud
03_sql_transaction
04_kv_basic
05_kv_ttl
06_kv_counter
07_backup_restore
08_api_keys
09_usage
10_credits
```

Python additionally:

```text
async_basic.py
```

# 26. Required SDK tests

Unit tests:

```text
authentication headers
URL construction
serialization
error mapping
pagination
retry policy
timeout handling
KV TTL mapping
credit integer/base-unit handling
```

Contract tests against a real local Combee server:

```text
create/list/get/delete Cell
SQL execute/query
transaction rollback
KV full subset
TTL expiry
backup/restore
API key lifecycle
tenant isolation
usage read
credit balance/ledger
voucher redemption (if enabled)
```

TypeScript and Python SHOULD produce equivalent server-visible behavior.

# 27. v0.1.0-beta acceptance criteria

```text
[ ] TypeScript SDK published to npm
[ ] Python SDK published to PyPI
[ ] TS and Python implement the same required feature matrix
[ ] SDK_SPEC is the canonical cross-language contract
[ ] Quickstart uses SDK instead of curl
[ ] typed error model documented
[ ] conservative retry behavior
[ ] pagination supported where required
[ ] tenant-isolation contract tests pass
[ ] usage APIs exposed
[ ] credits read APIs exposed
[ ] internal control-plane APIs NOT exposed
[ ] executable examples included
[ ] CI runs contract tests against a real Combee server
```

# 28. Proposed implementation order

```text
1. Freeze public HTTP API shapes
2. Implement usage metering endpoints
3. Implement credits/pricing/voucher endpoints
4. Produce OpenAPI or another machine-readable public API schema
5. Implement TypeScript SDK
6. Implement Python SDK against the same SDK_SPEC
7. Add cross-language contract tests
8. Publish beta packages
9. Replace landing/docs curl examples with SDK examples
10. Build Web Console on the same public management APIs
```

Architecture:

```text
                  Combee API

       ┌─────────────┼──────────────┐
       │             │              │
   TypeScript      Python       Web Console
      SDK            SDK
       │             │              │
       └─────────────┴──────────────┘
                     │
              same public API
```

The Web Console MUST NOT use privileged shortcuts into internal Data Node APIs.

# 29. Beta UX target

> One app. One Cell. SQL + KV included.

TypeScript:

```ts
const combee = new Combee({
  baseUrl: process.env.COMBEE_URL!,
  apiKey: process.env.COMBEE_API_KEY!,
});

const cell = await combee.cells.create({
  name: "my-app",
});

await cell.sql.execute(
  "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)"
);

await cell.sql.execute(
  "INSERT INTO users (name) VALUES (?)",
  ["Alice"],
);

await cell.kv.set(
  "session:abc",
  "user:1",
  { ttl: 3600 },
);
```

Python:

```python
from combee import Combee

combee = Combee(
    base_url=COMBEE_URL,
    api_key=COMBEE_API_KEY,
)

cell = combee.cells.create(name="my-app")

cell.sql.execute(
    "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)"
)

cell.sql.execute(
    "INSERT INTO users (name) VALUES (?)",
    ["Alice"],
)

cell.kv.set(
    "session:abc",
    "user:1",
    ttl=3600,
)
```

This is the primary SDK UX target for `v0.1.0-beta`.
