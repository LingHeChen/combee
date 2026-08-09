#!/usr/bin/env bash
# 生成 Closed Alpha 邀请码(voucher,默认 1000 Alpha Credits)。
# 用法:
#   COMBEE_ADMIN_TOKEN=<token> COMBEE_API_URL=http://127.0.0.1:8080 ./scripts/generate-invites.sh [count] [amount_units] [campaign]
# 默认:count=1,amount_units=1_000_000_000(1000 Credits),campaign=closed-alpha

set -euo pipefail

API="${COMBEE_API_URL:-http://127.0.0.1:8080}"
TOKEN="${COMBEE_ADMIN_TOKEN:-}"
COUNT="${1:-1}"
AMOUNT="${2:-1000000000}"
CAMPAIGN="${3:-closed-alpha}"

if [ -z "$TOKEN" ]; then
  echo "ERROR: COMBEE_ADMIN_TOKEN is required" >&2
  exit 1
fi

echo "Generating $COUNT invite code(s) (+$AMOUNT microcredits each, campaign=$CAMPAIGN)…"
curl -s -X POST "$API/admin/vouchers/generate" \
  -H "content-type: application/json" \
  -H "x-admin-token: $TOKEN" \
  -d "{\"amount_units\":$AMOUNT,\"count\":$COUNT,\"campaign\":\"$CAMPAIGN\"}" \
  | python3 -c '
import sys, json
data = json.load(sys.stdin)
for c in data.get("codes", []):
    print(f"{c[\"code\"]}  (+{c[\"amount_units\"]} microcredits = {c[\"amount_units\"]/1_000_000:.1f} Credits)")
' || { echo "ERROR: failed (check COMBEE_API_URL / COMBEE_ADMIN_TOKEN)" >&2; exit 1; }

echo
echo "用户注册时把这个码填到 'Alpha access code' 即可(注册后自动获得对应 Credits)。"
