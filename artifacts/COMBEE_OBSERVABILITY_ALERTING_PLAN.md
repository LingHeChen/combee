# Combee Observability & Alerting Plan

> Target: Cloud Alpha / v0.1.0-beta  
> Status: Proposed implementation plan  
> Scope: Logging, metrics, health probes, alerting, deployment events, retention, incident response  
> Deployment assumption: Docker Swarm on Tencent Cloud, single-region Alpha, PostgreSQL metadata store, Combee API + Console + DataNode + external object storage.

---

# 1. Goals

The observability system must answer these questions quickly:

1. Is Combee available to users?
2. Which component is failing?
3. When did the failure begin?
4. Which version/deployment introduced it?
5. Which tenants / Cells / requests are affected?
6. Is data durability at risk?
7. Does the incident require immediate intervention?
8. Can the operator identify a relevant `request_id` and find the complete request path?

The Alpha target is not a full enterprise observability platform.

The target is:

> **Detect important failures quickly, preserve enough context to diagnose them, and avoid operating a heavy self-hosted monitoring stack.**

---

# 2. Principles

## 2.1 Managed first

Prefer managed observability services over self-hosting Prometheus + Grafana + Loki during Alpha.

Recommended responsibility split:

```text
Combee
├── emits structured logs
├── exposes health/readiness probes
├── exposes low-cardinality metrics
└── emits lifecycle/deployment events

Managed platform
├── stores/searches logs
├── collects host metrics
├── collects application metrics
├── performs external probes
└── sends alerts
```

Suggested Tencent Cloud mapping:

```text
Application logs      → CLS
Host metrics          → Tencent Cloud Monitor
External availability → Cloud probing / independent uptime monitor
Application metrics   → Managed Prometheus (P1, not required for first Alpha)
Notifications         → WeCom / email / SMS / phone as appropriate
```

## 2.2 PostgreSQL is not an observability database

Do not write per-request logs or metrics into Combee metadata PostgreSQL.

PostgreSQL stores application/control-plane metadata. Logs and time-series telemetry belong in dedicated observability systems.

## 2.3 Logs and metrics have different jobs

```text
Metrics
→ Is the system unhealthy?

Logs
→ What exactly happened?

Probes
→ Can a real external client reach the service?

Events
→ What changed immediately before the incident?
```

Do not use high-cardinality identifiers such as `cell_id`, `tenant_id`, or `request_id` as metrics labels. Those belong in logs.

---

# 3. High-Level Architecture

```text
                           Internet
                              │
                      External Uptime Probe
                              │
                              ▼
                           Caddy
                              │
                   ┌──────────┴───────────┐
                   │                      │
              Console / BFF            API
                   │                      │
                   │                  DataNode
                   │                      │
                   └──────────┬───────────┘
                              │
                  Structured JSON stdout
                              │
                              ▼
                         Log Collector
                              │
                              ▼
                         Tencent CLS
                              │
                 ┌────────────┴────────────┐
                 │                         │
             Log Alerts                Search
                 │
                 ▼
          Alert Notification

Tencent Cloud host metrics
        │
        └───────────────► Alert Notification

Combee /metrics (P1)
        │
        ▼
Managed Prometheus
        │
        └───────────────► Alert Notification
```

---

# 4. Existing Logging Baseline

Combee already has a Logging P0 baseline:

```text
structured JSON output
service
request_id
tenant
cell
operation
status
latency_ms
error_code

request_id propagation:
BFF → API → DataNode RPC

event-style logging:
cell
usage flush
settlement
backup
replica
failover
node
```

Sensitive values must never be logged:

```text
password
API key secret
session secret
voucher code
SQL parameters
KV values
```

The observability plan builds on this contract rather than replacing it.

---

# 5. Required Common Log Schema

Every production service SHOULD emit JSON logs using a common envelope.

Recommended fields:

