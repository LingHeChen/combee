# Combee Cell Identity & Lifecycle Design

> Target: `v0.1.0-beta`
>
> Status: Proposed / Beta-blocking API refinement
>
> Scope: Cell naming, stable identity, idempotent lookup/create semantics, rename/reset behavior, SDK and REST API conventions.
>
> Motivation: A Cell must be easy to rediscover across application restarts without requiring users to persist an opaque UUID manually.

## 1. Problem

Current Cell creation is effectively:

```text
create()
→ generate UUID
→ return Cell
```

If an application restarts without persisting the returned UUID, it cannot naturally rediscover its previous Cell. This can accidentally create multiple Cells and make the application appear to have lost its previous data.

A real application usually thinks in stable logical names:

```text
production
staging
auth
cache
my-app
preview-123
```

Therefore Combee should support **tenant-scoped named Cells**.

## 2. Design Principle

A Cell has two identities:

```text
Immutable Identity
------------------
id: UUID

Human / Application Identity
----------------------------
name: string
```

Core rule:

> **Cell ID is immutable. Cell name is mutable and unique within a tenant.**

The UUID remains the authoritative identity used internally. The name is a tenant-scoped alias used for discovery and application configuration.

## 3. Metadata Model

Recommended conceptual schema:

```sql
cells (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    name TEXT NOT NULL,
    ...
)
```

Required uniqueness:

```sql
UNIQUE (tenant_id, name)
```

Names are not globally unique. All existing internal relationships—usage, credits, routing, backups, replication, fencing, placement—continue using `cell_id`, never `name`, as their permanent identity.

## 4. Cell Name Rules

Recommended Beta validation:

```regex
^[a-z0-9][a-z0-9-_]{0,62}$
```

Examples:

```text
production
blog-prod
preview-123
agent_42
```

Recommended behavior:

- Beta should normalize to lowercase or reject uppercase;
- maximum length: 63 characters;
- trim surrounding whitespace before validation;
- do not add a separate `display_name` unless needed later.

## 5. Resource Semantics

Combee should distinguish:

```text
create
ensure
get
getByName
rename
reset
delete
```

These operations must not be conflated.

## 6. Strict Create

`create` means:

> Create a new Cell. Fail if the requested name already exists.

REST:

```http
POST /v1/cells
```

Request:

```json
{
  "name": "production",
  "region": "auto"
}
```

Behavior:

```text
name does not exist
→ create Cell
→ 201

name already exists
→ CELL_ALREADY_EXISTS
→ 409
```

`create` should remain strict so accidental collisions are visible.

## 7. Ensure

`ensure` is the recommended application startup operation.

Meaning:

> Ensure a Cell with this name exists, and return it.

Behavior:

```text
Cell does not exist
→ create it
→ created = true

Cell already exists
→ return existing Cell
→ created = false
```

This operation MUST be idempotent.

Recommended REST shape:

```http
PUT /v1/cells/by-name/{name}
```

Optional body:

```json
{
  "region": "auto"
}
```

Suggested response:

```json
{
  "cell": {
    "id": "cell_uuid",
    "name": "my-app",
    "region": "auto"
  },
  "created": false
}
```

`PUT` is preferred because the client specifies the logical resource identity and the operation is naturally idempotent.

## 8. SDK API

### TypeScript

```ts
const cell = await combee.cells.ensure("my-app")
```

Optional:

```ts
const cell = await combee.cells.ensure("my-app", {
  region: "auto",
})
```

Strict creation:

```ts
const cell = await combee.cells.create({
  name: "my-app",
})
```

Lookup:

```ts
const cell = await combee.cells.getByName("my-app")
const cellById = await combee.cells.get(cellId)
```

### Python

```python
cell = combee.cells.ensure("my-app")
```

```python
cell = combee.cells.create(name="my-app")
cell = combee.cells.get_by_name("my-app")
```

## 9. Recommended Quickstart

TypeScript:

```ts
const combee = new Combee({
  apiKey: process.env.COMBEE_API_KEY!,
})

const cell = await combee.cells.ensure("my-app")

await cell.sql.execute(`
  CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
  )
`)
```

Python:

```python
combee = Combee(
    api_key=os.environ["COMBEE_API_KEY"],
)

cell = combee.cells.ensure("my-app")

cell.sql.execute("""
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
)
""")
```

Application startup becomes:

