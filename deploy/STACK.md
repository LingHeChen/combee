# Combee Cloud —— Docker Swarm 部署(滚动更新)

`deploy/docker-stack.yml` 是 Swarm 版本:前端(landing/docs/web)多副本 + `start-first` 滚动更新(先起新再停旧,**真无损**);api-server 2 副本 + `start-first`(⚠️ registry 边界见文末);data-node / postgres 单副本 + `stop-first`(数据安全,秒级窗口)。

> 为什么后端不 start-first:api-server 的 NodeRegistry 在进程内存(data-node 只注册到其中一个实例),data-node 是 SQLite 单点有状态——多副本并存会路由错乱或同卷双写。这两处要等应用层改造(registry 落 PG、multi-node)后才能真正无损。

## 1. 服务器初始化

```bash
# 服务器只需:Swarm 模式 + 配置目录
docker swarm init                 # 单机即 manager+worker
mkdir -p /opt/combee && cd /opt/combee

# 上传 deploy/ 目录(Caddyfile、docker-stack.yml、.env)
# .env 必须填好:CHANGE_ME_*、COS 凭据、COMBEE_ADMIN_API_KEY 等(见 .env.example)
```

## 2. 首次部署

```bash
cd /opt/combee/deploy
docker stack deploy -c docker-stack.yml combee

# 查看
docker stack services combee
docker service ps combee_web --no-trunc
```

Caddy 自动签 4 个域名的 HTTPS 证书(需 DNS 已解析、80/443 放行)。

## 3. 滚动更新(核心流程)

**新镜像推送到腾讯云后:**

```bash
cd /opt/combee/deploy

# 1. 让 Swarm 拿到新镜像(重要:Swarm 默认本地有镜像就不拉)
docker pull ccr.ccs.tencentyun.com/combee/web:latest
docker pull ccr.ccs.tencentyun.com/combee/api-server:latest
# ... 按需 pull 更新的服务镜像

# 2. 重新部署,按 update_config 滚动
docker stack deploy -c docker-stack.yml combee

# 3. 观察滚动过程
watch docker service ps combee_web --no-trunc    # web: 2 副本逐个替换,先起新再停旧
docker service ps combee_api-server --no-trunc   # 后端: 先停旧再起新(秒级窗口)
```

滚动规则(`deploy.update_config`):
- 前端: `order: start-first`(新实例健康后停旧)→ 请求不中断
- 后端: `order: stop-first`(先停再起)→ 秒级中断,数据安全
- `parallelism: 1` 逐个替换;`monitor: 10s` 健康观察;`failure_action: rollback` 失败自动回滚

## 4. 回滚

```bash
# 单服务回滚到上一个版本
docker service rollback combee_web

# 全部回滚:重新部署旧 tag(镜像保留历史 tag 时)
```

## 5. 运维

```bash
docker service ls                # 所有服务
docker service logs combee_caddy # 日志
docker stack ps combee           # 任务历史
docker stack rm combee           # 移除整个栈(卷保留,数据不丢)
```

## 与 compose 版的差异

| 项目 | compose(cloud) | Swarm(stack) |
|---|---|---|
| 构建 | image + build 共存 | **仅 image**(stack 不支持 build,必须用已推送镜像) |
| Caddyfile | bind mount `./Caddyfile` | Swarm **config** 注入 |
| 前端副本 | 1 | 2(start-first 滚动) |
| 后端 | 单实例手动重启 | 单实例 + 自动滚动/回滚 |
| 端口 | 短语法 | `published/target` 长语法 |
| 部署 | `docker compose up -d` | `docker stack deploy` |

> 域名布局、环境变量、COS、admin API key 等与 compose 版完全一致(共用 `deploy/.env` 与 `deploy/Caddyfile`)。

## ✅ api-server 2 副本已落地(shared authority + eventual cache)

NodeRegistry 已落 PostgreSQL:

- `data_nodes` 表是**权威**:register / heartbeat / unregister 写 PG(低频,直接落库);
- 每个 API 副本本地持 **TTL 缓存(3s)**;`cell → node` 路由缓存 **5s**;
- 任意 API 副本从同一 authority 拿节点/路由 → 多副本一致,无需等 data-node 重新注册;
- 实测:api-server 重启 1s 内即可路由;第二个独立实例(从未收到注册)也能路由 cell。
- 暂不做 PG LISTEN/NOTIFY(Alpha 不需要),TTL 兜底足够。

**error-triggered invalidate + 安全重试(已落地)**:
- 绑定 Cell 的客户端在 **RPC 失败(节点不可达/响应损坏)时立即失效该 Cell 路由缓存**;
- 下一次请求直接从 authority 重新解析,失败收敛从 5s TTL 变为"首次失败即恢复";
- 解析层(请求未发出)失败会失效 + 重试一次,安全;写请求不自动重试(避免重复写入)。
- 实测:data-node 宕机 → 请求 500 并触发 invalidate → 重启 data-node → 立即恢复 200。

**仍为已知边界**:
- data-node 单点有状态,更新仍是 stop-first(等 multi-node 后解决)。