```json
{
  "timestamp": "2026-08-10T10:00:00.000Z",
  "level": "INFO",
  "environment": "production",
  "service": "combee-api",
  "version": "git-sha",
  "instance": "swarm-task-or-host",
  "event": "request.completed",
  "request_id": "req_...",
  "tenant_id": "tenant_...",
  "cell_id": "cell_...",
  "operation": "kv.get",
  "status": 200,
  "latency_ms": 3.4,
  "error_code": null
}
```

Required global fields:

```text
timestamp
level
environment
service
version
instance
event
```

Request-path fields when applicable:

```text
request_id
tenant_id
cell_id
operation
status
latency_ms
error_code
```

Infrastructure/background-job fields when applicable:

```text
node_id
job
attempt
duration_ms
generation
replica_lag_ms
backup_id
```

---

# 6. Log Levels

## ERROR

A requested operation failed unexpectedly or a background job cannot complete.

Examples:

```text
request.failed
backup.failed
failover.failed
usage.flush.failed_after_retries
settlement.failed_after_retries
postgres.unavailable
object_store.unavailable
```

## WARN

The system degraded, retried, rejected, or approached a dangerous condition.

Examples:

```text
rate_limit.exceeded
quota.exceeded
replica.lag_high
node.heartbeat_timeout
request.slow
backup.retry
```

## INFO

Important lifecycle or control-plane events.

Examples:

```text
service.started
service.ready
node.registered
cell.open
cell.evict
backup.completed
failover.started
failover.completed
deployment.started
deployment.completed
```

## DEBUG

Detailed request and internal diagnostics.

Production default: disabled.

## TRACE

Development only.

---

# 7. Required Event Names

Use stable event names so alerts can query exact events.

## Request

```text
request.completed
request.failed
request.slow
```

## Cell

```text
cell.created
cell.open
cell.sleep
cell.evict
cell.reset
cell.deleted
```

## Node

```text
node.registered
node.heartbeat
node.heartbeat_timeout
node.unavailable
node.recovered
```

Do not log every successful heartbeat at INFO. Successful heartbeats SHOULD be DEBUG or omitted.

## Usage

```text
usage.flush.completed
usage.flush.failed
usage.flush.recovered
```

## Credits

```text
credits.settlement.completed
credits.settlement.failed
credits.settlement.recovered
```

## Backup

```text
backup.started
backup.completed
backup.failed
backup.retry
```

## Replica

```text
replica.catchup
replica.lag_high
replica.unhealthy
replica.recovered
```

## Failover

```text
failover.started
failover.promoted
failover.completed
failover.failed
```

## Dependencies

```text
postgres.unavailable
postgres.recovered
object_store.unavailable
object_store.recovered
```

## Deployment

```text
deployment.started
deployment.completed
deployment.failed
deployment.rollback_started
deployment.rollback_completed
```

---

# 8. Request ID Contract

`request_id` is the primary diagnostic correlation identifier.

Propagation path:

```text
Browser/BFF
    ↓
Combee API
    ↓
DataNode RPC
```

Rules:

1. Accept an incoming valid request ID when appropriate, otherwise generate one.
2. Preserve the same ID through downstream RPC.
3. Include it in relevant logs.
4. Return it in API error responses.
5. Display it in Console error UI.
6. Make it copyable.

Support workflow:

```text
user reports request_id
→ search CLS
→ reconstruct BFF/API/DataNode request path
```

---

# 9. Sensitive Data Policy

Production logs MUST NOT contain:

```text
cmb_sk_* full secrets
passwords
session IDs/secrets
voucher plaintext codes
authorization headers
cookies
SQL bound parameter values
KV values
full request bodies by default
```

SQL logging:

```text
Preferred:
operation = sql.execute

Optional DEBUG:
normalized/truncated SQL text

Never:
bound user parameters by default
```

KV logging:

```text
Allowed:
operation
key length
value size
TTL
success/failure

Avoid:
full value
```

Keys may also contain secrets or PII. Full KV keys SHOULD not be logged by default.

---

# 10. Health and Readiness Endpoints

Expose two separate endpoints.