```text
boot
↓
ensure Cell
↓
ensure schema
↓
run
```

The application no longer needs to persist a Cell UUID just to rediscover its own data.

## 10. Why `ensure` Instead of `create(if_exists=...)`

The server may internally support conflict policies, but the primary SDK should expose clear operations.

Avoid making destructive behavior easy through:

```ts
cells.create({
  name: "prod",
  ifExists: "reset",
})
```

Prefer explicit semantics:

```text
create  → strict new resource
ensure  → reuse-or-create
reset   → destructive data operation
```

This reduces accidental data loss and makes code self-documenting.

## 11. Low-level If-Exists Semantics

If a low-level API exposes an `if_exists` policy, Beta should keep it small:

```text
error
reuse
```

Definitions:

```text
error:
missing → create
exists  → 409 CELL_ALREADY_EXISTS

reuse:
missing → create
exists  → return existing
```

Do not call the reuse mode `append`. For an existing SQLite database, Combee is simply continuing to use the same Cell.

## 12. Reset

`reset` is destructive and should be explicit and separate from ordinary create/ensure flows.

SDK:

```ts
await cell.reset()
```

Python:

```python
cell.reset()
```

## 13. Reset Semantics

Reset SHOULD preserve the Cell identity.

Before:

```text
id         = abc
name       = test
generation = 4
data       = existing
```

After:

```text
id         = abc
name       = test
generation = 5
data       = empty
```

Reset is:

> Clear/reinitialize Cell data while preserving the logical resource.

Reset is NOT:

```text
delete Cell abc
create new Cell xyz
```

Preserving ID keeps Console URLs, saved references, usage history and resource identity stable.

## 14. Reset and Generation

A reset SHOULD advance generation or another authoritative incarnation/version marker:

```text
generation N
↓
reset
↓
generation N + 1
```

Existing fencing rules should apply so stale writers cannot continue writing against pre-reset state.

## 15. Reset and Replication

Recommended Beta behavior:

```text
1. fence writes
2. create/reset authoritative primary state
3. increment generation
4. invalidate old replica state
5. rebuild or resynchronize replica
6. resume service
```

Old replica state must never overwrite the reset Cell.

## 16. Reset and Backups

Reset MUST NOT silently destroy historical backups unless explicitly requested.

Recommended:

```text
Cell reset
→ active Cell becomes empty
→ previous backups remain available
```

This allows recovery from an accidental reset.

## 17. Rename

Cell names are mutable aliases.

```ts
await cell.rename("production-v2")
```

```python
cell.rename("production-v2")
```

Semantics:

```text
before:
id   = abc
name = production

after:
id   = abc
name = production-v2
```

Lookup behavior:

```text
getByName("production")
→ 404

getByName("production-v2")
→ Cell abc
```

Rename collision:

```text
target name already exists in tenant
→ 409 CELL_ALREADY_EXISTS
```

## 18. Delete

Deleting a Cell removes the named resource from the tenant namespace.

After:

```text
delete production
```

A later:

```text
ensure("production")
```

may create a NEW Cell with a NEW UUID.

Therefore:

```text
reset != delete + ensure
```

## 19. Get by ID vs Get by Name

Both remain supported.

```text
GET /v1/cells/{cell_id}
GET /v1/cells/by-name/{name}
```

SDK:

```ts
cells.get(cellId)
cells.getByName(name)
```

Application code should usually prefer `ensure(name)` or `getByName(name)`. Infrastructure/debug/support/permanent references should prefer immutable `cell_id`.

## 20. API Response Shape

Cell responses should include both:

```json
{
  "id": "9f0c...",
  "name": "production",
  "status": "active",
  "region": "auto",
  "created_at": "..."
}
```

## 21. Console Changes

The Web Console should treat the Cell name as the primary visible label.

Example:

```text
production
cell_9f0c...
```

The UUID becomes secondary/copyable metadata.

Create Cell should require or strongly encourage a name.

## 22. Onboarding Changes

First-run onboarding should create or ask for a Cell name such as:

```text
my-first-app
```

Do not make the primary onboarding flow:

```text
Create Cell
→ copy UUID
→ put UUID in env
```

Preferred:

```text
COMBEE_API_KEY=...
COMBEE_CELL_NAME=my-app
```

and application code uses `ensure(name)`.

## 23. Error Model Additions

Add:

