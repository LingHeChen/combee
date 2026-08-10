# Combee Incident Runbooks(告警处置手册)

> 对齐 `artifacts/COMBEE_OBSERVABILITY_ALERTING_PLAN.md` §25。
> 原则:**stateless 故障交给 Swarm 自愈,stateful 故障不盲目重启**。

---

## R1. API 不可用(P0)

**检查顺序:**

```bash
# 1. 外部探针确认(是不是只有你本地访问不了)
curl -sI https://api.combee.cloud/ready

# 2. Swarm 服务状态
docker service ls | grep combee_api-server
docker service ps combee_api-server --no-trunc | head

# 3. 日志
docker service logs --since 10m combee_api-server | grep -E '"level":"(ERROR|WARN)"' | tail -20

# 4. 依赖
docker service ls | grep -E "postgres|data-node"     # PG / DataNode 是否活着
docker service logs --since 5m combee_postgres | tail
```

**处理:**

- 若 api-server 任务反复重启且日志是配置/启动错误 → 查最近部署(`service.started` 版本),必要时 **rollback 最新 api-server 部署**
- 若是 PG/DataNode 挂了 → 见 R2/R3
- stateless 任务重启:`docker service update --force combee_api-server`(可自愈,先观察 1 分钟)

---

## R2. PostgreSQL 不可用(P0/P1)

**检查:**

```bash
docker service ps combee_postgres | head
docker service logs --since 10m combee_postgres | tail -20
df -h /   && docker exec $(docker ps -qf name=postgres) sh -c 'df -h /var/lib/postgresql/data'
```

**常见原因与处理:**

| 原因 | 处理 |
|---|---|
| 磁盘满 | 清理 Docker 缓存(`docker system prune -f`)、扩容;不要删 pg 数据 |
| OOM / 内存 | 看宿主内存,必要时扩规格;恢复后 PG 自动回 |
| 连接耗尽 | 重启 PG 服务(数据在卷,安全) |
| 迁移失败 | 查 `relation ... already exists` 级别日志;回滚迁移 |

**红线:** 无备份不得删 PG 数据卷;恢复失败找备份(`pg-data` 卷 + COS)。

---

## R3. DataNode 不可用(P0/P1)

**检查:**

```bash
docker service ls | grep data-node
docker service logs --since 10m combee_data-node | tail -20
# 节点心跳(api 侧):
docker service logs --since 10m combee_api-server | grep -iE "heartbeat|register" | tail
# 本地磁盘:
df -h / && du -sh /opt/combee/deploy/../data 2>/dev/null || true
```

**处理:**

- **单副本 stop-first 更新/崩溃**:等 Swarm 拉起;node-id 在卷(`/data/node-id`)持久,恢复后路由自动回归(≤几秒)
- 若 node-id 漂移导致旧 cell `data node unavailable` → 按 `databases.storage_node_id` 更新为当前 node-id(有脚本历史,谨慎,遵守 generation/fencing)
- **不要**手动 promote 旧副本/改 generation,除非明确 failover 流程

---

## R4. 磁盘接近满(P1/P0)

**检查最大占用:**

```bash
df -h /
du -sh /var/lib/docker 2>/dev/null
docker system df
docker volume ls -qf dangling=true | wc -l   # 孤立卷
```

**安全清理顺序(由安全到危险):**

1. `docker builder prune -af`(构建缓存)
2. `docker system prune -f`(悬空镜像/停止容器,**不删卷**)
3. 清理被忽略的 `target/`、旧构建产物
4. 确认 COS 备份完好后,再考虑清理 cell/WAL 文件(谨慎,先查对象存储)

**红线:** 不直接删 `/var/lib/docker/volumes/*_data` 下的数据;扩容最稳。

---

## R5. 备份失败(P1)

**检查:**

```bash
docker service logs --since 1h combee_data-node | grep -iE "backup" | tail -20
# COS 连通性:
docker run --rm --network combee_default curlimages/curl -s -o /dev/null -w "%{http_code}\n" https://cos.ap-guangzhou.myqcloud.com
# 最近成功备份:
docker service logs --since 24h combee_data-node | grep "backup.*complete" | tail
```

**处理:**

- 单次失败 → 重试即可(P2 观察)
- 连续失败 → 检查 COS 凭据/权限/bucket、本地磁盘、data-node 状态
- 恢复期失败是 P0:先恢复,再补备份

---

## R6. 部署回归(P1)

**检查:**

```bash
# 当前版本(service.started 日志)
docker service logs combee_api-server | grep service.started | tail -1
# 错误/延迟开始时间与部署时间对齐
docker service ps combee_web --no-trunc | head
docker service logs --since 30m combee_api-server | grep -E '"level":"ERROR"' | head
```

**处理:**

- 若强相关(部署后立即出现错误码/延迟劣化):
  - `docker service rollback combee_api-server`
  - 或回退到上一个**不可变镜像 tag**(不要在 latest 上打补丁)
- 回滚后确认 `service.started` 版本恢复

---

## 通用原则

- **数据节点(PostgreSQL/DataNode)不盲目重启**:先看日志、磁盘、网络;无备份不删数据。
- **stateless 服务(api/web/landing/docs/caddy)**:Swarm 会自愈,先观察再手动干预。
- **每次 P0/P1 后**:记录时间线(部署/错误/恢复)、request_id 样本、后续动作。
