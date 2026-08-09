# Combee Cloud 部署(deploy/)

Caddy 反向代理 + Landing + Docs + 完整 Combee 数据栈。根域名 `combee.cloud`。

## 域名布局(全部由 `.env` 配置,见 `.env.example`)

| 变量 | 默认值 | 服务 | 来源 |
|---|---|---|---|
| `COMBEE_LANDING_DOMAIN` | `combee.cloud` | Landing 推广页 | `landing/`(`landing/Dockerfile`) |
| `COMBEE_DOCS_DOMAIN` | `docs.combee.cloud` | 文档站 | `site/`(`site/Dockerfile`) |
| `COMBEE_API_DOMAIN` | `api.combee.cloud` | Public API | `crates/`(根 `Dockerfile`) |
| `COMBEE_CONSOLE_DOMAIN` | `console.combee.cloud` | Cloud Console 主前端 | `web/`(`web/Dockerfile`) |

## 服务间路径:统一环境变量

**compose 与 Caddyfile 引用同一组变量**,每个地址只有一个定义处,避免改一处漏一处导致不可达:

| 路径 | 变量(定义处) | 谁在用 |
|---|---|---|
| 反代上游 | `COMBEE_UPSTREAM_LANDING/DOCS/API` | Caddyfile(`{$VAR}`)+ compose |
| 数据节点端口 | `COMBEE_DATA_NODE_PORT` | data-node 监听 + api-server 的 `COMBEE_DATA_NODE_URL` |
| PostgreSQL 连接串 | 由 `POSTGRES_USER/PASSWORD/DB` 拼出 | api-server |
| 对象存储端点 | `COMBEE_S3_ENDPOINT` | data-node + minio-init |
| MinIO 凭证/桶 | `MINIO_ROOT_USER/PASSWORD`、`MINIO_BUCKET` | minio + data-node + minio-init |

改动只需编辑 `.env`,重启 compose 即可生效(改 Caddyfile 引用变量则 `docker compose restart caddy`)。

## 启动

```bash
# 1. 环境变量(默认值仅适合本地;生产务必改)
cp deploy/.env.example .env

# 2. DNS:三个域名 A/AAAA 指向部署机,80/443 公网可达

# 3. 构建并启动
docker compose -f deploy/docker-compose.cloud.yml up -d --build

# 4. 查看
docker compose -f deploy/docker-compose.cloud.yml ps
docker compose -f deploy/docker-compose.cloud.yml logs -f caddy
```

Caddy 会自动申请并续期 HTTPS 证书(邮箱 `COMBEE_ACME_EMAIL`)。

## 本地测试(无域名)

```bash
# 1. 把 deploy/Caddyfile 每个域名块改为 `tls internal`(自签证书)
# 2. /etc/hosts 加:
#    127.0.0.1 combee.cloud docs.combee.cloud api.combee.cloud
# 3. 启动后浏览器访问 https://combee.cloud(忽略自签警告)
```

## Landing page

`landing/` 是 Next.js 静态导出应用,`landing/Dockerfile` 构建为 nginx 镜像。
更新方式:改 `landing/` 源码后 `docker compose -f deploy/docker-compose.cloud.yml up -d --build landing`。
Caddyfile 无需改动(上游由 `COMBEE_UPSTREAM_LANDING` 控制,默认 `landing:80`)。

## 开发版 compose

根目录 `docker-compose.yml` 与 deploy 版共用同一套变量(未设置时使用内置默认值),
本地开发无需 `.env` 也能直接 `docker compose up -d --build`(含 docs 与 landing)。

## 生产清单

- 所有密码/令牌走 `.env`,不要提交仓库。
- `COMBEE_AUTH=key` + `COMBEE_API_KEYS` 提供用户 API key。
- `COMBEE_METADATA=postgres`(已配置),生产不要用 in-memory。
- 备份:MinIO bucket 由 `minio-init` 自动创建;API: `POST /v1/databases/{id}/backup`。
