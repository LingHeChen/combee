#!/usr/bin/env bash
# 故障注入:模拟磁盘满(向 /tmp 填充到指定阈值)。
# 验证:写操作被拒绝(QuotaExceeded / database is full)+ 磁盘告警触发 + 清理后恢复。
#
# 用法:
#   scripts/fault/disk-full.sh <threshold_pct>   # 例:95(填充到 95%)
#   scripts/fault/disk-full.sh clean             # 清理填充文件
set -euo pipefail

FILL_DIR="${COMBEE_FILL_DIR:-/tmp/combee-fault}"
ACTION="${1:-95}"

case "$ACTION" in
  clean)
    echo "==> 清理填充文件"
    rm -rf "$FILL_DIR"
    df -h / | tail -1
    exit 0
    ;;
  *)
    THRESHOLD="$ACTION"
    ;;
esac

mkdir -p "$FILL_DIR"
TOTAL_KB=$(df -P / | awk 'NR==2 {print $2}')
USED_KB=$(df -P / | awk 'NR==2 {print $3}')
TARGET_KB=$(( TOTAL_KB * THRESHOLD / 100 ))
NEED_KB=$(( TARGET_KB - USED_KB ))

if [ "$NEED_KB" -le 0 ]; then
  echo "!! 磁盘已 ≥${THRESHOLD}%,无需填充"
  df -h / | tail -1
  exit 0
fi

echo "==> 磁盘: 总量 ${TOTAL_KB}KB, 已用 ${USED_KB}KB, 目标 ${THRESHOLD}%(${TARGET_KB}KB)"
echo "==> 填充 ${NEED_KB}KB 到 $FILL_DIR ..."
dd if=/dev/zero of="$FILL_DIR/fill.bin" bs=1M count=$((NEED_KB/1024 + 1)) status=none || true

echo ""
echo "==> 当前状态(应 ≥${THRESHOLD}%):"
df -h / | tail -1

echo ""
echo "==> 验证步骤(手动):"
echo "  1) 写入 KV/SQL → 应报错(写拒绝)"
echo "  2) 5 分钟内告警群应收到 P0 磁盘告警(≥92%)"
echo "  3) 清理:scripts/fault/disk-full.sh clean → 写入恢复"
