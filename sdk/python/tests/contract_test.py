"""Contract tests:对真实 Combee server 执行 Python SDK 全功能矩阵。

运行:`COMBEE_URL=... pytest tests/`(或默认自动起本地 server)。
"""

import os
import subprocess
import sys
import time
import uuid
from pathlib import Path

import pytest

from combee import AsyncCombee, CellNotFoundError, Combee, SqlError

REPO_ROOT = Path(__file__).resolve().parents[3]
PORT = 18092
BASE_URL = f"http://127.0.0.1:{PORT}"


@pytest.fixture(scope="session")
def server():
    """启动真实 Combee server(dev 模式,单进程)。"""
    env = dict(os.environ)
    env.update(
        COMBEE_BIND_ADDR=f"127.0.0.1:{PORT}",
        COMBEE_DATA_DIR=str(REPO_ROOT / "target/.sdk-py-test-data"),
        COMBEE_AUTH="off",
        COMBEE_USAGE_FLUSH_INTERVAL_SECS="1",
    )
    proc = subprocess.Popen(
        [str(REPO_ROOT / "target/debug/combee-api-server")],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    deadline = time.time() + 30
    while time.time() < deadline:
        try:
            import urllib.request

            if urllib.request.urlopen(f"{BASE_URL}/v1/databases", timeout=1).status == 200:
                break
        except Exception:
            time.sleep(0.2)
    else:
        proc.kill()
        raise RuntimeError("server not ready")
    yield BASE_URL
    proc.kill()


@pytest.fixture()
def combee(server):
    yield Combee(base_url=server, api_key="dev-key")


@pytest.fixture()
def acombee(server):
    yield AsyncCombee(base_url=server, api_key="dev-key")


def test_cell_crud(combee):
    cell = combee.cells.create(name="py-contract")
    assert cell.id
    listed = combee.cells.list()
    assert any(c["id"] == cell.id for c in listed["items"])
    info = combee.cells.get(cell.id)
    assert info.id == cell.id
    cell.delete()
    assert not any(c["id"] == cell.id for c in combee.cells.list()["items"])


def test_sql_query_execute_transaction(combee):
    cell = combee.cells.create()
    cell.sql.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
    ins = cell.sql.execute("INSERT INTO users (name) VALUES (?)", ["Alice"])
    assert ins["rows_affected"] == 1
    q = cell.sql.query("SELECT id, name FROM users WHERE id = ?", [1])
    assert q["columns"] == ["id", "name"]
    assert q["rows"][0]["name"] == "Alice"

    with pytest.raises(SqlError):
        cell.sql.transaction(
            [
                {"sql": "INSERT INTO users (name) VALUES (?)", "params": ["Bob"]},
                {"sql": "INSERT INTO nope_table VALUES (1)"},
            ]
        )
    n = cell.sql.query("SELECT COUNT(*) AS n FROM users")["rows"][0]["n"]
    assert n == 1, "事务回滚"


def test_kv_full_subset(combee):
    cell = combee.cells.create()
    assert cell.kv.get("missing") is None
    assert cell.kv.set("greeting", "hello", ttl=3600) is True
    assert cell.kv.get("greeting") == "hello"
    assert cell.kv.ttl("greeting")["state"] == "expires"
    cell.kv.persist("greeting")
    assert cell.kv.ttl("greeting")["state"] == "persistent"
    cell.kv.mset({"a": "1", "b": "2"})
    assert cell.kv.mget(["a", "b", "nope"]) == ["1", "2", None]
    assert cell.kv.set("a", "overwrite", condition="nx") is False
    assert cell.kv.incr("pageviews") == 1
    assert cell.kv.incr("pageviews", 5) == 6
    assert cell.kv.decr("pageviews", 2) == 4
    assert cell.kv.exists("greeting") is True
    assert cell.kv.delete("greeting") is True


def test_api_keys_lifecycle(combee):
    created = combee.api_keys.create("prod")
    assert created["key"].startswith("cmb_sk_")
    keys = combee.api_keys.list()
    assert any(k["id"] == created["id"] for k in keys)
    combee.api_keys.revoke(created["id"])
    assert all(k["id"] != created["id"] or k.get("revoked_at") for k in combee.api_keys.list())


def test_usage_and_credits(combee):
    cell = combee.cells.create()
    cell.kv.set("u", "1")
    time.sleep(1.5)  # 等 usage flush(1s)
    summary = combee.usage.summary()
    assert summary["request_count"] >= 3
    balance = combee.credits.balance()
    assert balance["currency"] == "CREDIT"
    assert balance["available"].isdigit()
    pricing = combee.pricing.get()
    assert "version" in pricing


def test_errors_carry_request_id(combee):
    with pytest.raises(CellNotFoundError):
        combee.cells.get(str(uuid.UUID(int=0)))
    cell = combee.cells.create()
    with pytest.raises(SqlError) as exc:
        cell.sql.execute("THIS IS NOT SQL")
    assert isinstance(exc.value.request_id, str)


@pytest.mark.asyncio
async def test_async_parity(acombee):
    cell = await acombee.cells.acreate(name="async-cell")
    await cell.sql.aexecute("CREATE TABLE t (x INTEGER)")
    await cell.kv.aset("k", "v")
    assert await cell.kv.aget("k") == "v"
    assert await cell.kv.aincr("n") == 1
    balance = await acombee.credits.abalance()
    assert balance["currency"] == "CREDIT"
    await cell.adelete()
