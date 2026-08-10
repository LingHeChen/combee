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

echo "==> 4/4 等待并验证所有 api-server 副本的 token"
# 注意:replicas=2 + start-first 滚动,旧副本会继续服务直到新副本就绪。
# 等待滚动完成:旧任务全部退出后,再验证每个运行中副本。
for i in $(seq 1 24); do
  OLD=$(docker service ps combee_api-server --no-trunc -f desired-state=Shutdown --format '{{.ID}}' 2>/dev/null | wc -l | tr -d ' ')
  NEW=$(docker service ps combee_api-server --no-trunc -f desired-state=Running --format '{{.ID}}' 2>/dev/null | wc -l | tr -d ' ')
  # 等 2 个副本都 Running 且没有新的 Shutdown 计数增长(简单起见:等 Running == replicas)
  REPLICAS=$(docker service inspect combee_api-server --format '{{.Spec.Mode.Replicated.Replicas}}' 2>/dev/null)
  if [ "${NEW:-0}" -ge "${REPLICAS:-2}" ]; then break; fi
  sleep 5
done
FAIL=0
for CID in $(docker ps -q -f name=combee_api-server); do
  TOK=$(docker exec "$CID" printenv COMBEE_CONTROL_PLANE_TOKEN 2>/dev/null || true)
  if [ -n "$TOK" ]; then
    echo "OK ${CID:0:12} token: ${TOK:0:8}…"
  else
    echo "!! ${CID:0:12} token 为空" >&2
    FAIL=1
  fi
done
[ "$FAIL" = "0" ] || { echo "存在副本 token 为空,检查 .env 并重跑" >&2; exit 1; }

echo "==> 完成。检查:"
echo "    docker service logs combee_api-server --since=1m | grep -c no_token   # 应为 0"
echo "    docker service logs combee_data-node --since=1m | grep -c unauthorized # 应为 0"