## `/health`

Meaning: the process is alive.

Should not perform expensive dependency checks.

Typical use: container liveness.

## `/ready`

Meaning: this instance is able to serve real user traffic.

API readiness may include:

```text
metadata PostgreSQL reachable
required internal state initialized
at least one viable DataNode route when required by current architecture
```

DataNode readiness may include:

```text
runtime initialized
local storage writable
node identity loaded
fencing state valid
```

A dependency failure should cause readiness to fail before the process necessarily exits.

---

# 11. Docker Swarm Health Integration

Every stateless service SHOULD define a healthcheck.

Example concept:

```yaml
healthcheck:
  test: ["CMD", "curl", "-f", "http://localhost:8080/ready"]
  interval: 10s
  timeout: 3s
  retries: 3
```

API / Console rolling update:

```text
order: start-first
failure_action: rollback
```

DataNode:

```text
order: stop-first
```

until a Combee-aware drain/upgrade workflow exists.

Do not treat Swarm health checks as a substitute for external probes.

---

# 12. External Availability Probes

External probes MUST run outside the Combee host.

Minimum checks:

```text
https://combee.cloud/
https://console.combee.cloud/
https://api.combee.cloud/ready
```

Recommended interval:

```text
1 minute
```

Alert condition:

```text
2–3 consecutive failures
```

Do not alert on a single transient timeout.

External probes catch failures in:

```text
DNS
TLS
Caddy
security groups
public networking
server outage
```

---

# 13. Host Metrics

Use Tencent Cloud host monitoring for Alpha.

Required host-level metrics:

```text
CPU usage
memory usage
disk utilization
disk IO latency / IO pressure where available
network ingress/egress
load average
```

Suggested initial thresholds:

## Disk

```text
> 70% for 10m → P2 Warning
> 85% for 5m  → P1 High
> 95% for 1m  → P0 Critical
```

## Memory

```text
> 80% for 10m → P2
> 90% for 5m  → P1
> 97% for 2m  → P0
```

## CPU

```text
> 70% for 15m → P2
> 85% for 10m → P1
> 95% for 5m  → P1/P0 depending on user impact
```

Sustained utilization matters more than short spikes.

---

# 14. Application Metrics

Application metrics are P1 for Alpha.

Combee SHOULD expose a Prometheus-compatible:

```text
GET /metrics
```

Minimum metric set:

```text
combee_http_requests_total
combee_http_errors_total
combee_request_duration_seconds

combee_active_cells
combee_open_sqlite_connections

combee_usage_flush_failures_total
combee_usage_flush_lag_seconds

combee_credit_settlement_failures_total
combee_credit_settlement_lag_seconds

combee_backup_failures_total
combee_last_successful_backup_timestamp

combee_replica_lag_seconds

combee_failovers_total
combee_failover_failures_total

combee_postgres_errors_total
combee_object_store_errors_total
```

---

# 15. Metrics Cardinality Rules

Allowed labels should be low-cardinality.

Good:

```text
service
operation
status_class
node_role
error_class
```

Forbidden or strongly discouraged metric labels:

```text
tenant_id
cell_id
request_id
api_key_id
voucher_id
full URL
SQL text
KV key
```

These identifiers belong in logs.

---

# 16. Initial Alert Severity Model

Use four levels.

## P0 — Critical

Immediate operator attention.

Examples:

```text
public API unavailable
metadata PostgreSQL unavailable
all DataNodes unavailable
disk > 95%
failover failed
restore failed during an active recovery
persistent settlement corruption/risk
```

Notification:

```text
WeCom + SMS
phone optional
```

## P1 — High

Investigate within minutes.

Examples:

```text
5xx error ratio sustained above threshold
DataNode heartbeat timeout
replica lag severe
backup repeatedly failing
memory > 90%
disk > 85%
usage flush stalled
settlement stalled
```

Notification:

```text
WeCom + email
```

## P2 — Warning

Investigate during working hours.

Examples:

