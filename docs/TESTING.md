# Combee 测试指南

本文件逐一说明每个测试的**目的**(测什么)与**预期结果**(验证什么行为),
便于按主题定位覆盖范围、理解行为契约,以及评估改动后的影响面。

## 概览

```text
单元测试(内联在各 crate 源码中)
├── combee-common   ids / protocol / config(durability) … 13
├── combee-metadata InMemoryStore 目录语义 ……………………… 5
└── combee-data-node
    ├── kv          所有 KV 命令边界 ………………………… 18
    ├── sql         语句拦截 / 参数映射 / 事务 ……………… 10
    ├── storage     分桶 / schema / WAL / durability …… 8
    ├── ttl         过期判定 / GC ……………………………… 5
    ├── manager     并发 / LRU / 休眠 / 持久化 ……………… 6
    └── cache       共享缓存 + 缓存一致性 …………………… 13

集成测试(tests/ 目录,完整 HTTP 栈)
├── tests/integration.rs  lifecycle / SQL / KV / TTL / auth … 14
├── tests/concurrency.rs   并发正确性 ………………………………… 3
├── tests/kv_edge.rs       KV 边界与错误路径 ……………………… 5
├── tests/rpc.rs          内部 RPC(RemoteDataNodeClient)…… 3
├── tests/multi_node.rs   多节点注册/placement/路由 ……… 3
├── tests/replication.rs   单 replica 复制 ……………………… 3
└── tests/failover.rs      failover + generation fencing …… 3
(backup.rs 内联)           备份/恢复(快照 + WAL 增量)…… 5
─────────────────────────────────────────────
合计 127 个测试
```

## 运行方式

```bash
cargo test --workspace      # 全部测试(100)
cargo test -p combee-data-node            # 仅 data-node 单元测试
cargo test -p combee --test integration   # 仅主集成测试
cargo test -p combee --test concurrency   # 仅并发测试
cargo test -p combee --test kv_edge       # 仅 KV 边界测试
cargo clippy --workspace --all-targets    # lint(0 警告)
cargo fmt --all -- --check                # 格式检查
cargo run --release -p combee-benchmark   # 性能基准(见文末)
```

集成测试使用 `tempfile` 临时数据目录,互不污染;`tests/common/mod.rs`
提供共享 helper(`test_app` / `send` / `create_db`)。

---

## 单元测试

### combee-common — ids.rs

| 测试 | 目的 | 预期结果 |
|---|---|---|
| `parse_and_display_roundtrip` | UUID 字符串与 `DatabaseId`/`TenantId` 的解析与输出 | 合法 UUID 解析成功,`to_string()` 还原原串 |
| `invalid_uuid_rejected` | 非法输入的处理 | `not-a-uuid`、空串、截断串解析报错 |
| `new_ids_are_unique` | `DatabaseId::new()` 生成唯一 ID | 两次生成的 ID 不同 |
| `serde_roundtrip_as_plain_string` | JSON 序列化形态 | 序列化为普通 UUID 字符串,反序列化还原 |

### combee-common — protocol.rs

| 测试 | 目的 | 预期结果 |
|---|---|---|
| `sql_request_defaults_to_empty_params` | `SqlRequest.params` 缺省值 | 缺省为空数组 |
| `sql_result_roundtrip` | `SqlResult` 序列化往返 | columns/rows/rows_affected 无损还原 |
| `kv_set_request_defaults` | `KvSetRequest` 缺省字段 | ttl_seconds=None、nx/xx=false |
| `kv_incr_request_defaults_delta_to_one` | INCR 请求缺省 delta | delta 缺省为 1(即 INCR),显式覆盖生效 |
| `kv_expire_request_without_ttl_means_persist` | EXPIRE 缺省 TTL 的含义 | 缺省 ttl_seconds = None = PERSIST |
| `kv_keys_request_rejects_missing_field` | 必填字段缺失 | 缺少 `keys` 反序列化失败 |
| `kv_entry_serializes_ttl_null_as_absent_or_null` | `KvEntry.ttl_seconds` 序列化 | None 序列化为 null 且可还原 |

### combee-common — config.rs

