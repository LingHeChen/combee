"""Cells / SQL / KV / Backups / Replication(同步 + 异步等价)。"""

from __future__ import annotations

from typing import Any, Optional


class Sql:
    def __init__(self, http, cell_id: str):
        self._http = http
        self._cell = cell_id

    def _path(self) -> str:
        return f"/v1/databases/{self._cell}/sql"

    async def _apath(self) -> str:
        return self._path()

    def _map_rows(self, raw: dict) -> dict:
        columns = raw.get("columns", [])
        rows = [
            dict(zip(columns, row)) if isinstance(row, list) else row
            for row in raw.get("rows", [])
        ]
        return {"columns": columns, "rows": rows}

    @staticmethod
    def _body(sql: str, params: Optional[list]) -> dict:
        # 服务端不接受 params: null;无参数时省略字段
        body: dict = {"sql": sql}
        if params is not None:
            body["params"] = params
        return body

    def query(self, sql: str, params: Optional[list] = None) -> dict:
        return self._map_rows(self._http.request("POST", self._path(), self._body(sql, params)))

    async def aquery(self, sql: str, params: Optional[list] = None) -> dict:
        return self._map_rows(await self._http.request("POST", self._path(), self._body(sql, params)))

    def execute(self, sql: str, params: Optional[list] = None) -> dict:
        return self._http.request("POST", self._path(), self._body(sql, params))

    async def aexecute(self, sql: str, params: Optional[list] = None) -> dict:
        return await self._http.request("POST", self._path(), self._body(sql, params))

    def transaction(self, statements: list[dict]) -> list:
        return self._http.request("POST", f"/v1/databases/{self._cell}/transaction", {"statements": statements})

    async def atransaction(self, statements: list[dict]) -> list:
        return await self._http.request("POST", f"/v1/databases/{self._cell}/transaction", {"statements": statements})


class Kv:
    def __init__(self, http, cell_id: str):
        self._http = http
        self._cell = cell_id

    def _k(self, key: str) -> str:
        return f"/v1/databases/{self._cell}/kv/{key}"

    # ---- 同步 ----
    def get(self, key: str) -> Optional[str]:
        r = self._http.request("GET", self._k(key))
        return r.get("value") if r else None

    def get_json(self, key: str) -> Any:
        import json

        v = self.get(key)
        return json.loads(v) if v is not None else None

    def set(self, key: str, value: str, ttl: Optional[int] = None, condition: Optional[str] = None) -> bool:
        body = {"value": value, "ttl_seconds": ttl, "nx": condition == "nx", "xx": condition == "xx"}
        r = self._http.request("PUT", self._k(key), body)
        return bool(r.get("written"))

    def set_json(self, key: str, value: Any, ttl: Optional[int] = None, condition: Optional[str] = None) -> bool:
        import json

        return self.set(key, json.dumps(value), ttl, condition)

    def delete(self, key: str) -> bool:
        r = self._http.request("DELETE", self._k(key))
        return bool(r.get("deleted"))

    def exists(self, key: str) -> bool:
        r = self._http.request("POST", f"/v1/databases/{self._cell}/kv/ops/exists", {"keys": [key]})
        return bool(r[0])

    def mget(self, keys: list[str]) -> list[Optional[str]]:
        r = self._http.request("POST", f"/v1/databases/{self._cell}/kv/ops/mget", {"keys": keys})
        return list(r.get("values", []))

    def mset(self, entries: dict[str, str]) -> None:
        items = [{"key": k, "value": v} for k, v in entries.items()]
        self._http.request("POST", f"/v1/databases/{self._cell}/kv/ops/mset", {"items": items})

    def ttl(self, key: str) -> dict:
        r = self._http.request("POST", f"/v1/databases/{self._cell}/kv/ops/ttl", {"keys": [key]})
        ttl = r[0]
        if ttl is None:
            return {"state": "missing"}
        if ttl < 0:
            return {"state": "persistent"}
        return {"state": "expires", "seconds": ttl}

    def expire(self, key: str, seconds: int) -> bool:
        r = self._http.request("POST", f"/v1/databases/{self._cell}/kv/ops/expire", {"key": key, "ttl_seconds": seconds})
        return bool(r.get("updated"))

    def persist(self, key: str) -> bool:
        r = self._http.request("POST", f"/v1/databases/{self._cell}/kv/ops/expire", {"key": key, "ttl_seconds": None})
        return bool(r.get("updated"))

    def incr(self, key: str, delta: int = 1) -> int:
        r = self._http.request("POST", f"/v1/databases/{self._cell}/kv/ops/incr", {"key": key, "delta": delta})
        return int(r.get("value"))

    def decr(self, key: str, delta: int = 1) -> int:
        return self.incr(key, -delta)

    # ---- 异步 ----
    async def aget(self, key: str) -> Optional[str]:
        r = await self._http.request("GET", self._k(key))
        return r.get("value") if r else None

    async def aset(self, key: str, value: str, ttl: Optional[int] = None, condition: Optional[str] = None) -> bool:
        body = {"value": value, "ttl_seconds": ttl, "nx": condition == "nx", "xx": condition == "xx"}
        r = await self._http.request("PUT", self._k(key), body)
        return bool(r.get("written"))

    async def adelete(self, key: str) -> bool:
        r = await self._http.request("DELETE", self._k(key))
        return bool(r.get("deleted"))

    async def aincr(self, key: str, delta: int = 1) -> int:
        r = await self._http.request("POST", f"/v1/databases/{self._cell}/kv/ops/incr", {"key": key, "delta": delta})
        return int(r.get("value"))


