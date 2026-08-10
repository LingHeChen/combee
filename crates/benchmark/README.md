# Benchmark(性能基准)

对照 `artifacts/engineering/COMBEE_STABILITY_ROADMAP.md` §8 的连续基准要求。

## 运行

```bash
# 开发机性能基准(直接调 DataNode,无网络开销)
cargo run --release -p combee-benchmark

# 热点 Cell 并发瓶颈(per-db 锁分析)
cargo run --release -p combee-benchmark -- --contention

# 端到端(HTTP → API Server → RPC → Data Node;需先起 api-server)
cargo run --release -p combee-benchmark -- --e2e --url http://127.0.0.1:8080

# 容量基准(逻辑 Cell 数 × 活跃连接)
cargo run --release -p combee-benchmark -- --capacity --metadata postgres --total 1M --active 32,500,5000
```

## 持续基准(roadmap 8.1)

固定工作负载 + 固定环境,每日/每周记录指标:

**小 Cell 集(建议):**
- 1000 Cells × 10MB,100 req/s,持续运行
- 观察:内存、文件数、FD 用量、p50/p99 延迟

**大 Cell 集(按需):**
- 1 Cell × 50GB,验证 backup / restore / migration

**记录方式**(示例):
```bash
# 每天同一时段跑一次,追加到结果文件
cargo run --release -p combee-benchmark -- --e2e --url http://127.0.0.1:8080 \
  >> /var/log/combee-bench-$(date +%Y%m%d).log 2>&1
```

> 注意:性能数字必须带环境条件(版本、节点规格、metadata 是否 PostgreSQL、活跃 Cell 数),
> 不要发布无条件的营销性数字。
