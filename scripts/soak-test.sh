#!/usr/bin/env bash
# 缩短版 Soak Test(计划 12h;本环境跑 ~15min 验证无资源泄漏趋势)。
#
# 持续混合 workload:Cell create/delete、SQL 读写、KV SET/GET/INCR、TTL 过期。
# 每 30s 采样 API Server 与 Data Node 容器内存(RSS),输出趋势;判断是否持续增长。

set -uo pipefail
cd "$(dirname "$0")/.."

DURATION_MIN=${1:-15}
ROUNDS=$((DURATION_MIN * 2))   # 每 30s 一轮

# ---- 起单节点栈 ----
docker compose up -d postgres minio minio-init >/dev/null 2>&1
sleep 8
docker rm -f soak-dn soak-api >/dev/null 2>&1
docker run -d --name soak-dn --network combee_default \
  -e COMBEE_DATA_NODE_ADDR=0.0.0.0:9000 -e COMBEE_DATA_DIR=/data \
  -e COMBEE_API_SERVER_URL=http://soak-api:8080 -e COMBEE_NODE_ADVERTISE_URL=http://soak-dn:9000 \
  -e COMBEE_S3_ENDPOINT=http://minio:9000 -e COMBEE_S3_ACCESS_KEY=combee \
  -e COMBEE_S3_SECRET_KEY=combee123456 -e COMBEE_S3_BUCKET=combee-backups \
  -e COMBEE_SQL_TIMEOUT_SECS=5 \
  -v "$PWD/.docker-target/release":/app/bin -w / \
  rust:1.97-bookworm bash -c "/app/bin/combee-data-node" >/dev/null 2>&1
docker run -d --name soak-api --network combee_default -p 18081:8080 \
  -e COMBEE_BIND_ADDR=0.0.0.0:8080 -e COMBEE_METADATA=postgres -e COMBEE_MULTI_NODE=1 \
  -e COMBEE_DATABASE_URL=postgres://combee:combee@postgres:5432/combee -e COMBEE_DATA_DIR=/data \
  -v "$PWD/.docker-target/release":/app/bin -w / \
  rust:1.97-bookworm bash -c "/app/bin/combee-api-server" >/dev/null 2>&1
sleep 10

API=http://127.0.0.1:18081
for i in $(seq 1 30); do
  code=$(curl -s -o /dev/null -w "%{http_code}" $API/v1/databases 2>/dev/null || true)
  [ "$code" = "200" ] && break; sleep 2
done

echo "soak start: ${DURATION_MIN}min, sample every 30s"
echo "time api_mem_MB dn_mem_MB p50_us p99_us cells"
> /tmp/soak-report.txt

for r in $(seq 1 $ROUNDS); do
  T0=$(date +%s)
  # ---- 混合 workload(5 个并发 worker,30s) ----
  for w in 1 2 3 4 5; do
    (
      for i in $(seq 1 30); do
        case $((RANDOM % 8)) in
          0) curl -s -X POST $API/v1/databases >/dev/null 2>&1 ;;
          1) curl -s -X DELETE $API/v1/databases/$(cat /tmp/soak-db 2>/dev/null) >/dev/null 2>&1 ;;
          2) curl -s -X POST $API/v1/databases/$(cat /tmp/soak-db 2>/dev/null)/sql -H 'content-type: application/json' -d '{"sql":"SELECT count(*) FROM t"}' >/dev/null 2>&1 ;;
          3) curl -s -X POST $API/v1/databases/$(cat /tmp/soak-db 2>/dev/null)/sql -H 'content-type: application/json' -d '{"sql":"INSERT INTO t (x) VALUES (random()%1000)"}' >/dev/null 2>&1 ;;
          4) curl -s -X PUT $API/v1/databases/$(cat /tmp/soak-db 2>/dev/null)/kv/k -H 'content-type: application/json' -d '{"value":"v"}' >/dev/null 2>&1 ;;
          5) curl -s $API/v1/databases/$(cat /tmp/soak-db 2>/dev/null)/kv/k >/dev/null 2>&1 ;;
          6) curl -s -X POST $API/v1/databases/$(cat /tmp/soak-db 2>/dev/null)/kv/ops/incr -H 'content-type: application/json' -d '{"key":"c","delta":1}' >/dev/null 2>&1 ;;
          7) curl -s -X PUT $API/v1/databases/$(cat /tmp/soak-db 2>/dev/null)/kv/exp -H 'content-type: application/json' -d '{"value":"e","ttl_seconds":1}' >/dev/null 2>&1 ;;
        esac
      done
    ) &
  done
  wait

  # 采样
  API_MEM=$(docker stats --no-stream --format "{{.MemUsage}}" soak-api 2>/dev/null | awk -F'/' '{print $1}' | grep -oE '^[0-9.]+' || echo 0)
  DN_MEM=$(docker stats --no-stream --format "{{.MemUsage}}" soak-dn 2>/dev/null | awk -F'/' '{print $1}' | grep -oE '^[0-9.]+' || echo 0)
  # 延迟采样:50 次 GET
  LAT=$(for i in $(seq 1 50); do
    s=$(curl -s -o /dev/null -w "%{time_total}" $API/v1/databases/$(cat /tmp/soak-db 2>/dev/null)/kv/k 2>/dev/null)
    echo "$s"; done | sort -n)
  P50=$(echo "$LAT" | awk 'NR==25{print $1*1000000}')
  P99=$(echo "$LAT" | awk 'NR==50{print $1*1000000}')
  CELLS=$(curl -s $API/v1/databases 2>/dev/null | python3 -c 'import sys,json;print(len(json.load(sys.stdin)))' 2>/dev/null || echo 0)
  echo "$r api=${API_MEM}MB dn=${DN_MEM}MB p50=${P50}us p99=${P99}us cells=$CELLS"
  echo "$r $API_MEM $DN_MEM $P50 $P99 $CELLS" >> /tmp/soak-report.txt
  # 若尚无 db,创建(soak-db)
  if [ ! -f /tmp/soak-db ]; then
    DB=$(curl -s -X POST $API/v1/databases | python3 -c 'import sys,json;print(json.load(sys.stdin)["id"])' 2>/dev/null)
    [ -n "$DB" ] && { echo "$DB" > /tmp/soak-db; curl -s -X POST $API/v1/databases/$DB/sql -H 'content-type: application/json' -d '{"sql":"CREATE TABLE t (x INTEGER)"}' >/dev/null 2>&1; }
  fi
  # 对齐 30s 周期
  ELAPSED=$(( $(date +%s) - T0 ))
  [ $ELAPSED -lt 30 ] && sleep $((30 - ELAPSED))
done

echo "=== soak report ==="
cat /tmp/soak-report.txt
docker rm -f soak-dn soak-api >/dev/null 2>&1
docker compose down >/dev/null 2>&1
echo "soak done"
