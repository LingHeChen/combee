# Changelog

All notable changes to Combee are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [v0.1.0-alpha.1] — 2026-08-08

首个 Public Alpha 预发布。功能冻结:V0 只修 bug / 补测试 / 安全加固,不再新增产品能力。

### Added

- **Cell 生命周期**:`POST/GET/DELETE /v1/databases`;懒创建(目录记录零 IO,首次访问才落盘 SQLite)。
- **SQL**:单条执行 + 多语句原子事务(`/transaction`);参数绑定;SQL 执行超时中断
  (`COMBEE_SQL_TIMEOUT_SECS`,默认 30s);`__sys_*` 内部表 / `BEGIN`/`COMMIT`/`ATTACH` 等
  事务控制与附加语句、多语句注入、`VACUUM INTO` 文件逃逸全部拦截。
- **KV**:GET / SET / DEL / EXISTS / MGET / MSET / TTL / EXPIRE / INCR;
  TTL 惰性过期 + 后台 GC;共享 moka 缓存(`COMBEE_KV_CACHE_CAPACITY`,默认 100k),
  read-through fill + write-update/invalidate,SQLite 始终权威;hit 无锁快路径。
- **持久化强度**:`COMBEE_KV_DURABILITY` fast / normal / strict。
- **Active DB Manager**:最多 `COMBEE_MAX_ACTIVE_DBS`(默认 100)个并发 SQLite 连接,
  LRU 逐出 + 空闲休眠(checkpoint + close);所有阻塞 SQLite 操作在 `spawn_blocking`。
- **多租户**:`tenants` / `api_keys`(仅存 sha256 哈希,`cmb_sk_` 前缀)/ `databases.tenant_id`;
  `COMBEE_AUTH=key` 强制 `x-api-key` 校验;隔离在 repository 层强制,跨租户一律 404;
  `POST/GET/DELETE /v1/api-keys`、`POST/GET /v1/tenants`。
- **Control plane auth**:`COMBEE_CONTROL_PLANE_TOKEN` 保护 `/internal/nodes/*` 与
  data-node `/rpc/*`;租户 `x-api-key` 永远不能调用内部接口。
- **备份/恢复(对象存储)**:一致性快照(`VACUUM INTO`)+ WAL 增量归档(`snapshot + wal` 对)→
  S3/MinIO(`COMBEE_S3_*`);restore 优先增量、回退全量,支持指定版本;自动周期归档
  (`COMBEE_WAL_BACKUP_INTERVAL_SECS`);本地 fs 后端供测试。
- **单 replica + 自动 failover**:复制通道复用 WAL 归档(`COMBEE_REPLICA_INTERVAL_SECS`);
  主节点心跳超时自动提升副本(或手动 `POST /v1/databases/:id/failover`),
  generation fencing 防脑裂(旧主复活写被拒)。
- **多节点**:Data Node agent 注册 + 心跳(`COMBEE_API_SERVER_URL` /
  `COMBEE_NODE_ADVERTISE_URL`),round-robin placement,按 Cell 路由到对应节点;
  NodeId 持久化(重启身份不变)。
- **元数据后端**:InMemory(默认)/ PostgreSQL(`COMBEE_METADATA=postgres`)。
- **Benchmark**:`cargo run --release -p combee-benchmark`(性能 / mixed / contention / capacity /
  e2e),输出 `capacity.csv|md`、`contention.csv|md`。
- **仓库配套**:`LICENSE`(Apache-2.0)、`CHANGELOG.md`、`SECURITY.md`、`CONTRIBUTING.md`。

### Performance(对照设计文档 §22,Apple Silicon 本机)

- KV hot GET p50/p99 ≈ 10µs / 35µs(目标 <1ms / <5ms);
- KV fast SET p99 ≈ 63µs、strict SET p99 ≈ 125µs(目标 <5ms / <20ms);
- Simple SQL p99 ≈ 41µs(目标 <20ms);
- 创建 20,000 个逻辑 Cell ≈ 15ms(零 IO);活跃连接数严格 ≤ 上限;
- 4 CPU + 8GiB 容器:1M logical Cells × 5k active,p99 ≈ 64µs,缓存命中率 100%。

### Security

- API key 只存哈希;跨租户访问 404(不泄露 Cell 存在性);
- SQL 注入面:多语句 / `__sys` 表 / `ATTACH` / `VACUUM INTO` / `load_extension` 拦截;
- Control-plane 令牌;租户 key 与内部接口完全隔离;
- Release Gate 审计:BLOCKER=0 / HIGH=0,详见 `docs/RELEASE_READINESS.md`。

### Known Limitations

- 单 Cell 写串行(per-db 锁);读可并行(单 Cell ~800 万 ops/s);
- 无资源配额(max KV value / max SQL 结果 / 并发上限);
- 默认元数据 in-memory,生产用 postgres;failover 依赖对象存储;
- V0 明确不做:RESP / PG wire / Blob / 多副本 / 复杂 scheduler。

### License

Apache-2.0。见 [`LICENSE`](LICENSE)。
