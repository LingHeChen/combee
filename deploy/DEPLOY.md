# Combee Cloud 云服务器部署指南

目标:在一台云服务器上部署完整服务(landing / console / docs / api / data-node / postgres / caddy;对象存储用腾讯云 COS),
全部镜像已推送到腾讯云容器镜像服务,服务器**无需构建**,拉镜像即可运行。

## 0. 前置条件

| 项目 | 要求 |
|---|---|
| 服务器 | Linux x86_64(amd64),Ubuntu 22.04+ 或 Debian 12;建议 2C4G+、系统盘 50G+ |
| 域名 | `combee.cloud`(及 3 个子域)的 DNS 控制权 |
| 镜像 | 已推送 `ccr.ccs.tencentyun.com/combee/{landing,console,docs,api-server,data-node}:latest` |

## 1. 服务器环境

```bash
# 安装 Docker(官方脚本;国内服务器可换用镜像源,见下方备注)
curl -fsSL https://get.docker.com | sh
systemctl enable --now docker

# 安装 compose 插件
apt-get update && apt-get install -y docker-compose-plugin
docker compose version
```

> **国内服务器拉镜像**:腾讯云 CVM 拉 `ccr.ccs.tencentyun.com` 镜像无需加速(同域内网快);
> 首次拉 postgres/caddy 等 Docker Hub 镜像慢的话,给 Docker 配镜像加速
> (腾讯云加速器或 `registry.cn-hangzhou.aliyuncs.com` 等),写入 `/etc/docker/daemon.json` 后 `systemctl restart docker`。

## 2. 安全组 / 防火墙

放行 **80/tcp、443/tcp、443/udp**(Caddy 自动签 HTTPS,HTTP 会自动跳 HTTPS)。
其余端口无需对外开放(全部服务走 compose 内网)。

## 3. 域名 DNS

把以下记录指向服务器公网 IP(A 记录;有 IPv6 可加 AAAA):

```text
combee.cloud         A   <服务器公网IP>
docs.combee.cloud    A   <服务器公网IP>
api.combee.cloud     A   <服务器公网IP>
console.combee.cloud A   <服务器公网IP>
```

## 4. 获取配置

服务器上准备 `deploy/` 目录(从 git 拉取或 scp 上传),然后配置环境变量:

```bash
cd deploy
cp .env.example .env
vim .env   # 必须替换所有 CHANGE_ME_*:PG/MinIO 密码、control/admin 令牌
```

`.env` 关键项说明(与 compose/Caddyfile 同一套变量):

- `COMBEE_ACME_EMAIL`:证书过期提醒邮箱。
- `POSTGRES_PASSWORD` / `MINIO_ROOT_PASSWORD`:强密码。
- `COMBEE_CONTROL_PLANE_TOKEN` / `COMBEE_ADMIN_TOKEN`:强随机,三套互不相同。
- `NEXT_PUBLIC_COMBEE_API_URL` / `NEXT_PUBLIC_CONSOLE_URL` 等:`NEXT_PUBLIC_*` 是**构建期**变量,
  使用已推送镜像时它们已按默认(`api.combee.cloud` / `console.combee.cloud`)内联,无需(也不能)在服务器改;
  如要改域名,需要在开发机重新构建并推送镜像。

## 5. 启动

```bash
cd deploy
# 拉取所有已推送镜像并启动(compose 里 image + build 共存:服务器只拉镜像,不构建)
docker compose -f docker-compose.cloud.yml pull
docker compose -f docker-compose.cloud.yml up -d

# 查看状态
docker compose -f docker-compose.cloud.yml ps
docker compose -f docker-compose.cloud.yml logs -f caddy
```

首次启动 Caddy 会自动申请 4 个域名的 HTTPS 证书(约几十秒到几分钟,取决于 DNS 生效)。

## 6. 首次初始化(重要)

### 6.1 回填 Console 的 Session Cell

Console 用户账号存在一个 Session Cell 里;首次启动后 web 会自动创建,把 id 回填进 `.env`,
防止以后容器重建导致账号"消失":

```bash
docker compose -f docker-compose.cloud.yml exec web cat .bff-cell-id
# 输出形如 6aa8ec20-31da-47ba-974a-de4399bd19ca,填入 .env 的 COMBEE_BFF_CELL
docker compose -f docker-compose.cloud.yml up -d web   # 应用新配置
```

### 6.2 签发 API key

```bash
# 方式一:在 Console 页面(console.combee.cloud)用管理员/邀请码注册后签发
# 方式二:通过 admin 接口(需 COMBEE_ADMIN_TOKEN)或预置 COMBEE_API_KEYS
```

`.env` 中 `COMBEE_CONSOLE_SIGNUP` 默认 `code`(邀请制):
- 生成邀请码(voucher)后用户在 console 注册;或临时改 `open` 开放注册(测试期)。

## 7. 验证

```bash
curl -I https://combee.cloud          # landing,302/200 + TLS 正常
curl -I https://console.combee.cloud  # console
curl -I https://docs.combee.cloud     # docs
curl -I https://api.combee.cloud/openapi.json   # API
```

浏览器访问 https://console.combee.cloud 注册/登录。

## 8. 备份与更新

**数据都在命名卷里**(`pg-data`、`data-node-data`、`api-data`、`caddy-data`);备份数据在腾讯云 COS,不会因容器重建丢失:

```bash
# 备份卷(示例:pg 数据)
docker run --rm -v combee-cloud_pg-data:/data -v $PWD:/backup alpine tar czf /backup/pg-data.tar.gz -C /data .

# 更新镜像并滚动重启
docker compose -f docker-compose.cloud.yml pull
docker compose -f docker-compose.cloud.yml up -d
```

> 提示:更新前先 `docker compose -f docker-compose.cloud.yml exec api-server ...`
> 或直接 `docker compose ... exec data-node` 触发一次备份;备份也可用 API `POST /v1/databases/{id}/backup`。

## 9. 常见问题

| 现象 | 处理 |
|---|---|
| 证书未签发 | 确认 DNS 生效、80/443 放行;`docker compose logs -f caddy` 看错误 |
| Console 登录报账号不存在 | 检查 `.env` 的 `COMBEE_BFF_CELL` 是否回填(见 6.1) |
| API 401 | 确认请求带有效 `x-api-key`;`COMBEE_AUTH=key` 是强制校验 |
| 磁盘不足 | 数据卷会增长;监控磁盘,定期备份后清理无用卷/镜像 |

## 附:服务器直接构建(不用已推送镜像)

如果要在服务器上自己构建(不推荐,耗时且需要 npm/crates 网络):

```bash
docker compose -f docker-compose.cloud.yml up -d --build
```

国内服务器构建需保证能访问 npm registry 与 crates.io(可配置代理/镜像)。
`deploy/docker-compose.cloud.yml` 中每个服务同时声明了 `image:`(优先用)与 `build:`(备选),互不冲突。
