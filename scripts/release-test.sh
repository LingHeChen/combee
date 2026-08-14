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
    H=$(curl -s -H "x-control-token: $GATE_TOKEN" $API/internal/nodes 2>/dev/null | python3 -c 'import sys,json; d=json.load(sys.stdin); print(sum(1 for n in d if n["healthy"]))' 2>/dev/null || echo 0)
    [ "$H" -ge 1 ] 2>/dev/null && return 0
    sleep 1
  done
  return 1
}

ok()   { PASS=$((PASS+1)); RESULTS+=("PASS  $1"); echo "PASS  $1"; }
fail() { FAIL=$((FAIL+1)); RESULTS+=("FAIL  $1"); echo "FAIL  $1"; }
warn() { WARN=$((WARN+1)); RESULTS+=("WARN  $1"); echo "WARN  $1"; }

# gate 专用凭据(仅本脚本使用;postgres 模式下必须 Key 认证 + control token)
GATE_KEY="cmb_sk_release_gate_0000000000000000"
GATE_TOKEN="release-gate-ctrl-token"
GATE_HASH=$(python3 -c 'import hashlib;print(hashlib.sha256(b"cmb_sk_release_gate_0000000000000000").hexdigest())')
KEYH=(-H "x-api-key: $GATE_KEY")

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

  # 3.1 Fresh install:独立网络 + 独立 postgres/minio(与宿主机在跑的 combee 全家桶完全隔离,
  # 避免 placement 选到外部 data-node 导致场景失真)+ data-node + api-server
  section "Fresh Install(全新环境)"
  NET=combee-gate-net
  docker network rm $NET >/dev/null 2>&1 || true
  docker network create $NET >/dev/null 2>&1
  # 独立 PostgreSQL(映射 55432 供宿主 cargo test 连接)
  docker rm -f g-postgres >/dev/null 2>&1
  docker run -d --name g-postgres --network $NET -p 127.0.0.1:55432:5432 \
    -e POSTGRES_USER=combee -e POSTGRES_PASSWORD=combee -e POSTGRES_DB=combee \
    postgres:17 >/dev/null 2>&1
  for i in $(seq 1 30); do
    H=$(docker exec g-postgres pg_isready -U combee 2>/dev/null && echo healthy || echo starting)
    [ "$H" = "healthy" ] && break; sleep 2
  done
  # 独立 MinIO(对象存储)
  docker rm -f g-minio >/dev/null 2>&1
  docker run -d --name g-minio --network $NET \
    -e MINIO_ROOT_USER=combee -e MINIO_ROOT_PASSWORD=combee-secret-123456 \
    minio/minio:latest server /data --console-address ":9001" >/dev/null 2>&1
  for i in $(seq 1 30); do
    docker exec g-minio mc alias set local http://127.0.0.1:9000 combee combee-secret-123456 >/dev/null 2>&1 && break
    sleep 1
  done
  docker exec g-minio mc mb --ignore-existing local/combee-backups >/dev/null 2>&1 || true

  # 预置 gate 专用 API key(Key 模式:sha256 查 api_keys 表)。
  # 全新 g-postgres 无 schema(api-server 启动才建表),先建 api_keys 表再插入。
  docker exec g-postgres psql -U combee -d combee -c \
    "CREATE TABLE IF NOT EXISTS api_keys (id UUID PRIMARY KEY, tenant_id UUID NOT NULL, name TEXT NOT NULL DEFAULT 'default', key_hash TEXT NOT NULL UNIQUE, created_at BIGINT NOT NULL, revoked_at BIGINT);" >/dev/null 2>&1
  docker exec g-postgres psql -U combee -d combee -c \
    "INSERT INTO api_keys (id, tenant_id, name, key_hash, created_at) VALUES (gen_random_uuid(), '00000000-0000-0000-0000-000000000001', 'release-gate', '$GATE_HASH', extract(epoch from now())::bigint) ON CONFLICT (key_hash) DO NOTHING" >/dev/null 2>&1

  # 3.0.5 Postgres 并发幂等回归(credits 双花):直接连 g-postgres 的宿主映射端口。
  section "Postgres 并发幂等(credits 双花回归)"
  if DATABASE_URL=postgres://combee:combee@127.0.0.1:55432/combee \
     cargo test -p combee-metadata -- --ignored > /tmp/pg-idem-test.log 2>&1 \
     && grep -qE "test result: ok" /tmp/pg-idem-test.log; then
    ok "Postgres 并发幂等(credits 双花回归)"
  else
    echo "---- 并发幂等测试输出(尾部)----"
    tail -15 /tmp/pg-idem-test.log
    fail "Postgres 并发幂等(credits 双花回归)"
  fi

  docker rm -f rel-dn rel-dn2 rel-api >/dev/null 2>&1
  for name in rel-dn rel-dn2; do
    docker run -d --name $name --network $NET \
      -e COMBEE_DATA_NODE_ADDR=0.0.0.0:9000 -e COMBEE_DATA_DIR=/data \
      -e COMBEE_API_SERVER_URL=http://rel-api:8080 -e COMBEE_NODE_ADVERTISE_URL=http://$name:9000 \
      -e COMBEE_CONTROL_PLANE_TOKEN=$GATE_TOKEN \
      -e COMBEE_S3_ENDPOINT=http://g-minio:9000 -e COMBEE_S3_ACCESS_KEY=combee \
      -e COMBEE_S3_SECRET_KEY=combee-secret-123456 -e COMBEE_S3_BUCKET=combee-backups \
      -e COMBEE_WAL_BACKUP_INTERVAL_SECS=3 -e COMBEE_REPLICA_INTERVAL_SECS=3 \
      -e COMBEE_SQL_TIMEOUT_SECS=5 \
      -v "$PWD/.docker-target/release":$BIN -w / \
      $IMG bash -c "$BIN/combee-data-node" >/dev/null 2>&1
  done
  docker run -d --name rel-api --network $NET -p 18080:8080 \
    -e COMBEE_BIND_ADDR=0.0.0.0:8080 -e COMBEE_METADATA=postgres -e COMBEE_MULTI_NODE=1 \
    -e COMBEE_AUTH=key -e COMBEE_CONTROL_PLANE_TOKEN=$GATE_TOKEN \
    -e COMBEE_DATABASE_URL=postgres://combee:combee@g-postgres:5432/combee \
    -e COMBEE_DATA_DIR=/data \
    -v "$PWD/.docker-target/release":$BIN -w / \
    $IMG bash -c "$BIN/combee-api-server" >/dev/null 2>&1

  API=http://127.0.0.1:18080
  for i in $(seq 1 40); do
    code=$(curl -s -o /dev/null -w "%{http_code}" $API/v1/databases "${KEYH[@]}" 2>/dev/null || true)
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
        H=$(curl -s -H "x-control-token: $GATE_TOKEN" $API/internal/nodes 2>/dev/null | python3 -c 'import sys,json; d=json.load(sys.stdin); print(sum(1 for n in d if n["healthy"]))' 2>/dev/null || echo 0)
        [ "$H" -ge 1 ] 2>/dev/null && REG_OK=1 && break
        sleep 1
      done
      if [ -z "$REG_OK" ]; then fail "Data Node 注册(registry 无健康节点)"; else ok "Data Node 注册($H 个 healthy 节点)"; fi
    # 创建 Cell + SQL + KV + 数据
    DB=$(curl -s -X POST $API/v1/databases "${KEYH[@]}" | python3 -c 'import sys,json;print(json.load(sys.stdin)["id"])' 2>/dev/null || true)
    if [ -z "$DB" ]; then
      fail "create Cell"
    else
      ok "create Cell($DB)"
      curl -s -X POST $API/v1/databases/$DB/sql "${KEYH[@]}" -H 'content-type: application/json' -d '{"sql":"CREATE TABLE t (x INTEGER)"}' >/dev/null
      curl -s -X POST $API/v1/databases/$DB/sql "${KEYH[@]}" -H 'content-type: application/json' -d '{"sql":"INSERT INTO t VALUES (42)"}' >/dev/null
      curl -s -X PUT $API/v1/databases/$DB/kv/k "${KEYH[@]}" -H 'content-type: application/json' -d '{"value":"persist-me"}' >/dev/null
      echo "$DB" > /tmp/combee-release-db
    fi
  fi

  # 3.2 Restart persistence:重启所有容器后数据仍在
  section "重启持久性(全部容器重启)"
  docker restart rel-dn rel-dn2 rel-api >/dev/null 2>&1
  docker compose restart postgres minio >/dev/null 2>&1
  sleep 10
  for i in $(seq 1 20); do
    code=$(curl -s -o /dev/null -w "%{http_code}" $API/v1/databases "${KEYH[@]}" 2>/dev/null || true)
    [ "$code" = "200" ] && break; sleep 2
  done
  if ! wait_registered; then fail "重启后节点未注册"; else ok "重启后节点注册"; fi

  DB=$(cat /tmp/combee-release-db 2>/dev/null || true)
  if [ -n "$DB" ]; then
    V=$(curl -s $API/v1/databases/$DB/kv/k "${KEYH[@]}" 2>/dev/null | python3 -c 'import sys,json;print(json.load(sys.stdin).get("value",""))' 2>/dev/null)
    if [ "$V" = "persist-me" ]; then
      ok "重启后 KV 数据仍在"
    else
      fail "重启后 KV 数据丢失(期望 persist-me,got $V)"
    fi
    N=$(curl -s -X POST $API/v1/databases/$DB/sql "${KEYH[@]}" -H 'content-type: application/json' -d '{"sql":"SELECT COUNT(*) AS n FROM t"}' 2>/dev/null | python3 -c 'import sys,json;print(json.load(sys.stdin)["rows"][0][0])' 2>/dev/null)
    if [ "$N" = "1" ]; then ok "重启后 SQL 数据仍在"; else fail "重启后 SQL 数据丢失(期望 1,got $N)"; fi
  else
    warn "无 db id,跳过重启持久性"
  fi

  # 3.3 Kill -9 Data Node(主)+ 重启 + integrity_check
  section "Kill -9 + SQLite integrity_check"
  PRIMARY=$(docker exec g-postgres psql -U combee -d combee -t -c "SELECT storage_node_id::text FROM databases WHERE id='$(cat /tmp/combee-release-db 2>/dev/null)'" 2>/dev/null | tr -d ' ' || true)
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
    IC=$(curl -s -X POST $API/v1/databases/$DB/sql "${KEYH[@]}" -H 'content-type: application/json' -d '{"sql":"PRAGMA integrity_check"}' 2>/dev/null)
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
    B1=$(curl -s -o /dev/null -w "%{http_code}" -X POST $API/v1/databases/$DB/backup "${KEYH[@]}")
    if [ "$B1" != "200" ]; then
      echo "!! gate: backup 返回 $B1"
      curl -s -X POST $API/v1/databases/$DB/backup "${KEYH[@]}" | head -c 400
      echo
      echo "!! data-node 日志(backup 相关):"
      docker logs rel-dn 2>&1 | grep -iE "backup|error|unavailable" | tail -5
      docker logs rel-dn2 2>&1 | grep -iE "backup|error|unavailable" | tail -5
    fi
    curl -s -X PUT $API/v1/databases/$DB/kv/k "${KEYH[@]}" -H 'content-type: application/json' -d '{"value":"after-backup"}' >/dev/null 2>&1
    sleep 7   # 等待 WAL 增量周期归档(3s,≥2 周期),使 restore 能恢复到 after-backup
    # 破坏数据节点本地数据目录:只删 Cell 数据文件与 manifest,
    # 保留 node-id(节点身份不变,restore 才能路由到同一节点)。
    for name in rel-dn rel-dn2; do
      docker exec $name sh -c 'find /data -name "*.sqlite*" -delete; rm -f /data/*.manifest.json' >/dev/null 2>&1 || true
    done
    docker restart rel-dn rel-dn2 >/dev/null 2>&1
    sleep 8
    # 由于 metadata 持久且路由到主节点,主节点数据目录被清空 → restore 从对象存储
    RC=$(curl -s -o /dev/null -w "%{http_code}" -X POST $API/v1/databases/$DB/restore "${KEYH[@]}" -H 'content-type: application/json' -d '{}')
    if [ "$RC" != "204" ]; then
      echo "!! gate: restore 返回 $RC,重试一次"
      sleep 5
      RC=$(curl -s -o /dev/null -w "%{http_code}" -X POST $API/v1/databases/$DB/restore "${KEYH[@]}" -H 'content-type: application/json' -d '{}')
    fi
    V=$(curl -s $API/v1/databases/$DB/kv/k "${KEYH[@]}" 2>/dev/null | python3 -c 'import sys,json;print(json.load(sys.stdin).get("value",""))' 2>/dev/null)
    if [ "$V" = "after-backup" ]; then
      ok "删卷后 restore 恢复(仅对象存储)"
    else
      fail "删卷后 restore 失败(期望 after-backup,got $V)"
    fi
  else
    warn "无 db id,跳过备份恢复"
  fi

  # 3.5 ENOSPC:磁盘满行为(容器小文件系统,§3)
  section "ENOSPC:磁盘满行为(容器小文件系统)"
  # 清掉上一段的 data-node,确保 enospc-dn 是唯一可用节点(placement 只选它)
  docker rm -f enospc-dn enospc-api rel-dn rel-dn2 >/dev/null 2>&1
  # data-node 的 /data 挂 24MB tmpfs → 可快速填满触发 ENOSPC
  docker run -d --name enospc-dn --network $NET --tmpfs /data:size=24m \
    -e COMBEE_DATA_NODE_ADDR=0.0.0.0:9000 -e COMBEE_DATA_DIR=/data \
    -e COMBEE_API_SERVER_URL=http://enospc-api:8080 -e COMBEE_NODE_ADVERTISE_URL=http://enospc-dn:9000 \
    -e COMBEE_CONTROL_PLANE_TOKEN=$GATE_TOKEN \
    -e COMBEE_S3_ENDPOINT=http://g-minio:9000 -e COMBEE_S3_ACCESS_KEY=combee \
    -e COMBEE_S3_SECRET_KEY=combee-secret-123456 -e COMBEE_S3_BUCKET=combee-backups \
    -e COMBEE_SQL_TIMEOUT_SECS=5 \
    -v "$PWD/.docker-target/release":$BIN -w / \
    $IMG bash -c "$BIN/combee-data-node" >/dev/null 2>&1
  docker run -d --name enospc-api --network $NET -p 18081:8080 \
    -e COMBEE_BIND_ADDR=0.0.0.0:8080 -e COMBEE_METADATA=postgres -e COMBEE_MULTI_NODE=1 \
    -e COMBEE_AUTH=key -e COMBEE_CONTROL_PLANE_TOKEN=$GATE_TOKEN \
    -e COMBEE_DATABASE_URL=postgres://combee:combee@g-postgres:5432/combee \
    -e COMBEE_DATA_DIR=/data \
    -v "$PWD/.docker-target/release":$BIN -w / \
    $IMG bash -c "$BIN/combee-api-server" >/dev/null 2>&1
  ENOSPC_API=http://127.0.0.1:18081
  for i in $(seq 1 40); do
    code=$(curl -s -o /dev/null -w "%{http_code}" $ENOSPC_API/v1/databases "${KEYH[@]}" 2>/dev/null || true)
    [ "$code" = "200" ] && break; sleep 2
  done
  sleep 5   # 等 enospc-dn agent 完成首次注册
  # 严格等待:healthy 节点恰为 enospc-dn 一个(旧节点心跳超时后必须从列表消失,
  # 否则 placement 可能把 Cell 放到已删除的 rel-dn/rel-dn2 上,导致场景失真)。
  OK=0
  for i in $(seq 1 30); do
    OK=$(curl -s -H "x-control-token: $GATE_TOKEN" $ENOSPC_API/internal/nodes 2>/dev/null | python3 -c '
import sys,json
try:
    d=json.load(sys.stdin)
    h=[n for n in d if n["healthy"]]
    print(1 if len(h)==1 and "enospc-dn" in h[0]["addr"] else 0)
except Exception:
    print(0)' 2>/dev/null || echo 0)
    [ "$OK" = "1" ] && break; sleep 2
  done
  if [ "$OK" != "1" ]; then
    fail "ENOSPC:唯一 healthy 节点不是 enospc-dn"
    echo "!! enospc-dn 容器状态:"; docker ps -a --filter name=enospc-dn --format '{{.Names}} {{.Status}}'
    echo "!! enospc-dn 日志:"; docker logs enospc-dn 2>&1 | tail -8
    docker rm -f enospc-dn enospc-api >/dev/null 2>&1
    DBE="__skip__"
  else
  DBE=$(curl -s -X POST $ENOSPC_API/v1/databases "${KEYH[@]}" | python3 -c 'import sys,json;print(json.load(sys.stdin)["id"])' 2>/dev/null || true)
  fi
  if [ -z "$DBE" ] || [ "$DBE" = "__skip__" ]; then
    fail "ENOSPC:create Cell"
  else
    ok "ENOSPC:create Cell"
    # 基线数据(填充前);写入失败说明路由错误(placement 未落在 enospc-dn)
    B1=$(curl -s -o /dev/null -w "%{http_code}" -X POST $ENOSPC_API/v1/databases/$DBE/sql "${KEYH[@]}" -H 'content-type: application/json' -d '{"sql":"CREATE TABLE t (x INTEGER)"}')
    B2=$(curl -s -o /dev/null -w "%{http_code}" -X POST $ENOSPC_API/v1/databases/$DBE/sql "${KEYH[@]}" -H 'content-type: application/json' -d '{"sql":"INSERT INTO t VALUES (1)"}')
    B3=$(curl -s -o /dev/null -w "%{http_code}" -X PUT $ENOSPC_API/v1/databases/$DBE/kv/k "${KEYH[@]}" -H 'content-type: application/json' -d '{"value":"v1"}')
    if [ "$B1$B2$B3" != "200200200" ]; then
      fail "ENOSPC:基线写入失败(路由错误?B1=$B1 B2=$B2 B3=$B3)"
      docker rm -f enospc-dn enospc-api >/dev/null 2>&1
      DBE="__skip__"
    fi
    if [ "$DBE" != "__skip__" ]; then
    # 填满 /data(dd 到 ENOSPC;容器内无 wget,用 dd 验证填充)
    docker exec enospc-dn sh -c 'dd if=/dev/zero of=/data/fill.bin bs=1M count=64 status=none 2>/dev/null || true' >/dev/null 2>&1
    sleep 1
    # 验证确实填满(否则后续"写应被拒"断言无意义)
    FULL=$(docker exec enospc-dn sh -c 'df -P /data | awk "NR==2{print \$5}"' 2>/dev/null | tr -d '%')
    if [ -z "$FULL" ] || [ "$FULL" -lt 90 ] 2>/dev/null; then
      fail "ENOSPC:tmpfs 未填满(当前 ${FULL}%),跳过写拒绝断言"
      docker exec enospc-dn sh -c 'rm -f /data/fill.bin' >/dev/null 2>&1
      DBE="__skip__"
    else
      ok "ENOSPC:tmpfs 已填满(${FULL}%)"
    fi
    fi
    if [ "$DBE" != "__skip__" ]; then
    # 写应被明确拒绝(非 200;不静默成功、不 panic)
    C1=$(curl -s -o /dev/null -w "%{http_code}" -X POST $ENOSPC_API/v1/databases/$DBE/sql "${KEYH[@]}" -H 'content-type: application/json' -d '{"sql":"INSERT INTO t VALUES (2)"}')
    C2=$(curl -s -o /dev/null -w "%{http_code}" -X PUT $ENOSPC_API/v1/databases/$DBE/kv/k2 "${KEYH[@]}" -H 'content-type: application/json' -d '{"value":"x"}')
    if [ "$C1" != "200" ] && [ "$C2" != "200" ]; then
      ok "ENOSPC:写被明确拒绝(SQL=$C1 KV=$C2,不静默成功)"
    else
      fail "ENOSPC:写未被拒绝(SQL=$C1 KV=$C2)"
    fi
    # 读仍可用 + integrity_check 无损坏(无静默数据损失)
    R1=$(curl -s -X POST $ENOSPC_API/v1/databases/$DBE/sql "${KEYH[@]}" -H 'content-type: application/json' -d '{"sql":"SELECT COUNT(*) AS n FROM t"}' 2>/dev/null | python3 -c 'import sys,json;print(json.load(sys.stdin)["rows"][0][0])' 2>/dev/null)
    IC=$(curl -s -X POST $ENOSPC_API/v1/databases/$DBE/sql "${KEYH[@]}" -H 'content-type: application/json' -d '{"sql":"PRAGMA integrity_check"}' 2>/dev/null)
    if [ "$R1" = "1" ] && echo "$IC" | grep -q '"ok"'; then
      ok "ENOSPC:读可用 + integrity_check = ok(无损坏)"
    else
      fail "ENOSPC:读/integrity 异常(R1=$R1 IC=$IC)"
    fi
    # data-node 进程存活(未 panic)
    if docker top enospc-dn 2>/dev/null | grep -q "combee-data-node"; then
      ok "ENOSPC:data-node 存活(进程在)"
    else
      fail "ENOSPC:data-node 异常(进程不在)"
    fi
    # 清理填充 → 写恢复
    docker exec enospc-dn sh -c 'rm -f /data/fill.bin' >/dev/null 2>&1
    sleep 1
    C3=$(curl -s -o /dev/null -w "%{http_code}" -X POST $ENOSPC_API/v1/databases/$DBE/sql "${KEYH[@]}" -H 'content-type: application/json' -d '{"sql":"INSERT INTO t VALUES (3)"}')
    if [ "$C3" = "200" ]; then
      ok "ENOSPC:清理后写恢复"
    else
      fail "ENOSPC:清理后写未恢复(SQL=$C3)"
    fi
    fi
  fi
  docker rm -f enospc-dn enospc-api >/dev/null 2>&1

  # 清理(gate 专用资源;不动宿主机 compose 全家桶)
  docker rm -f rel-dn rel-dn2 rel-api enospc-dn enospc-api g-postgres g-minio >/dev/null 2>&1
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
