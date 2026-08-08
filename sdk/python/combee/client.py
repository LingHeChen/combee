"""核心 HTTP 客户端(同步 + 异步,httpx)。"""

from __future__ import annotations

import time
import uuid
from typing import Any, Optional

import httpx

from .errors import CombeeError, from_error_body

USER_AGENT = "combee-python/0.1.0a1"


class _BaseHttp:
    def __init__(self, base_url: str, api_key: str, timeout_ms: int = 30_000):
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.timeout_ms = timeout_ms

    def _headers(self, idempotency_key: Optional[str] = None) -> dict[str, str]:
        h = {
            "content-type": "application/json",
            "x-api-key": self.api_key,
            "x-request-id": str(uuid.uuid4()),
            "user-agent": USER_AGENT,
        }
        if idempotency_key:
            h["idempotency-key"] = idempotency_key
        return h

    def _handle(self, resp: httpx.Response) -> Any:
        rid = resp.headers.get("x-request-id")
        if resp.is_success:
            if not resp.content:
                return None
            return resp.json()
        try:
            body = resp.json()
            code = str(body.get("code", "internal"))
            message = str(body.get("error", resp.text))
        except Exception:
            code, message = "internal", resp.text or f"HTTP {resp.status_code}"
        raise from_error_body(code, message, resp.status_code, rid)

    def paginate(self, path: str, limit: int = 100, cursor: Optional[str] = None) -> dict:
        sep = "&" if "?" in path else "?"
        q = f"{sep}limit={limit}" + (f"&cursor={cursor}" if cursor else "")
        return self.request("GET", f"{path}{q}")


class CombeeHttp(_BaseHttp):
    def __init__(self, *args, retry: bool = True, **kwargs):
        super().__init__(*args, **kwargs)
        self._client = httpx.Client(timeout=self.timeout_ms / 1000.0, trust_env=False)
        self.retry = retry

    def request(
        self,
        method: str,
        path: str,
        body: Optional[dict] = None,
        idempotency_key: Optional[str] = None,
    ) -> Any:
        url = f"{self.base_url}{path}"
        attempts = 3 if self.retry and method == "GET" else 1
        last: Optional[CombeeError] = None
        for attempt in range(attempts):
            try:
                resp = self._client.request(
                    method, url, json=body, headers=self._headers(idempotency_key)
                )
                return self._handle(resp)
            except CombeeError as e:
                last = e
                if not (attempt + 1 < attempts and e.status is not None and e.status >= 500):
                    raise
                time.sleep(0.1 * (2 ** attempt))
        raise last  # pragma: no cover

    def close(self) -> None:
        self._client.close()


class AsyncCombeeHttp(_BaseHttp):
    def __init__(self, *args, retry: bool = True, **kwargs):
        super().__init__(*args, **kwargs)
        self._client = httpx.AsyncClient(timeout=self.timeout_ms / 1000.0, trust_env=False)
        self.retry = retry

    async def request(
        self,
        method: str,
        path: str,
        body: Optional[dict] = None,
        idempotency_key: Optional[str] = None,
    ) -> Any:
        import asyncio

        url = f"{self.base_url}{path}"
        attempts = 3 if self.retry and method == "GET" else 1
        last: Optional[CombeeError] = None
        for attempt in range(attempts):
            try:
                resp = await self._client.request(
                    method, url, json=body, headers=self._headers(idempotency_key)
                )
                return self._handle(resp)
            except CombeeError as e:
                last = e
                if not (attempt + 1 < attempts and e.status is not None and e.status >= 500):
                    raise
                await asyncio.sleep(0.1 * (2 ** attempt))
        raise last  # pragma: no cover

    async def aclose(self) -> None:
        await self._client.aclose()