| 测试 | 目的 | 预期结果 |
|---|---|---|
| `durability_parse_and_display` | `KvDurability` 解析 | fast/normal/strict(含 wal/full 别名、大小写)解析正确;非法值报错;默认 Fast |
| `env_parse_falls_back_to_default` | 环境变量缺省回退 | 未设置的变量回退到默认值(不 set_var,避免 edition 2024 unsafe 与并行竞争) |

### combee-metadata — store.rs

| 测试 | 目的 | 预期结果 |
|---|---|---|
| `create_get_list_delete_roundtrip` | 目录基本流 | 创建后状态为 `created`,可查询/列出,删除后列表为空 |
| `duplicate_create_rejected` | 同租户重复创建 | 返回 `DatabaseAlreadyExists` |
| `get_and_delete_missing_rejected` | 对不存在记录的访问 | 均返回 `DatabaseNotFound` |
| `tenants_are_isolated` | 租户隔离 | B 租户看不到 A 的库;同 id 在不同租户可共存;删除互不影响 |
| `list_sorted_by_created_at_then_id` | 列表顺序确定性 | 按 `(created_at, id)` 排序,同秒创建时按 id 升序,无随机顺序 |

### combee-data-node — kv.rs(基于 `__sys_kv` 表)

| 测试 | 目的 | 预期结果 |
|---|---|---|
| `set_get_roundtrip` | 基本写读 | 写入值可读回,持久 key 无 ttl |
| `set_overwrites_existing` | SET 覆盖语义 | 同 key 二次 SET 覆盖旧值 |
| `empty_key_rejected` | 空 key 防御 | SET/INCR 空 key 返回 `InvalidRequest` |
| `get_missing_returns_none` | 缺失 key | 返回 None |
| `get_expired_invisible_and_deleted_lazily` | lazy expiration | 过期 key 不可见,且底层行被顺手删除 |
| `get_non_utf8_value_rejected` | BLOB 值防护 | 非 UTF-8 存储值读取返回 `InvalidRequest` |
| `set_nx_and_xx` | 条件写入 | NX 存在时不覆盖;XX 不存在时不写入 |
| `del_semantics` | 删除语义 | 删除成功返回 true,重复删除返回 false |
| `exists_ignores_expired` | EXISTS 与过期 | 过期 key 视为不存在 |
| `mget_mset_preserve_order` | 批量读写顺序 | 结果与请求 keys 顺序一一对应,含 TTL 项 |
| `ttl_semantics` | TTL 三态 | 缺失=None;持久=-1;带 TTL=剩余秒数(1..=100);过期=None |
| `expire_and_persist` | EXPIRE/PERSIST | 可设置/清除 TTL;对缺失 key 返回 false |
| `incr_semantics` | INCR/DECR | 从 0 开始;正负 delta 正确;负数即 DECR |
| `incr_keeps_original_ttl` | INCR 保留 TTL(Redis 语义) | 不带 TTL 的 INCR 不改变原过期时间 |
| `incr_sets_ttl_when_provided` | INCR 携带 TTL | 首次 INCR 带 TTL 后剩余秒数递减 |
| `incr_on_non_integer_errors` | 类型防御 | 对非整数值 INCR 返回 `InvalidRequest` |
| `incr_overflow_errors` | 溢出防御 | `i64::MAX + 1` 返回 `InvalidRequest`(checked_add) |
| `incr_on_expired_resets` | 过期 key 的 INCR | 过期视为不存在,重置为 delta |

### combee-data-node — sql.rs

| 测试 | 目的 | 预期结果 |
|---|---|---|
| `check_statement_rejects_dangerous_sql` | 语句白/黑名单 | `__sys*` 表访问 403;BEGIN/COMMIT/ATTACH 等 400;多语句 400;正常语句(含结尾分号、注释、引号内分号)放行 |
| `multi_statement_detector_edge_cases` | 多语句扫描器边界 | 引号/注释/转义引号中的分号不误判;真实多语句均检出 |
| `param_types_map_to_sqlite_values` | 参数类型映射 | null→NULL;true/false→1/0;浮点→REAL;字符串→TEXT |
| `unsupported_param_types_rejected` | 参数类型防御 | object/array 参数返回 `InvalidRequest` |
| `parameter_count_mismatch_rejected` | 占位符数量校验 | 参数数量与 `?` 不匹配时报错 |
| `multi_statement_injection_rejected` | 注入防护 | `SELECT 1; DROP TABLE t` 报错,表未被删除 |
| `select_returns_columns_and_rows` | 查询结果形态 | 返回 columns 与 rows,rows_affected=0 |
| `transaction_commits_and_rolls_back` | 事务原子性 | 全部成功提交;任一失败整体回滚 |
| `empty_transaction_rejected` | 空事务防御 | 空 statements 返回 `InvalidRequest` |
| `transaction_rejects_sys_table_access` | 事务内访问控制 | 事务中访问 `__sys_kv` 返回 403 |

