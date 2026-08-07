# Contributing to Combee

Thanks for your interest! Combee is currently in **V0 freeze**(`v0.1.0-alpha`):
bug fixes, tests, docs, and security hardening are welcome;
**new product features are not** until the release — put them on the backlog first.

## Project layout

```text
crates/
├── common/       # ids / errors / protocol / config / api_key
├── metadata/     # control-plane catalog(InMemory / PostgreSQL)
├── data-node/    # SQLite Runtime + KV Runtime + Active DB Manager + TTL GC + backup/replica/failover
└── api-server/   # Axum HTTP API(Auth / Lifecycle / SQL / KV / control plane)
tests/            # integration tests(+ tests/release/*, tests/control_plane.rs, tests/tenancy.rs …)
docs/             # design / testing / release readiness
```

## Dev setup

```bash
cargo build --workspace
cargo test --workspace        # all unit + integration tests
cargo clippy --workspace --all-targets   # must be 0 warnings
cargo fmt --all               # rustfmt
```

Requires Rust 1.85+(edition 2024). Optional: Docker for `docker compose`
(PostgreSQL + MinIO + multi-node e2e) and PostgreSQL 17 for `COMBEE_METADATA=postgres`.

## Before submitting a PR

1. `cargo test --workspace` — all green;
2. `cargo clippy --workspace --all-targets` — 0 warnings;
3. `cargo fmt --all -- --check` — clean;
4. For behavior changes, add tests under `tests/` documenting **purpose and expected
   result**(see `docs/TESTING.md` for the style);
5. Update `CHANGELOG.md` under `[Unreleased]`;
6. If it touches the release gate, run `./scripts/release-test.sh`.

## Testing conventions

- Each integration test must state its **目的**(purpose) and **预期结果**(expected result);
- Keep tests deterministic: avoid wall-clock dependencies where possible
  (prefer explicit TTL / poll loops with small timeouts);
- SQL escaping / tenant isolation / auth are security-sensitive:
  any change must keep `tests/tenancy.rs` and `tests/control_plane.rs` green.

## Code conventions

- Rust, edition 2024; `cargo fmt` style;
- Errors: use `combee_common::CombeeError`; add variants for new failure classes
  (keep `kind()`/`from_kind()` in sync for RPC round-trip);
- Tenant isolation is enforced at the repository layer:
  always call `get_database(tenant, id)` — never resolve by `id` alone in handlers;
- All blocking SQLite work goes through `spawn_blocking`; never block the async runtime;
- HTTP API changes must keep the `cmb_sk_`/`x-api-key`/`AuthContext` contract stable.

## Scope freeze(V0)

Do not implement: RESP protocol, PG wire protocol, Blob storage, multi-replica (>1),
more complex schedulers. See README "V0 范围冻结".

## License

By contributing you agree that your contributions are licensed under Apache-2.0
(see [`LICENSE`](LICENSE)).
