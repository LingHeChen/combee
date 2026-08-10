#!/usr/bin/env bash
# 故障注入:网络隔离指定节点(丢弃该容器所有入/出流量,模拟断网)。
# 验证:心跳超时 → 路由更新 / failover;恢复后自动回归。
#
# 用法:
#   scripts/fault/network-isolate.sh <service> on    # 断网
#   scripts/fault/network-isolate.sh <service> off   # 恢复
# 例:scripts/fault/network-isolate.sh data-node on
#
# 注意:容器内需要 iptables(swarm 节点为 root,可用)。隔离只影响容器网络,不影响 SSH。
set -euo pipefail

TARGET="${1:?usage: network-isolate.sh <service> on|off}"
ACTION="${2:?usage: network-isolate.sh <service> on|off}"

CIDS=$(docker ps -q --filter name="combee_${TARGET}" | tr '\n' ' ')
if [ -z "$CIDS" ]; then
  echo "!! 未找到 combee_${TARGET} 容器" >&2
  exit 1
fi

case "$ACTION" in
  on)
    echo "==> 注入:隔离 combee_${TARGET} 网络($CIDS)"
    for c in $CIDS; do
      pid=$(docker inspect -f '{{.State.Pid}}' "$c")
      # 进入容器网络命名空间,丢弃所有流量
      nsenter -t "$pid" -n iptables -I INPUT -j DROP
      nsenter -t "$pid" -n iptables -I OUTPUT -j DROP
      echo "    $c 已隔离(INPUT/OUTPUT DROP)"
    done
    echo "==> 观察(30s):心跳超时后 data_nodes 表应标记离线,路由应切走/报不可达"
    ;;
  off)
    echo "==> 恢复:combee_${TARGET} 网络($CIDS)"
    for c in $CIDS; do
      pid=$(docker inspect -f '{{.State.Pid}}' "$c")
      nsenter -t "$pid" -n iptables -D INPUT -j DROP || true
      nsenter -t "$pid" -n iptables -D OUTPUT -j DROP || true
      echo "    $c 已恢复"
    done
    echo "==> 观察:心跳恢复注册,路由自动回归(≤几秒)"
    ;;
  *)
    echo "usage: network-isolate.sh <service> on|off" >&2
    exit 1
    ;;
esac
