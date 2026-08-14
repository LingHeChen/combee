# Combee Cloud SLO(Cloud Alpha)

> 状态:Cloud Alpha 对外收真实用户前的承诺基线。
> 原则(见 `COMBEE_CLOUD_HARDENING.md` §9 可靠性天花板):**诚实优先,不过度承诺**。
> 本模型是"能容忍少量数据新鲜度损失"(单 Cell 写串行 + 单副本 + WAL 增量归档),不是多副本强一致。

---

## 承诺指标

| 指标 | 目标 | 测量方式 | 备注 |
|---|---|---|---|
| **可用性** | **99.9%**(月) | 外部探针 `GET /ready`(每 5 分钟)成功率 | 不含计划内维护;不含域名/备案等基础设施事件 |
| **Backup RPO** | **≤ 30s**(典型 ≤ 15s) | WAL 增量归档周期(`COMBEE_WAL_BACKUP_INTERVAL_SECS=15`) | 极端情况(归档间隔中崩溃)≤ 2×周期 |
| **Restore RTO** | **≤ 30 分钟**(单 Cell,数据量 < 50GB) | 删卷恢复演练实测 | 依赖 COS 带宽;更大 Cell 线性放宽 |
| **Failover RTO**(有副本时) | **≤ 60s** | 心跳超时(10s)+ 扫描周期(30s)+ 副本追平 | 无副本时不可自动 failover,如实告知 |

## 明确不做(刻意推迟,见 Hardening §9/§10)

- 多副本(>1)强一致:Non-Goal before Alpha;
- capacity-aware scheduler、跨区/多活:Non-Goal before Alpha;
- 计划内维护窗口的零停机:不承诺。

## 故障语义(对用户诚实)

| 事件 | 用户可见行为 |
|---|---|
| 写失败(磁盘满/节点不可用/超时) | **明确报错**(4xx/5xx),绝不静默成功、绝不假装落盘 |
| 主节点崩溃、无副本 | 请求返回明确错误(如 503/500),等待人工或自动恢复;无静默数据丢失 |
| 主节点崩溃、有副本 | 自动 failover,写请求在新主上继续;最近 ≤ RPO 的写可能丢失(如实告知) |
| Cell 完整性校验失败 | Cell 进入只读保护,写被拒、读可用,立即告警 |

## 验证方式(发布前必须完成)

1. `./scripts/release-test.sh` 全绿(含删卷恢复 / ENOSPC / kill-9);
2. 每季度一次删卷恢复演练,记录实测 RPO/RTO 并回填本表;
3. 12h soak(`scripts/soak-test.sh 720`)无内存单调增长、无延迟恶化;
4. 故障注入演练:`scripts/fault/kill-node.sh`、`scripts/fault/disk-full.sh`、`scripts/fault/network-isolate.sh`。