```text
disk > 70%
high latency
unusual 429 rate
single backup retry
replica lag elevated but recoverable
storage growth anomaly
```

Notification:

```text
WeCom or email
```

## P3 — Info

No active notification.

Examples:

```text
deployment completed
node registered
backup completed
failover completed
```

---

# 17. Core Alpha Alert Rules

## Public API unavailable

Source: external probe.

```text
/ready fails 3 consecutive checks
→ P0
```

## API 5xx spike

Initial condition:

```text
>= 10 HTTP 5xx responses in 5 minutes
AND
5xx ratio >= 5% when denominator is available
→ P1
```

Do not page on one isolated 500.

## PostgreSQL unavailable

```text
postgres.unavailable sustained > 30s
OR readiness failure caused by PostgreSQL
→ P0/P1
```

Use P0 when public API is unavailable.

## All DataNodes unavailable

```text
no healthy/eligible DataNode available
→ P0
```

## Single DataNode heartbeat lost

```text
heartbeat exceeds configured failure timeout
→ P1
```

Include `node_id`, last heartbeat and affected Cell count if available.

## Backup failure

```text
single failure → P2
3 consecutive failures → P1
no successful backup for expected durability window → P1
```

## Replica lag

Initial suggested values:

```text
> 30s for 5m → P2
> 5m for 2m  → P1
```

Tune after observing real workload.

## Failover failed

```text
event = failover.failed
→ P0
```

## Usage flush stalled

```text
usage flush failures continuously for > 5m
OR flush lag exceeds acceptable window
→ P1
```

## Credit settlement stalled

```text
settlement lag > 10m
OR persistent failed settlement
→ P1
```

During early Alpha, avoid hard-disconnecting users solely because of a temporary settlement telemetry problem.

## Object storage unavailable

```text
transient failure → retry + log
sustained failure → P1
```

Escalate to P0 when it prevents required recovery/failover operations.

---

# 18. Deployment Observability

Every production deployment SHOULD emit:

```text
deployment.started
deployment.completed
deployment.failed
deployment.rollback_started
deployment.rollback_completed
```

Required fields:

```text
version
previous_version
deployment_id
environment
started_at
duration_ms
```

This lets an operator correlate:

```text
new deployment
↓
latency/error spike
```

without guessing.

---

# 19. Alert Message Format

Alerts must contain enough context to decide whether immediate action is necessary.

Bad:

```text
ERROR: service failed
```

Good:

```text
[P1] Combee API error rate elevated

Environment: production
Service: combee-api
Version: a13f82c

5xx rate: 7.2%
Window: 5m
Requests: 4,231
Errors: 305

Top error:
DATA_NODE_UNAVAILABLE

Started:
2026-08-10 18:23 +08:00

Request sample:
req_...

Logs:
<managed log query link>
```

---

# 20. Alert Deduplication

Avoid alert storms.

Rules:

1. Group repeated events by service/error class.
2. Use a cooldown window.
3. Send one active incident notification instead of one message per log.
4. Send a recovery notification when the condition clears.

Example:

```text
18:20 P1 API 5xx elevated
18:21 same condition → suppress
18:22 same condition → suppress
18:27 recovered → send recovery
```

---

# 21. Log Retention

Initial recommendation:

```text
ERROR / WARN
30 days

INFO
7–14 days

DEBUG
disabled in normal production
```

Do not retain every high-volume successful request indefinitely.

Possible successful request strategy:

```text
errors                 → always keep
slow requests          → always keep
important lifecycle    → always keep
normal successful GET  → sample or summarize
```

---

# 22. Slow Request Logging

Define an initial slow-request threshold.

Suggested starting point:

```text
API request > 500 ms
```

Emit:

```text
event = request.slow
level = WARN
```

Do not alert on every slow request. Alert only on sustained latency degradation.

---

# 23. User-Facing Quota/Rate-Limit Telemetry

Expected user-limit events should be distinguishable from server failures.

Examples:

