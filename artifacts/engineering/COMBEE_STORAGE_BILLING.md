# Combee 存储计费设计:GB·小时(GB·h)

> 状态:**已实现**(✅)。创建 2026-08-14。
> 目标:给"按需付费"补上唯一缺的、也是**第一位持续成本**的维度 —— 存储。
> 现状缺口:`settlement.rs` 里 `StorageBytes` 被 `continue` 跳过,**存储完全没有计费**。
> 一个用户写 10GB、之后零操作,现在一分钱不付,却占你磁盘 + 备份到永远。

---

## 0. 定价模型上下文(为什么是 GB·h)

整体计费形态(讨论结论):**预付 credits(充值 + 新用户赠送)+ 按量扣费 + 资源包/套餐包**。
按量扣费的**头部维度应是"存储 + 出口"**(这两个才是真实成本;ops 对我们近乎免费,给大额免费额度即可)。

存储用 **GB·h(十进制 GB=1e9)** 而非 GB·月:Cell 随时创建/删除,GB·h 粒度细、更公平,
且能吃到"冷/空 Cell 零计费"的架构优势。

---

## 1. 核心难点:storage 是 gauge,计费要时间积分

所有现有 metric 都是**加法计数器**(ops、bytes):`usage_add` 累加、settlement `sum × rate`。
但 **storage 是 gauge(某时刻的字节数)** —— 把不同分钟桶的 gauge 相加没有物理意义。

GB·h 的本质是**对字节数做时间积分**:

```
GB·h = ∫ bytes dt / (1e9 × 3600)
     ≈ Σ(采样字节 × 采样间隔秒) / (1e9 × 3600)      # 定间隔 Riemann 和
```

## 2. 关键技巧:在采样点就把 gauge 变成加法计数器

**不要存 gauge;在采样那一刻直接算出「字节·秒」增量,当成一个普通加法 metric 记进去。**
这样下游(flush 聚合 → settlement rating → 幂等账本 → `pricing.rate`)**一行都不用改**,全部复用。

新增计费 metric:`StorageByteSecs`(字节·秒,可加)。

- **采样器**(每 `SAMPLE_INTERVAL` 秒,默认 300s):对每个有数据的 Cell
  `bytes = storage_bytes(cell)` → `meter.record(tenant, cell, StorageByteSecs, bytes * SAMPLE_INTERVAL_SECS)`。
- **flush**:和别的计数器一样写进 `usage_buckets`,零改动。
- **settlement**:`StorageByteSecs` 是普通计数器,**现有 loop 自动计费**。
  只需**保留**对旧 `StorageBytes`(gauge)的 `continue` 跳过、**不要**跳过新 metric。settlement.rs 逻辑改动 ≈ 0。
- **幂等 / 重启容错**:白拿现有 `usage_add` 回收重试 + settlement `reference_id` 幂等。

> gauge → 积分的转换只发生一次(采样器里一次乘法)。这是本设计最值钱的地方:
> 用一个"派生加法 metric"把存储塞进你已有的整条计费流水线。

## 3. 计价配置(GB·h)

pricing 规则结构不变,填数即可:

```
metric      = StorageByteSecs
unit_size   = 3_600_000_000_000        # 1 GB·h = 1e9 bytes × 3600 s = 3.6e12 字节·秒
price_units = <每 GB·h 的 microcredits>
```

`pricing.rate` 的 `div_ceil` 会按 GB·h 向上取整。
示例:收 ¥0.01/GB·h、且约定 1 credit = ¥1 → `price_units = 10_000`(=0.01 credit)。
对外展示 **¥/GB·h**,内部照旧 microcredits,展示层翻译。

## 4. 必须定的边界

1. **采样哪些 Cell —— 含冷 Cell。**
   冷库照样占磁盘要收费,所以采样器遍历 **metadata 目录(`list_all_databases`)**,不是 LRU active 集。
   懒创建、无文件的 Cell → `storage::storage_bytes` 读文件返回 0 → `record` 对 `delta=0` 直接 return →
   **冷 / 空 Cell 天然零计费**,与架构一致。

2. **规模演进(重要)。**
   beta 单机 OK(in-proc `storage_bytes` 就是一次 file stat)。
   但 100 万 Cell 时"每 5 分钟 stat 一百万次"是负担 → 演进为**把采样下沉到 data-node**
   (它本地遍历自己的 data_dir、按 (tenant, cell) 汇总一次、批量上报),避免 N 次 RPC。
   **先做单机版,规模到了再换。**

3. **溢出边界。**
   字节·秒很大:100GB×300s=3e13;1TB 租户单次结算 ~3e14;i64 上限 9.2e18,**留 ~3 万倍余量**。
   结算窗口是分钟级,不会长期累积。确认 postgres `usage_buckets.value` 为 BIGINT。

4. **重启重复采样。**
   极端下同一分钟桶可能被采两次(多算一个采样间隔),金额极小、可接受。
   要精确就给采样器加 per-cell `last_sampled_bucket` 守卫。

5. **GB 口径。** 计费用**十进制 GB=1e9**(展示惯例),不用 GiB,免得用户算不清。

6. **采样间隔 vs 精度。** 300s 下,存活几分钟的短命 Cell 有 ±1 间隔误差;
   存储通常留存数小时/天,误差可忽略。要更细就降到 60s(采样成本换精度)。

## 5. 落地改动清单(小而集中)

| 文件 | 改动 |
|---|---|
| `common/src/usage.rs` | 加 `StorageByteSecs` 变体 + `as_str`/`parse`("storage_byte_secs") |
| `api-server/src/usage.rs` | 加 `spawn_storage_sampler`(遍历目录 → 记 byte·secs) |
| `api-server/src/main.rs`(或 app 装配处) | 起采样器任务 + 读 `COMBEE_STORAGE_SAMPLE_INTERVAL_SECS`(默认 300) |
| `settlement.rs` | 无需改逻辑(仅确认新 metric 不被 skip;旧 `StorageBytes` gauge 仍 skip) |
| pricing 初始化 / seed | 给 `StorageByteSecs` 配一条规则 |
| `handlers/usage.rs`(可选) | summary 加"本周期 storage GB·h",让用户看得见 |

> 保留旧 `StorageBytes`(gauge)**仅用于展示**("当前存储"),**不参与计费**;计费一律走 `StorageByteSecs`。

## 6. 测试计划

- **单元**:采样器把 `bytes × interval` 记成 byte·secs;`pricing.rate` 对 3.6e12 字节·秒 = 1 GB·h。
- **端到端**:写 X 字节 → 跑 N 个采样间隔 → flush → settlement →
  扣款 ≈ `(X/1e9) × (N×interval/3600) × 单价`(注意 div_ceil 向上取整)。
- **边界**:空/冷 Cell 不产生扣费;删除 Cell 后停止计费;跨 cell 汇总正确。
- **幂等**:重复 settle 同 bucket 不重复扣(复用现有 reference_id 机制)。

## 7. 与硬上限的关系

存储计费 ≠ 存储限额。二者并存:
- **计费**:GB·h 扣 credits(本文件)。
- **硬上限**:`COMBEE_STORAGE_HARD_BYTES` 防单租户塞满磁盘(见 CLOUD_HARDENING §P0/配额)。
- **余额耗尽**:soft limit 当前不断服;真正收钱前需 hard-limit / 降级路径(见 CLOUD_HARDENING §1 扣费策略讨论)。