class Backups:
    def __init__(self, http, cell_id: str):
        self._http = http
        self._cell = cell_id

    def create(self) -> dict:
        return self._http.request("POST", f"/v1/databases/{self._cell}/backup")

    def create_incremental(self) -> dict:
        return self._http.request("POST", f"/v1/databases/{self._cell}/backup/incr")

    def restore(self, version: Optional[str] = None) -> None:
        self._http.request("POST", f"/v1/databases/{self._cell}/restore", {"version": version} if version else {})

    def restore_latest(self) -> None:
        self.restore()


class Replication:
    def __init__(self, http, cell_id: str):
        self._http = http
        self._cell = cell_id

    def get(self) -> dict:
        r = self._http.request("GET", f"/v1/databases/{self._cell}/replication")
        return {
            "enabled": bool(r.get("replica_node")),
            "replica_node": r.get("replica_node"),
        }

    def enable(self, replica_node: str) -> None:
        self._http.request("POST", f"/v1/databases/{self._cell}/replication", {"replica_node": replica_node})

    def disable(self) -> None:
        self._http.request("DELETE", f"/v1/databases/{self._cell}/replication")


class Cell:
    def __init__(self, http, cell_id: str):
        self.id = cell_id
        self.sql = Sql(http, cell_id)
        self.kv = Kv(http, cell_id)
        self.backups = Backups(http, cell_id)
        self.replication = Replication(http, cell_id)
        self._http = http

    def info(self) -> dict:
        all_cells = self._http.request("GET", "/v1/databases?limit=1000")
        for c in all_cells or []:
            if c.get("id") == self.id:
                return c
        from .errors import CellNotFoundError

        raise CellNotFoundError(f"cell not found: {self.id}")

    async def ainfo(self) -> dict:
        all_cells = await self._http.request("GET", "/v1/databases?limit=1000")
        for c in all_cells or []:
            if c.get("id") == self.id:
                return c
        from .errors import CellNotFoundError

        raise CellNotFoundError(f"cell not found: {self.id}")

    def delete(self) -> None:
        self._http.request("DELETE", f"/v1/databases/{self.id}")

    async def adelete(self) -> None:
        await self._http.request("DELETE", f"/v1/databases/{self.id}")


class Cells:
    def __init__(self, http):
        self._http = http

    def create(self, name: Optional[str] = None, region: Optional[str] = None) -> Cell:
        key = f"cell:{name}" if name else None
        r = self._http.request("POST", "/v1/databases", {}, idempotency_key=key)
        return Cell(self._http, r["id"])

    async def acreate(self, name: Optional[str] = None, region: Optional[str] = None) -> Cell:
        key = f"cell:{name}" if name else None
        r = await self._http.request("POST", "/v1/databases", {}, idempotency_key=key)
        return Cell(self._http, r["id"])

    def get(self, cell_id: str) -> Cell:
        cell = Cell(self._http, cell_id)
        cell.info()
        return cell

    async def aget(self, cell_id: str) -> Cell:
        cell = Cell(self._http, cell_id)
        await cell.ainfo()
        return cell

    def list(self, limit: int = 100, cursor: Optional[str] = None) -> dict:
        arr = self._http.request("GET", f"/v1/databases?limit={limit}")
        return {"items": arr or [], "next_cursor": None}

    async def alist(self, limit: int = 100, cursor: Optional[str] = None) -> dict:
        arr = await self._http.request("GET", f"/v1/databases?limit={limit}")
        return {"items": arr or [], "next_cursor": None}

    def delete(self, cell_id: str) -> None:
        self._http.request("DELETE", f"/v1/databases/{cell_id}")

    async def adelete(self, cell_id: str) -> None:
        await self._http.request("DELETE", f"/v1/databases/{cell_id}")