```text
CELL_ALREADY_EXISTS
INVALID_CELL_NAME
CELL_NOT_FOUND
CELL_RESET_FAILED
CELL_RENAME_CONFLICT
```

Suggested mappings:

```text
CELL_ALREADY_EXISTS   → 409
INVALID_CELL_NAME     → 400
CELL_NOT_FOUND        → 404
CELL_RENAME_CONFLICT  → 409
CELL_RESET_FAILED     → 500/503 depending on cause
```

SDK errors should map accordingly.

## 24. Concurrency Requirements

`ensure(name)` must remain correct under concurrent calls.

Example:

```text
process A → ensure("prod")
process B → ensure("prod")
```

Both MUST return the same Cell. Only one Cell may be created.

Required protection:

```text
UNIQUE (tenant_id, name)
```

plus transactional insert/select logic. Do not rely on a race-prone check-then-insert sequence without database-enforced uniqueness.

## 25. Existing Cells Migration

Existing UUID-only Cells need a migration strategy.

Recommended Beta migration:

```text
existing Cell:
id = abc
name = generated default
```

Possible default:

```text
cell-<short-id>
```

Example:

```text
cell-8db81f2a
```

Do not change existing Cell IDs. Users may rename these Cells later.

## 26. Compatibility

Existing ID-based APIs should remain supported throughout `0.1.x`.

The name-based API is additive. Documentation should gradually move primary examples to `ensure(name)`.

## 27. SDK Spec Changes Required

Update `COMBEE_V0.1.0_BETA_SDK_SPEC.md` with:

```text
cells.ensure(name, options?)
cells.getByName(name)
cell.rename(name)
cell.reset()
```

`CellInfo` must include:

```ts
name: string;
```

## 28. Public API Changes Required

Update:

```text
docs/API.md
openapi.json
```

Required semantic surface:

```text
POST   /v1/cells
GET    /v1/cells/:id
GET    /v1/cells/by-name/:name
PUT    /v1/cells/by-name/:name
PATCH  /v1/cells/:id
POST   /v1/cells/:id/reset
DELETE /v1/cells/:id
```

Exact routes may vary, but semantics must remain as documented.

## 29. Tests Required

Metadata:

```text
same tenant duplicate name rejected
different tenants same name allowed
rename preserves ID
rename collision rejected
```

Ensure:

```text
ensure missing → creates
ensure existing → reuses
100 concurrent ensure calls → exactly one Cell
restart + ensure → same Cell ID
```

Reset:

```text
reset preserves ID
reset clears SQL user state
reset clears KV state
reset increments generation/incarnation
old writer fenced
replica stale state cannot overwrite reset state
historical backups remain recoverable
```

Delete:

```text
delete name
ensure same name
→ creates new UUID
```

Tenant isolation:

```text
tenant A getByName("prod")
cannot resolve tenant B prod
```

SDK:

```text
TS ensure parity
Python ensure parity
getByName parity
rename parity
reset parity
typed error parity
```

## 30. Release Gate

This refinement is complete when:

```text
[ ] Cell metadata includes tenant-scoped name
[ ] UNIQUE(tenant_id, name) enforced
[ ] existing Cells migrated without changing IDs
[ ] strict create implemented
[ ] idempotent ensure implemented
[ ] getByName implemented
[ ] rename implemented
[ ] reset implemented with identity preservation
[ ] generation/fencing behavior verified after reset
[ ] Console uses names as primary labels
[ ] OpenAPI updated
[ ] SDK Spec updated
[ ] TypeScript SDK updated
[ ] Python SDK updated
[ ] Quickstart changed to ensure(name)
[ ] concurrency race tests pass
[ ] release tests pass
```

## 31. Final Decision

For `v0.1.0-beta`:

1. Every Cell receives a tenant-scoped name.
2. UUID remains immutable authoritative identity.
3. `create(name)` is strict and conflicts on an existing name.
4. `ensure(name)` is idempotent and is the recommended application startup API.
5. `reset` is an explicit destructive operation, not an ordinary create option.
6. Reset preserves Cell ID and advances generation/incarnation.
7. Rename changes only the alias, not Cell identity.
8. Existing ID-based APIs remain supported.
9. Primary SDK and documentation examples migrate to name-based `ensure`.

> **Names solve discovery. UUIDs preserve identity. `ensure` solves restart ergonomics. Explicit `reset` protects data.**