### combee-data-node — storage.rs

| 测试 | 目的 | 预期结果 |
|---|---|---|
| `db_path_uses_hex_bucket` | 分桶布局 | 路径为 `<data>/<uuid前两位>/<uuid>.sqlite` |
| `db_path_is_stable_and_unique_per_db` | 路径确定性 | 同 db 路径稳定;不同 db 路径不同 |
| `open_initializes_internal_schema_and_wal` | 连接初始化 | `__sys_kv`/`__sys_meta` 建好;journal_mode=wal |
| `open_is_idempotent_across_connections` | schema 幂等 | 重开连接数据仍在,schema_version 唯一 |
| `checkpoint_flushes_wal_into_main_db` | WAL 合并 | TRUNCATE checkpoint 后 WAL 为空,主库可读回数据 |
| `remove_files_cleans_all_suffixes` | 文件清理 | 主库/-wal/-shm 全部删除 |
| `remove_files_skips_missing_files` | 懒删除容错 | 文件不存在时静默跳过(懒创建未触发的 db) |
| `open_applies_durability_pragma` | durability → synchronous 映射 | fast→OFF(0)、normal→NORMAL(1)、strict→FULL(2) |

### combee-data-node — ttl.rs

| 测试 | 目的 | 预期结果 |
|---|---|---|
| `is_expired_boundaries` | 过期判定边界 | `expires_at == now` 视为过期;持久 key 永不过期 |
| `expires_at_from_and_remaining` | 时间换算 | 相对 TTL→绝对时间;剩余秒数计算;已过期钳制为 0 |
| `unix_now_is_reasonable` | 时钟合理性 | 当前 unix 秒在 2020–2100 区间 |
| `gc_removes_only_expired` | 后台 GC 语义 | 只删过期 key,保留未过期与持久 key |
| `gc_respects_limit` | GC 批量上限 | 单次最多删 limit 条,下一轮可继续 |

### combee-data-node — manager.rs(Active DB Manager)

| 测试 | 目的 | 预期结果 |
|---|---|---|
| `concurrent_incr_same_db_is_atomic` | 同 Cell 串行化 | 50 并发 INCR 后值恰好为 50(无丢失更新) |
| `different_dbs_do_not_interfere` | 跨 Cell 并行 | 两 db 并行写同名 key 互不干扰,各读回各值 |
| `lru_evicts_oldest_when_at_capacity` | LRU 上限 | 容量 1 时新 db 逐出旧 db;被逐出后重开数据仍在 |
| `idle_timeout_sleeps_connections` | 空闲休眠/唤醒 | 空闲超时后连接被回收(active_count=0),再次访问自动激活 |
| `delete_database_removes_connection_and_files` | 删除清理 | 连接数归零、磁盘文件移除 |
| `data_persists_across_manager_restart` | 重启持久化 | 新 manager 实例打开同一目录,KV 与 SQL 数据均读回 |

### combee-data-node — cache.rs(全局共享 KV 缓存)

**纯缓存层(3)**

| 测试 | 目的 | 预期结果 |
|---|---|---|
| `hit_miss_and_lazy_expiry` | 命中/未命中/惰性过期 | 空缓存 miss;带 TTL 条目 hit 且剩余秒数正确;过期条目立即失效并计入 miss;持久 key 永不过期;hits/misses 计数准确 |
| `key_scoped_to_database` | 缓存键隔离 | db A 的条目对 db B 不可见 |
| `invalidate_removes_entry` | 失效 | invalidate 后同一 key 变 miss |

**DataNode 层:缓存一致性(10)**

