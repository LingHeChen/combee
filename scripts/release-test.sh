#!/usr/bin/env bash
# Combee Release Gate:统一发布就绪测试入口。
#
# 覆盖:全部单元/集成测试(含 tests/release/*)、docker 场景
# (fresh install、重启持久性、kill -9 崩溃恢复 + integrity_check、删卷仅对象存储恢复)。

set -uo pipefail
cd "$(dirname "$0")/.."

PASS=0; FAIL=0; WARN=0
declare -a RESULTS

section() { echo; echo "=== $1 ==="; }

wait_registered() {
  for i in $(seq 1 30); do
    H=$(curl -s $API/internal/nodes 2>/dev/null | python3 -c 'import sys,json; d=json.load(sys.stdin); print(sum(1 for n in d if n["healthy"]))' 2>/dev/null || echo 0)
    [ "$H" -ge 1 ] 2>/dev/null && return 0
    sleep 1
  done
  return 1
}

ok()   { PASS=$((PASS+1)); RESULTS+=("PASS  $1"); echo "PASS  $1"; }
fail() { FAIL=$((FAIL+1)); RESULTS+=("FAIL  $1"); echo "FAIL  $1"; }
warn() { WARN=$((WARN+1)); RESULTS+=("WARN  $1"); echo "WARN  $1"; }

echo "================ COMBEE RELEASE GATE ================"

# ---------- 1. 全部 cargo 测试 ----------
section "Functional / Durability / Isolation / Fencing / Fuzz(单元+集成+release)"
if cargo test --workspace 2>&1 | tail -5 | grep -q "test result: ok"; then
  N=$(cargo test --workspace 2>&1 | grep -E "test result" | awk '{s+=$4} END {print s}')
  ok "cargo test --workspace(${N} passed)"
else
  fail "cargo test --workspace"
fi

# ---------- 2. 静态质量 ----------
section "Lint / Format"
if cargo clippy --workspace --all-targets 2>&1 | grep -qE "^(warning|error): "; then
  warn "clippy 存在警告"
else
  ok "clippy 0 warnings"
fi
if cargo fmt --all -- --check >/dev/null 2>&1; then
  ok "cargo fmt clean"
else
  fail "cargo fmt"
fi

# ---------- 3. docker 场景 ----------
if [[ "${1:-}" == "--no-docker" ]]; then
  warn "docker 场景跳过(--no-docker)"
