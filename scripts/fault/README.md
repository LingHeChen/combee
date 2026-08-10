# Fault Injection(故障注入)

按 `artifacts/engineering/COMBEE_STABILITY_ROADMAP.md` §9 实现,用于验证:

| 故障 | 脚本 | 验证点 |
|---|---|---|
| 进程崩溃 | `kill-node.sh data-node` | Swarm 自愈重启、数据一致性 |
| 网络隔离 | `network-isolate.sh data-node on/off` | 心跳超时 → 路由更新 → 恢复自动回归 |
| 磁盘满 | `disk-full.sh 95` / `disk-full.sh clean` | 写拒绝、P0 磁盘告警、清理恢复 |

统一流程:注入 → 观察日志/告警 → 验证数据 → 清理。
建议在 staging 或低峰期执行;data-node 是单点有状态,先确认对象存储备份完好。