| 测试 | 目的 | 预期结果 |
|---|---|---|
| `set_then_get_is_cache_hit` | 写更新(write-update) | set 后 get 为缓存 hit(hits=1,misses=0) |
| `miss_fills_cache_then_hits` | 读填充(read-through) | 冷启动首次 get 为 miss(从 SQLite 读回),第二次为 hit |
| `overwrite_and_delete_invalidate_cache` | 覆盖/删除一致性 | set 新值后读到新值;del 后读到 None |
| `incr_and_expire_stay_consistent_with_cache` | INCR/EXPIRE 一致性 | INCR 后缓存反映新值且 TTL 保留;EXPIRE 后 TTL 更新 |
| `expired_cache_entry_falls_back_to_sqlite` | 缓存过期回退 | TTL=0 写入后 get 恒为 None(缓存过期 → SQLite 兜底) |
| `ttl_seconds_decrease_across_cache_hits` | TTL 基于绝对时间 | 连续两次缓存 hit 的剩余秒数递减(不冻结) |
| `cache_isolation_between_databases` | 跨 db 缓存隔离 | 两 db 同 key 各读各值,均为各自 hit |
| `eviction_does_not_break_correctness` | 驱逐正确性 | 容量 2 下写 4 个 key,被逐出条目从 SQLite 读回,值全对 |
| `cache_survives_shutdown_but_data_reads_from_sqlite` | 缓存仅内存 | 重启后缓存冷(0 hit 1 miss),数据从 SQLite 读回 |
| `delete_database_clears_its_cache_entries` | 删库清缓存 | delete_database 后该 db 缓存条目清零 |

---

## 集成测试(完整 HTTP 栈)

### tests/integration.rs

| 测试 | 目的 | 预期结果 |
|---|---|---|
| `database_lifecycle` | 生命周期 | create 201 → list 包含 → sql 可用 → delete 204 → 再访问 404 → 重复删除 404 |
| `sql_basic_and_params` | SQL 基本 + 参数绑定 | 建表/插入(rows_affected=1)/带条件查询结果正确;语法错误 400 |
| `sql_value_types_roundtrip` | JSON 参数类型往返 | null/bool/float/字符串经 HTTP 正确映射;`true` 返回为 1;float 参与比较 |
| `sql_rows_affected_on_update` | UPDATE 统计 | 更新 3 行时 rows_affected=3 |
| `sql_transaction_with_select` | 事务内查询 | 事务中 CREATE+INSERT+SELECT 均返回,SELECT 结果正确 |
| `sql_transaction_atomicity` | 事务原子性(HTTP 层) | 全成功提交;第二条失败时第一条也回滚(COUNT=2) |
| `sql_transaction_requires_statements` | 空事务防御 | 空 statements 返回 400 |
| `kv_basic` | KV 基本读写 | SET→GET;缺失 key;批量 EXISTS;MGET/MSET;DEL 两次(第二 false) |
| `kv_ttl_and_expire` | TTL 全链路 | 写入 TTL→剩余秒数;PERSIST→-1;重新 EXPIRE;TTL=0 立即不可见;不存在 key EXPIRE→false |
| `kv_incr_and_nx_xx` | INCR/NX/XX(HTTP 层) | INCR 从 0、增量、DECR;非整数 400;NX 不覆盖;XX 需存在 |
| `lazy_create_creates_file_on_first_access` | 懒创建 | CREATE 后目录为空;首次 KV 写入后恰好出现 1 个 sqlite 文件 |
| `active_connection_limit_with_lru_eviction` | 连接上限(HTTP 层) | 3 个 db 并发上限 2,active_count 恒 ≤2;被逐出 db 重开正常 |
| `auth_requires_api_key` | API key 认证 | 无/错 key 401;任一合法 key 放行 |
| `forbidden_statements_are_rejected` | 访问控制(HTTP 层) | `__sys_kv` 403;BEGIN/COMMIT/ROLLBACK 400;非法 UUID 400 |

### tests/concurrency.rs

| 测试 | 目的 | 预期结果 |
|---|---|---|
| `concurrent_incr_is_atomic_over_http` | 同 key 并发 INCR | 20 并发后值恰好为 20 |
| `concurrent_set_same_key_last_writer_wins` | 并发 SET | 最终值为某个完整写入值,无撕裂、无错误 |
| `concurrent_writes_to_different_dbs_do_not_interfere` | 跨 db 并发隔离 | 10 个 db 并行建表/写入/查询,各自读到自己的数据 |

### tests/kv_edge.rs

