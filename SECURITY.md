# Security Policy

## Supported Versions

| Version | Supported |
|---|---|
| v0.1.0-alpha.x | ✅(active development) |
| < v0.1.0-alpha.1 | ❌ |

Combee is pre-1.0. Security fixes are backported to the latest `v0.1.0-alpha.x` only.

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

Report privately to the maintainers by email or private channel:

- **Email**: (to be filled in before public release)
- **GitHub private advisory**: use the "Report a vulnerability" flow on the repository.

Please include:

1. Affected version(s) and environment(OS / deployment shape: single-process, 3-container, multi-node);
2. Steps to reproduce(minimal request sequence preferred);
3. Impact description and a suggested fix if you have one.

We aim to acknowledge within **48 hours** and to ship a fix + advisory within
**7 days** for HIGH/CRITICAL issues.

## Scope

In scope(things we consider security-sensitive in V0):

- Authentication & tenant isolation(`x-api-key` / `AuthContext` / `get_database(tenant, id)` boundary);
- Control-plane auth(`COMBEE_CONTROL_PLANE_TOKEN`, `/internal/*` and `/rpc/*`);
- SQL injection surface: multiple statements, `__sys_*` internal tables,
  `ATTACH`/`DETACH`, `VACUUM INTO` file-escape, `load_extension`, CLI-only functions, unbounded recursion;
- Secret handling: API keys stored as sha256 hashes only; S3 credentials via env;
- RPC error propagation(error kind round-trip must not leak internals).

Out of scope in V0(document as known limitations):

- No resource quotas(max KV value / max SQL result / per-cell concurrency caps);
- Metadata defaults to in-memory(production must use PostgreSQL);
- Replication/failover relies on object storage availability.

## Security model summary

```text
client ──x-api-key──▶ API Server
                        ├─ /v1/*      → AuthContext{tenant_id} → repository 层强制隔离(跨租户 404)
                        └─ /internal/*→ COMBEE_CONTROL_PLANE_TOKEN;租户 key 永远 401
Data Node /rpc/*        ← COMBEE_CONTROL_PLANE_TOKEN(与 API Server 共享)
```

## Disclosure

We will publish an advisory for confirmed HIGH/CRITICAL issues after a fix is released.