```text
quota.exceeded
rate_limit.exceeded
```

These should normally be WARN/structured events, not ERROR.

Track aggregate 429 volume. High 429 rate may indicate:

```text
bad default quota
abusive client
SDK retry bug
unexpected production workload
```

---

# 24. Initial Operational Dashboard

Do not build a giant dashboard. One Alpha dashboard is enough.

## Availability

```text
API readiness
Console availability
5xx rate
request latency p50/p95/p99
```

## Capacity

```text
CPU
RAM
disk %
disk growth
network
active Cells
open SQLite connections
```

## Durability

```text
last successful backup
backup failures
replica lag
failover count/failures
object storage errors
```

## Accounting

```text
usage flush lag
usage flush failures
credit settlement lag
settlement failures
```

## Release

```text
current version
last deployment
last rollback
```

---

# 25. Incident Runbooks

Every P0/P1 alert SHOULD have a short runbook.

Start with six runbooks.

## API unavailable

Check:

```text
external probe
Caddy
Swarm service state
API logs
PostgreSQL availability
DataNode availability
recent deployment
```

Actions:

```text
rollback latest API deployment if correlated
restart failed stateless task if necessary
do not restart PostgreSQL/DataNode blindly
```

## PostgreSQL unavailable

Check:

```text
container/service state
disk
memory
PostgreSQL logs
connection exhaustion
recent migration
```

Avoid destructive recovery actions without a backup.

## DataNode unavailable

Check:

```text
node heartbeat
Swarm task status
local disk
DataNode logs
generation/fencing
replica state
object storage
```

Do not manually promote stale state without respecting Combee generation/fencing rules.

## Disk nearly full

Check largest consumers:

```text
Cell files
WAL
Docker layers
logs
PostgreSQL
temporary snapshots
```

Actions:

```text
do not delete Cell/WAL files blindly
clean safe Docker artifacts first
verify backup/object storage state
expand disk if necessary
```

## Backup failure

Check:

```text
object storage credentials/connectivity
local free disk
snapshot/WAL logs
specific Cell failures
retry history
```

## Deployment regression

Check:

```text
deployment version
error-rate start time
latency start time
new error codes
```

If strongly correlated:

```text
docker service rollback
or deploy previous immutable image tag
```

---

# 26. Swarm-Specific Requirements

Service logs must identify:

```text
Swarm service
task/instance
host/node
image version
```

Stateless service failures should be recoverable by Swarm.

Stateful failures remain Combee-aware.

Important distinction:

```text
Swarm
→ process orchestration

Combee
→ data placement / primary ownership / replication / generation fencing
```

Do not treat `replicas: N` as database-level HA for DataNodes.

---

# 27. Observability of NodeRegistry / Routing

As API becomes multi-replica, routing/control-plane metadata must be observable.

Recommended events:

```text
route.cache.hit
route.cache.miss
route.cache.invalidated
route.lookup.failed
route.stale_generation
```

Do not log every cache hit at INFO.

Metrics may include aggregate:

```text
route_cache_hits_total
route_cache_misses_total
route_refresh_failures_total
```

No `cell_id` metric labels.

---

# 28. Production Version Identification

Every service MUST expose or log its exact build version.

Recommended:

```text
COMBEE_VERSION=<git sha or release tag>
```

At startup:

```json
{
  "event": "service.started",
  "service": "combee-api",
  "version": "a13f82c"
}
```

Optional health response:

```json
{
  "status": "ok",
  "version": "a13f82c"
}
```

---

# 29. Phase Plan

## P0 — Before opening Cloud Alpha

Required:

```text
[ ] JSON stdout logs from all production services
[ ] common log schema
[ ] environment/version/instance/event fields
[ ] request_id end-to-end
[ ] logs collected into managed log service
[ ] sensitive data policy verified
[ ] /health
[ ] /ready
[ ] external probes
[ ] host CPU/RAM/disk alerts
[ ] API-down alert
[ ] 5xx-spike alert
[ ] PostgreSQL unavailable alert
[ ] DataNode unavailable alert
[ ] repeated backup failure alert
[ ] failover failed alert
[ ] usage flush stalled alert
[ ] settlement stalled alert
[ ] deployment events
[ ] alert deduplication + recovery notifications
[ ] six minimal incident runbooks
```