| 测试 | 目的 | 预期结果 |
|---|---|---|
| `empty_key_rejected` | 空 key 防御(HTTP 层) | INCR/MSET 空 key 均 400 |
| `missing_database_returns_404_for_kv` | 幽灵 db 隔离 | 对不存在 db 的 GET/PUT/DELETE/INCR 全部 404 |
| `unicode_and_large_values_roundtrip` | 编码与体积 | 中文/emoji key 与 value 往返一致;100KB value 无损 |
| `reserved_endpoint_words_can_be_used_as_keys` | 保留名冲突 | `exists`/`mget`/`ttl`/`expire`/`incr` 均可作普通 key 读写(操作端点位于 `/kv/ops/*`) |
| `incr_with_ttl_over_http` | INCR 带 TTL(HTTP 层) | 首次 INCR 带 TTL 后 GET 返回剩余秒数 |

---

## 测试驱动出的产品修复

在补充测试的过程中发现并修复了 4 处真实问题:

1. **后台 TTL GC 语法错误**(`crates/data-node/src/ttl.rs`):
   bundled SQLite 未开启 `SQLITE_ENABLE_UPDATE_DELETE_LIMIT`,`DELETE ... LIMIT` 报错;
   改为子查询挑选待删行,并新增 `gc_removes_only_expired` / `gc_respects_limit` 回归测试。

2. **多语句注入被静默忽略**(`crates/data-node/src/sql.rs`):
   `rusqlite::prepare` 只编译第一条语句,`SELECT 1; DROP TABLE t` 中的第二条被悄悄丢弃。
   新增引号/注释感知的分号扫描器,显式拒绝多语句(允许结尾分号与分号后注释),
   并新增 `multi_statement_injection_rejected` / `multi_statement_detector_edge_cases` 回归测试。

3. **保留名 key 与操作端点冲突 405**(`crates/api-server/src/app.rs`):
   静态路由 `/kv/exists` 等优先级高于 `/kv/{key}`,同名 key 的 GET/PUT 会返回 405。
   操作端点统一移至 `/kv/ops/*`,任意 key 名均可读写;
   新增 `reserved_endpoint_words_can_be_used_as_keys` 回归测试。

4. **列表排序不确定性**(`crates/metadata/src/store.rs`):
   同秒创建的记录 `created_at` 相同,排序退化为 HashMap 迭代顺序。
   排序键改为 `(created_at, id)`,并新增 `list_sorted_by_created_at_then_id` 回归测试。

---

## 已知缺口与后续方向

以下模块尚未实现,因此没有对应测试(实现时需补齐):

- quota / rate limit(设计文档 V0 Platform 项,尚未实现);
- graceful shutdown 端到端验证(当前仅靠启动冒烟人工确认,可改为 process-level 测试);
- 独立 Data Node 进程 / gRPC 客户端(`GrpcDataNodeClient` 未实现);
- cache 的 write-back(异步落盘)模式尚未实现 —— 当前写路径先落 SQLite 再更新缓存,
  若未来为 Fast SET 引入纯内存 ACK + 后台 flush,需要新增"崩溃后缓存与磁盘一致"的测试;
- 无锁快路径的一致性依赖"写 ACK 前缓存已更新"的次序,若未来引入多副本/多 Data Node,
  需要重新论证跨节点线性化(当前单节点成立)。

说明:`PostgresStore`(`crates/metadata/src/postgres.rs`,SQLx)需要真实 PostgreSQL,
不纳入 `cargo test`(无内嵌数据库);已在 Docker 中做过端到端验证
(API Server + PostgreSQL 创建/查询 1M+ 条记录)与 capacity benchmark(`--metadata postgres`)。

---

## Benchmark(性能基准)

`crates/benchmark` 直接调用 DataNode(不含 HTTP 与公网 RTT),对照设计文档 §22 目标;
`--e2e` 模式走完整链路(client → HTTP → API Server → RPC → Data Node):

```bash
cargo run --release -p combee-benchmark            # 默认性能基准(含 mixed workload)
cargo run --release -p combee-benchmark -- --mixed # 仅 cache miss 梯度 + mixed workload
cargo run --release -p combee-benchmark -- --contention # 热点 Cell 并发瓶颈
cargo run --release -p combee-benchmark -- --capacity                      # 容量基准(默认 10k/100k/1M × 32/100/500/1k/5k)
cargo run --release -p combee-benchmark -- --capacity --metadata postgres --total 1M --active 32,500,5000
cargo run --release -p combee-benchmark -- --e2e --url http://127.0.0.1:8080    # 端到端
```

