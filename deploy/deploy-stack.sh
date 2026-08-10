#!/usr/bin/env bash
# Combee Swarm 一键部署:source .env → stack deploy → 验证控制面 token。
#
# 用法(服务器 /opt/combee/deploy):
#   git pull && bash deploy-stack.sh
#
# 为什么需要 source .env:
#   旧版 docker 的 `docker stack deploy` 不会自动读取 .env 文件,
#   导致 ${VAR:-} 展开为空 —— api-server 容器缺 COMBEE_CONTROL_PLANE_TOKEN
#   时,它调 data-node RPC 不带 token → 全链路 401。
#   (曾发生:register/RPC 401 排查两天,根因就是部署时 .env 没加载。)
set -euo pipefail
cd "$(dirname "$0")"

echo "==> 1/4 加载 .env"
if [ ! -f .env ]; then
  echo "缺少 .env,从 .env.example 复制并填写: cp .env.example .env" >&2
  exit 1
fi
set -a && . ./.env && set +a

echo "==> 2/4 校验控制面 token 非空"
: "${COMBEE_CONTROL_PLANE_TOKEN:?COMBEE_CONTROL_PLANE_TOKEN 未在 .env 配置,拒绝部署}"

echo "==> 3/4 部署 stack"
docker stack deploy -c docker-stack.yml combee

echo "==> 4/4 等待并验证 api-server 容器 token"
sleep 20
CID=$(docker ps -q -f name=combee_api-server.1 | head -1)
if [ -n "$CID" ]; then
  TOK=$(docker exec "$CID" printenv COMBEE_CONTROL_PLANE_TOKEN 2>/dev/null || true)
  if [ -n "$TOK" ]; then
    echo "OK api-server token: ${TOK:0:8}…"
  else
    echo "!! api-server 容器 token 为空 —— 部署失败或 .env 未生效,重跑本脚本" >&2
    exit 1
  fi
else
  echo "!! 未找到 api-server 容器(可能仍在滚动),稍后手动验证" >&2
fi

echo "==> 完成。检查:"
echo "    docker service logs combee_api-server --since=1m | grep -c no_token   # 应为 0"
echo "    docker service logs combee_data-node --since=1m | grep -c unauthorized # 应为 0"
