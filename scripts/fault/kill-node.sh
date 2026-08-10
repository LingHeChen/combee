#!/usr/bin/env bash
# 故障注入:kill -9 指定 Combee 服务(进程级故障,验证 Swarm 自愈 + 数据恢复)。
# 用法:
#   scripts/fault/kill-node.sh <service>   # 例:data-node / api-server / web
#   scripts/fault/kill-node.sh all         # 全部 combee 服务(最狠)
set -euo pipefail

STACK="${COMBEE_STACK:-combee}"
TARGET="${1:-data-node}"

echo "==> 注入:kill -9 $STACK/$TARGET"
if [ "$TARGET" = "all" ]; then
  docker ps -q --filter name="${STACK}_" | xargs -r docker kill -9
  echo "    已 kill 全部 ${STACK}_* 容器"
else
  docker ps -q --filter name="${STACK}_${TARGET}" | xargs -r docker kill -9
  echo "    已 kill ${STACK}_${TARGET}"
fi

echo "==> 观察 Swarm 自愈(10s 内应自动重启):"
sleep 10
docker service ls | grep "${STACK}_${TARGET}" || true

echo ""
echo "==> 验证步骤(手动):"
echo "  1) docker service ps ${STACK}_${TARGET} --no-trunc        # 重启记录"
echo "  2) docker service logs --since 2m ${STACK}_${TARGET}      # 启动日志(service.started)"
echo "  3) curl -s https://api.combee.cloud/ready                 # API 可用性"
echo "  4) 写入并读取一条数据,确认恢复后数据一致"