| 场景 | 实测(Apple Silicon 本机) | 设计目标(§22) |
|---|---:|---:|
| KV hot GET p50 / p99 | 10.1µs / 34.6µs | < 1ms / < 5ms |
| KV cold GET p99 | 46.8µs | < 20ms |
| Fast SET p99(无 fsync) | 62.7µs | < 5ms |
| Normal SET p99(WAL fsync) | 72.4µs | — |
| Strict SET p99(FULL fsync) | 125.2µs | < 20ms |
| Simple SQL p99 | 40.9µs | < 20ms |
| 20,000 逻辑 Cell 创建 | ~15ms(零 IO) | 一台机器承载海量 Cell |
| 随机访问 200 db 后活跃连接 | 32(上限 32) | 100000 db ≠ 100000 连接 |

### cache miss 梯度(4 CPU + 8GiB 容器,冷读 = 每个只读一次的 key)

| miss 比例 | p50 | p95 | p99 | hit rate(实测) |
|---|---:|---:|---:|---:|
| 0%(全热读) | 25.1µs | 42.9µs | 64.7µs | 100.0% |
| 25% | 26.6µs | 45.0µs | 71.3µs | 75.0% |
| 50% | 28.4µs | 46.1µs | 76.9µs | 50.0% |
| 75% | 29.5µs | 47.3µs | 74.0µs | 25.0% |
| 100%(全冷读) | 30.5µs | 50.3µs | 80.8µs | 0.0% |

结论:即使 100% 缓存 miss(全部读 SQLite),p99 仍 < 100µs —— 缓存让热读
快 ~2.5×,但 SQLite 兜底路径本身也远优于 §22 的 20ms 目标。

### mixed workload(60% 热读 / 20% 写 / 10% 冷读 / 10% 过期读)

p50 30.7µs / p95 48.5µs / p99 75.6µs,实测 hit rate 74.7%(与 60% 热读 + 40% miss 的构成一致)。

### contention(4 CPU + 8GiB 容器,1 个热点 Cell)

| operation | concurrency | throughput (ops/s) | p99 (µs) | lock avg (µs) | queue max |
|---|---|---:|---:|---:|---:|
| GET (cache hit) | 1 | 27,922 | 70.0 | 0.04 | 1 |
| GET (cache hit) | 8 | 27,805 | 446.3 | 251.8 | 8 |
| GET (cache hit) | 32 | 26,703 | 1,650.9 | 1,160.6 | 32 |
| GET (cache hit) | 128 | 28,294 | 5,668.3 | 4,482.1 | 128 |
| GET (cache hit) | 512 | 28,212 | 22,521.8 | 18,008.5 | 512 |
| SET (sqlite write) | 1 | 20,880 | 93.4 | 0.05 | 1 |
| SET (sqlite write) | 512 | 21,261 | 31,122.6 | 23,874.1 | 512 |
| mixed SQL/KV | 1 | 25,030 | 78.0 | 0.04 | 1 |
| mixed SQL/KV | 512 | 24,571 | 30,716.6 | 20,658.4 | 512 |

**结论(优化前)**:per-db 串行化把单 Cell 吞吐钉成常数(GET ~27k、SET ~21k、mixed ~24k
ops/s,与并发度无关),p99 随并发线性上升,queue max = 并发数(512 并发全部在锁上排队)。
对**写**(SET/SQL)这是必要且健康的;但对**缓存命中的 GET** 是**不必要的瓶颈**。

**已实施优化:缓存命中无锁快路径**(`crates/data-node/src/lib.rs`):
GET/MGET/TTL/EXISTS 命中缓存时直接返回(纯内存,不经过 per-db 锁与 `spawn_blocking`),
miss 才进锁读 SQLite 并填充;写操作保持锁内串行。一致性论证:缓存条目是不可变已提交
快照,写 ACK 前缓存必已更新(read-your-writes),并发读-写可线性化
(读要么先于写提交读到旧值,要么后于写提交读到新值)。回归测试:
`cache_hits_take_no_per_db_lock`(命中不产生锁样本)、
`concurrent_reads_and_writes_stay_linearizable`(并发读-写值恒为已提交值)。