This is sufficient for Closed/Public Alpha.

## P1 — After real users begin using Combee

Add:

```text
[ ] Prometheus-compatible /metrics
[ ] managed Prometheus collection
[ ] RPS
[ ] error ratio
[ ] p50/p95/p99 latency
[ ] active Cells
[ ] open SQLite connections
[ ] replica lag metric
[ ] backup age
[ ] usage flush lag
[ ] settlement lag
[ ] one operational dashboard
```

## P2 — Only when scale justifies it

Consider:

```text
OpenTelemetry
distributed tracing
trace sampling
SLOs
error budgets
multi-region dashboards
advanced anomaly detection
central incident management
```

These are not Alpha blockers.

---

# 30. Release Gate

Observability readiness is complete when all of the following pass:

```text
[ ] kill API task → Swarm replacement + alert behavior verified
[ ] block public API → external probe generates P0
[ ] force PostgreSQL unavailable → readiness fails + alert generated
[ ] stop DataNode → heartbeat/unavailable alert generated
[ ] inject repeated backup failure → P1 generated
[ ] inject usage flush failure → no immediate alert, then alert after sustained threshold
[ ] simulate failover failure → P0 generated
[ ] fill test disk beyond warning threshold → host alert generated
[ ] deploy new version → deployment event visible
[ ] rollback → rollback event visible
[ ] request_id from Console error can find API/DataNode logs
[ ] API key/password/session/voucher/SQL params/KV values absent from logs
[ ] recovery notification emitted after incident clears
```

---

# 31. Recommended First Alert Table

| Signal | Threshold | Severity | Source |
|---|---|---:|---|
| Public `/ready` unavailable | 3 consecutive probes | P0 | External probe |
| API 5xx spike | >=10/5m and >=5% | P1 | Logs / metrics |
| PostgreSQL unavailable | >30s | P0/P1 | Logs + readiness |
| All DataNodes unavailable | immediate confirmed state | P0 | Application |
| DataNode heartbeat lost | failure timeout exceeded | P1 | Application |
| Disk usage | >70% / 85% / 95% | P2/P1/P0 | Host monitor |
| Memory usage | >80% / 90% / 97% | P2/P1/P0 | Host monitor |
| Backup failed | single / 3 consecutive | P2/P1 | Logs |
| Replica lag | >30s / >5m | P2/P1 | App metric/log |
| Failover failed | any confirmed failure | P0 | Logs |
| Usage flush stalled | >5m | P1 | Logs/metric |
| Settlement lag | >10m | P1 | Logs/metric |
| Object storage unavailable | sustained | P1 | Logs |
| Request latency | sustained regression | P2/P1 | Metrics |

Thresholds are initial operating values, not permanent product guarantees. Tune them using real Alpha traffic.

---

# 32. Final Operating Model

The Alpha observability stack should remain intentionally small:

```text
Structured Logs
      │
      ▼
Managed Log Service
      │

Host Metrics ───────────┐
                        │
External Probes ────────┼──► Alert Rules ───► Operator
                        │
Application Metrics ────┘
     (P1)
```

The system is ready when a user can report:

```text
"Combee failed. Request ID: req_..."
```

and the operator can quickly determine:

```text
what failed
where it failed
when it started
which version is running
whether data durability is affected
whether rollback/recovery is required
```

That is the Cloud Alpha observability standard.

---

# 33. Non-Goals for Alpha

Do not block Alpha on:

```text
self-hosted ELK
self-hosted Loki
self-hosted Prometheus/Grafana
full distributed tracing
service mesh telemetry
enterprise incident platform
complex anomaly detection
multi-region SLO framework
```

Add these only when operational evidence justifies the complexity.
