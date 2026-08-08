"""API Keys / Usage / Credits / Pricing(User Control Plane)。"""

from __future__ import annotations

from typing import Optional


class ApiKeys:
    def __init__(self, http):
        self._http = http

    def create(self, name: Optional[str] = None) -> dict:
        r = self._http.request("POST", "/v1/api-keys", {}, idempotency_key=f"key:{name or 'anon'}")
        # 服务端返回 {key, record:{...}} → 平铺
        return {**r["record"], "key": r["key"]}

    async def acreate(self, name: Optional[str] = None) -> dict:
        r = await self._http.request("POST", "/v1/api-keys", {}, idempotency_key=f"key:{name or 'anon'}")
        return {**r["record"], "key": r["key"]}

    def list(self) -> list:
        return self._http.request("GET", "/v1/api-keys")

    async def alist(self) -> list:
        return await self._http.request("GET", "/v1/api-keys")

    def revoke(self, key_id: str) -> None:
        self._http.request("DELETE", f"/v1/api-keys/{key_id}")

    async def arevoke(self, key_id: str) -> None:
        await self._http.request("DELETE", f"/v1/api-keys/{key_id}")


class Usage:
    def __init__(self, http):
        self._http = http

    def _q(self, opts: Optional[dict]) -> str:
        parts = []
        if opts and opts.get("from"):
            parts.append(f"from={opts['from']}")
        if opts and opts.get("to"):
            parts.append(f"to={opts['to']}")
        return ("?" + "&".join(parts)) if parts else ""

    def summary(self, opts: Optional[dict] = None) -> dict:
        return self._http.request("GET", f"/v1/usage/summary{self._q(opts)}")

    async def asummary(self, opts: Optional[dict] = None) -> dict:
        return await self._http.request("GET", f"/v1/usage/summary{self._q(opts)}")

    def cell(self, cell_id: str, opts: Optional[dict] = None) -> dict:
        return self._http.request("GET", f"/v1/cells/{cell_id}/usage{self._q(opts)}")

    async def acell(self, cell_id: str, opts: Optional[dict] = None) -> dict:
        return await self._http.request("GET", f"/v1/cells/{cell_id}/usage{self._q(opts)}")

    def timeseries(self, metric: str, interval: str = "minute", opts: Optional[dict] = None) -> list:
        parts = [f"metric={metric}", f"interval={interval}"]
        if opts and opts.get("from"):
            parts.append(f"from={opts['from']}")
        if opts and opts.get("to"):
            parts.append(f"to={opts['to']}")
        return self._http.request("GET", f"/v1/usage/timeseries?{'&'.join(parts)}")

    async def atimeseries(self, metric: str, interval: str = "minute", opts: Optional[dict] = None) -> list:
        parts = [f"metric={metric}", f"interval={interval}"]
        if opts and opts.get("from"):
            parts.append(f"from={opts['from']}")
        if opts and opts.get("to"):
            parts.append(f"to={opts['to']}")
        return await self._http.request("GET", f"/v1/usage/timeseries?{'&'.join(parts)}")


class Credits:
    def __init__(self, http):
        self._http = http

    def balance(self) -> dict:
        return self._http.request("GET", "/v1/credits/balance")

    async def abalance(self) -> dict:
        return await self._http.request("GET", "/v1/credits/balance")

    def transactions(self, limit: int = 100, cursor: Optional[str] = None) -> dict:
        return self._http.paginate("/v1/credits/transactions", limit, cursor)

    async def atransactions(self, limit: int = 100, cursor: Optional[str] = None) -> dict:
        return await self._http.request("GET", f"/v1/credits/transactions?limit={limit}")

    def redeem(self, code: str) -> dict:
        return self._http.request("POST", "/v1/credits/redeem", {"code": code})

    async def aredeem(self, code: str) -> dict:
        return await self._http.request("POST", "/v1/credits/redeem", {"code": code})


class Pricing:
    def __init__(self, http):
        self._http = http

    def get(self) -> dict:
        return self._http.request("GET", "/v1/pricing")

    async def aget(self) -> dict:
        return await self._http.request("GET", "/v1/pricing")