**优化后(4 CPU + 8GiB 容器,同参数)**:

| operation | concurrency | throughput (ops/s) | p99 (µs) | lock avg (µs) | queue max |
|---|---|---:|---:|---:|---:|
| GET (cache hit) | 1 | 4,955,376 | 1.21 | 0.00 | 0 |
| GET (cache hit) | 32 | 8,106,070 | 5.50 | 0.00 | 0 |
| GET (cache hit) | 512 | 7,903,501 | 1.29 | 0.00 | 0 |
| SET (sqlite write) | 1 | 20,811 | 105.67 | 0.04 | 1 |
| SET (sqlite write) | 512 | 21,506 | 27,089 | 23,600 | 512 |
| mixed SQL/KV | 1 | 45,812 | 82.88 | 0.04 | 1 |
| mixed SQL/KV | 512 | 45,432 | 24,813 | 22,421 | 512 |

热点 Cell 的**读**瓶颈消除(GET 吞吐提升 ~290×,p99 从 22.5ms 降至 ≤5.5µs,锁统计全零);
**写**路径保持串行(必要,单 Cell ~21k 写 ops/s 稳定)。

### end-to-end(三容器:API Server + PostgreSQL + Data Node,4+8 client 容器)

| operation | concurrency | throughput (ops/s) | p50 (µs) | p95 (µs) | p99 (µs) |
|---|---|---:|---:|---:|---:|
| GET (cache hit) | 1 | 4,877 | 195 | 268 | 347 |
| GET (cache hit) | 8 | 22,789 | 334 | 468 | 583 |
| GET (cache hit) | 32 | 37,715 | 827 | 1,157 | 1,449 |
| SET | 1 | 3,783 | 252 | 363 | 582 |
| SET | 8 | 10,935 | 666 | 877 | 3,765 |
| SET | 32 | 10,958 | 2,691 | 5,649 | 6,415 |
| SQL SELECT | 1 | 4,011 | 242 | 359 | 437 |
| SQL SELECT | 8 | 14,807 | 531 | 704 | 811 |
| SQL SELECT | 32 | 14,921 | 2,122 | 2,444 | 3,949 |

端到端链路含 **HTTP(1 跳)+ 内部 RPC(1 跳)+ JSON 序列化**,单请求 p50 ≈ 200µs
(进程内直连 hot GET 为 ~1µs,网络与序列化占大头);并发 32 时 GET 吞吐 ~37k ops/s、
p99 1.45ms,写路径(SET/SQL)受 per-db 串行化影响 p99 4-6ms —— 均在设计目标内。
部署方式见 `docker-compose.yml`(`docker compose up -d --build`)。

### 多节点(2 × Data Node + API Server + PostgreSQL,docker)

- `NodeRegistry`:register / heartbeat / 健康过滤(10s 超时)/ round-robin placement / metrics(`GET /internal/nodes`);
- Data Node agent 自愈注册(启动顺序无关,注册失败自动重试);
- 实测:2 节点注册 healthy,创建 6 个 db 分布在两节点(PG 中 `storage_node_id` 3+3),Cell 请求按节点路由,跨节点数据隔离;
- e2e 回归(多节点栈):GET 并发 32 p99 2.83ms、SET p99 6.46ms、SQL p99 3.60ms(比单节点略高,含 metadata 路由一跳)。

### 自动 failover + generation fencing

- 流程:主节点心跳超时(10s)且有副本 → 副本追平(replicate)→ metadata 提升副本
  (`storage_node_id = 副本`、清 replica、`generation += 1`)→ fence 新主 → fence 旧主
  (`i64::MAX` 降级标记,任何正常写被拒 → 防脑裂);
- 写请求带 generation(Data Node `check_generation` 校验),不匹配返回 Forbidden;
- 自动扫描(`COMBEE_FAILOVER_INTERVAL_SECS`)或手动 `POST /v1/databases/:id/failover`;
- 测试(tests/failover.rs):fencing 拒绝旧 generation 写、failover 全链路(副本提升 +
  generation+1 + 旧主写被拒)、metadata promote 语义;
- MinIO 实测:停掉主节点容器 → 自动扫描触发 failover(PG 中 primary 切到副本、
  generation=1)→ 写走新主成功。