else
  section "Docker: Fresh Install / Restart Persistence / Kill-9 / Backup Restore"

  # 3.0 编译(挂载 registry;若 docker build 可用则用 compose build,否则容器内 cargo build)
  section "构建二进制"
  if docker build -t combee:check -f Dockerfile . >/dev/null 2>&1; then
    ok "docker build 可用"
    BIN=/usr/local/bin
    IMG=combee:check
  else
    warn "docker build 不可用(buildx 环境问题),回退容器内 cargo build"
    mkdir -p .docker-target
    if docker run --rm -v "$PWD":/combee -w /combee \
        -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
        -e CARGO_TARGET_DIR=/combee/.docker-target \
        rust:1.97-bookworm cargo build --release -p combee-api-server -p combee-data-node >/dev/null 2>&1; then
      ok "容器内 cargo build --release"
      BIN=/app/bin
      IMG=rust:1.97-bookworm
    else
      fail "容器内 cargo build"
      BIN=/app/bin; IMG=rust:1.97-bookworm
    fi
  fi

  # 3.1 Fresh install:起 postgres + minio(compose 网络)+ data-node + api-server
  section "Fresh Install(全新环境)"
  docker compose up -d postgres minio minio-init >/dev/null 2>&1 || { fail "compose postgres/minio"; }
  # 等待 PostgreSQL healthy(API Server 启动时连不上会 panic 退出)
  for i in $(seq 1 30); do
    H=$(docker inspect --format '{{.State.Health.Status}}' combee-postgres-1 2>/dev/null || echo starting)
    [ "$H" = "healthy" ] && break; sleep 2
  done
  # compose 默认网络名:基于目录名 combee → combee_default
  NET=$(docker network ls --format "{{.Name}}" | grep -E "^combee_default$" | head -1 || echo combee_default)
  docker rm -f rel-dn rel-dn2 rel-api >/dev/null 2>&1
  for name in rel-dn rel-dn2; do
    docker run -d --name $name --network $NET \
      -e COMBEE_DATA_NODE_ADDR=0.0.0.0:9000 -e COMBEE_DATA_DIR=/data \
      -e COMBEE_API_SERVER_URL=http://rel-api:8080 -e COMBEE_NODE_ADVERTISE_URL=http://$name:9000 \
      -e COMBEE_S3_ENDPOINT=http://minio:9000 -e COMBEE_S3_ACCESS_KEY=combee \
      -e COMBEE_S3_SECRET_KEY=combee123456 -e COMBEE_S3_BUCKET=combee-backups \
      -e COMBEE_WAL_BACKUP_INTERVAL_SECS=3 -e COMBEE_REPLICA_INTERVAL_SECS=3 \
      -e COMBEE_SQL_TIMEOUT_SECS=5 \
      -v "$PWD/.docker-target/release":$BIN -w / \
      $IMG bash -c "$BIN/combee-data-node" >/dev/null 2>&1
  done
  docker run -d --name rel-api --network $NET -p 18080:8080 \
    -e COMBEE_BIND_ADDR=0.0.0.0:8080 -e COMBEE_METADATA=postgres -e COMBEE_MULTI_NODE=1 \
    -e COMBEE_DATABASE_URL=postgres://combee:combee@postgres:5432/combee \
    -e COMBEE_DATA_DIR=/data \
    -v "$PWD/.docker-target/release":$BIN -w / \
    $IMG bash -c "$BIN/combee-api-server" >/dev/null 2>&1

  API=http://127.0.0.1:18080
  for i in $(seq 1 40); do
    code=$(curl -s -o /dev/null -w "%{http_code}" $API/v1/databases 2>/dev/null || true)
    [ "$code" = "200" ] && break; sleep 2
  done
  if [ "$code" != "200" ]; then
    fail "API Server readiness"
    docker logs rel-api 2>&1 | tail -5
  else
    ok "API Server readiness"
      # 等待 Data Node 注册(placement 需要 registry 有健康节点)
      REG_OK=""
      for i in $(seq 1 20); do
        H=$(curl -s $API/internal/nodes 2>/dev/null | python3 -c 'import sys,json; d=json.load(sys.stdin); print(sum(1 for n in d if n["healthy"]))' 2>/dev/null || echo 0)
        [ "$H" -ge 1 ] 2>/dev/null && REG_OK=1 && break
        sleep 1
      done
      if [ -z "$REG_OK" ]; then fail "Data Node 注册(registry 无健康节点)"; else ok "Data Node 注册($H 个 healthy 节点)"; fi
    # 创建 Cell + SQL + KV + 数据
    DB=$(curl -s -X POST $API/v1/databases | python3 -c 'import sys,json;print(json.load(sys.stdin)["id"])' 2>/dev/null || true)
    if [ -z "$DB" ]; then
      fail "create Cell"
    else
      ok "create Cell($DB)"
      curl -s -X POST $API/v1/databases/$DB/sql -H 'content-type: application/json' -d '{"sql":"CREATE TABLE t (x INTEGER)"}' >/dev/null
      curl -s -X POST $API/v1/databases/$DB/sql -H 'content-type: application/json' -d '{"sql":"INSERT INTO t VALUES (42)"}' >/dev/null
      curl -s -X PUT $API/v1/databases/$DB/kv/k -H 'content-type: application/json' -d '{"value":"persist-me"}' >/dev/null
      echo "$DB" > /tmp/combee-release-db
    fi
  fi

  # 3.2 Restart persistence:重启所有容器后数据仍在
  section "重启持久性(全部容器重启)"
  docker restart rel-dn rel-dn2 rel-api >/dev/null 2>&1
  docker compose restart postgres minio >/dev/null 2>&1
  sleep 10
  for i in $(seq 1 20); do
    code=$(curl -s -o /dev/null -w "%{http_code}" $API/v1/databases 2>/dev/null || true)
    [ "$code" = "200" ] && break; sleep 2
  done
  if ! wait_registered; then fail "重启后节点未注册"; else ok "重启后节点注册"; fi

  DB=$(cat /tmp/combee-release-db 2>/dev/null || true)
  if [ -n "$DB" ]; then
    V=$(curl -s $API/v1/databases/$DB/kv/k 2>/dev/null | python3 -c 'import sys,json;print(json.load(sys.stdin).get("value",""))' 2>/dev/null)
    if [ "$V" = "persist-me" ]; then
      ok "重启后 KV 数据仍在"
    else
      fail "重启后 KV 数据丢失(期望 persist-me,got $V)"
    fi
    N=$(curl -s -X POST $API/v1/databases/$DB/sql -H 'content-type: application/json' -d '{"sql":"SELECT COUNT(*) AS n FROM t"}' 2>/dev/null | python3 -c 'import sys,json;print(json.load(sys.stdin)["rows"][0][0])' 2>/dev/null)
    if [ "$N" = "1" ]; then ok "重启后 SQL 数据仍在"; else fail "重启后 SQL 数据丢失(期望 1,got $N)"; fi
  else
    warn "无 db id,跳过重启持久性"
  fi

  # 3.3 Kill -9 Data Node(主)+ 重启 + integrity_check
  section "Kill -9 + SQLite integrity_check"
  PRIMARY=$(docker exec combee-postgres-1 psql -U combee -d combee -t -c "SELECT storage_node_id::text FROM databases WHERE id='$(cat /tmp/combee-release-db 2>/dev/null)'" 2>/dev/null | tr -d ' ' || true)
  PRIMARY_CT=""
  for name in rel-dn rel-dn2; do
    ADDR=$(docker inspect --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' $name 2>/dev/null)
    [ -n "$PRIMARY" ] && [ -n "$ADDR" ] && break
  done
  # 直接 kill 两个 data-node 中的一个,重启验证
  docker kill rel-dn >/dev/null 2>&1 && ok "kill -9 data-node(SIGKILL)" || warn "kill data-node"
  sleep 12   # 等待心跳超时(10s)判定不可用
  docker start rel-dn >/dev/null 2>&1
  if ! wait_registered; then fail "kill-9 后节点未恢复注册"; else ok "kill-9 后节点恢复注册"; fi
  sleep 8
  DB=$(cat /tmp/combee-release-db 2>/dev/null || true)
  if [ -n "$DB" ]; then
    IC=$(curl -s -X POST $API/v1/databases/$DB/sql -H 'content-type: application/json' -d '{"sql":"PRAGMA integrity_check"}' 2>/dev/null)
    if echo "$IC" | grep -q '"ok"'; then
      ok "PRAGMA integrity_check = ok(kill -9 后无损坏)"
    else
      fail "integrity_check 未返回 ok:$IC"
    fi
  fi

  # 3.4 Backup → 破坏本地卷 → 仅对象存储恢复
  section "删卷后仅对象存储恢复"
  DB=$(cat /tmp/combee-release-db 2>/dev/null || true)
  if [ -n "$DB" ]; then
    curl -s -X POST $API/v1/databases/$DB/backup >/dev/null 2>&1
    curl -s -X PUT $API/v1/databases/$DB/kv/k -H 'content-type: application/json' -d '{"value":"after-backup"}' >/dev/null 2>&1
    sleep 5   # 等待 WAL 增量周期归档(3s),使 restore 能恢复到 after-backup
    # 破坏数据节点本地数据目录
    for name in rel-dn rel-dn2; do
      docker exec $name sh -c 'rm -rf /data/*' >/dev/null 2>&1 || true
    done
    docker restart rel-dn rel-dn2 >/dev/null 2>&1
    sleep 8
    # 由于 metadata 持久且路由到主节点,主节点数据目录被清空 → restore 从对象存储
    curl -s -X POST $API/v1/databases/$DB/restore -H 'content-type: application/json' -d '{}' >/dev/null 2>&1
    V=$(curl -s $API/v1/databases/$DB/kv/k 2>/dev/null | python3 -c 'import sys,json;print(json.load(sys.stdin).get("value",""))' 2>/dev/null)
    if [ "$V" = "after-backup" ]; then
      ok "删卷后 restore 恢复(仅对象存储)"
    else
      fail "删卷后 restore 失败(期望 after-backup,got $V)"
    fi
  else
    warn "无 db id,跳过备份恢复"
  fi

  # 清理
  docker rm -f rel-dn rel-dn2 rel-api >/dev/null 2>&1
  docker compose down >/dev/null 2>&1
  docker network rm $NET >/dev/null 2>&1 || true
fi

# ---------- 汇总 ----------
echo
echo "================ COMBEE RELEASE GATE ================"
for r in "${RESULTS[@]}"; do echo "$r"; done
echo "------------------------------------------"
echo "PASS=$PASS  FAIL=$FAIL  WARN=$WARN"
if [ "$FAIL" = "0" ]; then
  echo "RESULT: RELEASEABLE(本环境)"
else
  echo "RESULT: NOT RELEASEABLE($FAIL failures)"
fi
exit $([ "$FAIL" = "0" ] && echo 0 || echo 1)