### 单 replica(复制)

- 复制通道 = WAL 增量归档:副本 Data Node 周期从对象存储拉取主节点的"主库 + WAL"应用到本地;
- `POST /v1/databases/:id/replication` 设置副本(metadata `replica_node_id`),
  副本节点周期 `GET /internal/nodes/{id}/replicas` 获取职责并拉取
  (`COMBEE_REPLICA_INTERVAL_SECS`);
- 测试(tests/replication.rs):主写+归档 → 副本拉取数据一致、多轮增量追赶、
  未归档写入不复制、replication API 设置/查询/取消、metadata 副本查询;
- MinIO 实测:主节点写 → 周期归档 → 副本拉取(每 3s)→ 主再写 → 副本 WAL 增长;
  主副本 WAL 文件 md5 完全一致(精确复制)。

### WAL 增量备份

- 每轮归档 = per-db 锁内拷贝"主库 + 当前 WAL"(与写串行,保证对齐),
  上传 `backups/{db}/incr/snapshot-{rev}.sqlite` + `wal-{rev}.sqlite-wal`;
  恢复 = 主库 + WAL 重放(SQLite 原生),RPO 缩短到归档间隔;
- 自动模式:`COMBEE_WAL_BACKUP_INTERVAL_SECS` 周期对活跃 Cell 归档;
  手动:`POST /v1/databases/:id/backup/incr`;
- 测试(backup.rs `incremental_tests`):多轮归档恢复点语义(归档后的写入不出现)、
  跨 checkpoint(实例重启)的多轮恢复、缺省恢复优先增量而非旧全量快照;
- MinIO 实测:周期归档自动产生多轮 `snapshot-*`/`wal-*` 对;炸毁节点后 restore
  恢复到最近归档点(kv=v2、表 3 行)—— 比"上次手动快照"更新,即 RPO 缩短生效。

### 备份 / 恢复(MinIO 端到端)

- `crates/data-node/src/backup.rs`:VACUUM INTO 一致性快照 → object_store 上传(S3/MinIO 或本地 fs);
  DataNode `backup()` / `restore()`(缺省最新或指定版本,关连接 + 清缓存 + 原子替换);
- 内联测试 `backup_then_restore_after_destroy`:备份 → 删除本地文件(模拟节点炸毁)→ 恢复 → KV 与 SQL 数据一致;
- docker 实测(MinIO):写数据 → backup(对象落 MinIO)→ 修改数据 → 删除 data-node 容器(数据全丢)→ 重建 → restore → 数据精确回到备份点;
- API:`POST /v1/databases/:id/backup`、`POST /v1/databases/:id/restore`(body `{"version": key}` 可选)。

### capacity(4 CPU + 8GiB 容器,1M total,`--metadata postgres`)

| total | active | RSS (MB) | fd | p99 (µs) | hit % | 备注 |
|---|---:|---:|---:|---:|---:|---|
| 1M | 32 | 24.9 | 108 | 64.2 | 100 | 1M 目录记录在 PostgreSQL,进程内存几乎为零 |
| 1M | 500 | 83.5 | 1512 | 62.4 | 100 | |
| 1M | 5,000 | 646.4 | 15012 | 64.0 | 100 | |

1M 条 metadata 记录批量创建(UNNEST INSERT)耗时 ~2.9s;对比 in-memory 后端
(1M 条记录占进程 ~470MB),PostgreSQL 后端把目录内存开销移出 Data Node 进程。

### tests/tenancy.rs —— 租户隔离与 API key 生命周期

| 测试 | 目的 | 预期结果 |
|---|---|---|
| `cross_tenant_access_is_rejected` | 租户 B 无法访问 A 的 Cell | sql / transaction / kv / delete / backup / restore 跨租户一律 404;A 数据完好 |
| `api_key_lifecycle_and_revocation` | key 明文仅返回一次;撤销即失效;跨租户不能撤销 | 创建 201 含明文(仅此一次);列表只存 64 位 sha256 哈希;A 撤销 B 的 key → 404;B 撤销后该 key 立即 401 |
| `tenant_a_data_invisible_to_tenant_b` | 无/错误 key 拒绝;资源对他人不可见 | 无 key/错 key → 401(不泄露 Cell 存在性) |
