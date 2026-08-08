"""Combee SDK。

```python
from combee import Combee

combee = Combee(base_url="https://api.combee.example", api_key="cmb_sk_...")
cell = combee.cells.create(name="my-app")
cell.sql.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
cell.kv.set("session:abc", "user:1", ttl=3600)
```
"""

from .client import AsyncCombeeHttp, CombeeHttp
from .cells import Backups, Cell, Cells, Kv, Replication, Sql
from .control import ApiKeys, Credits, Pricing, Usage
from .errors import (
    ApiKeyNotFoundError,
    AuthenticationError,
    CellNotFoundError,
    CombeeError,
    DataNodeUnavailableError,
    InsufficientCreditsError,
    InternalServerError,
    InvalidRequestError,
    PermissionDeniedError,
    QuotaExceededError,
    RateLimitError,
    SqlError,
    SqlTimeoutError,
)

__all__ = [
    "Combee",
    "AsyncCombee",
    "CombeeError",
    "AuthenticationError",
    "PermissionDeniedError",
    "CellNotFoundError",
    "ApiKeyNotFoundError",
    "InvalidRequestError",
    "SqlError",
    "SqlTimeoutError",
    "RateLimitError",
    "QuotaExceededError",
    "InsufficientCreditsError",
    "DataNodeUnavailableError",
    "InternalServerError",
]


class Combee:
    """同步客户端。"""

    def __init__(self, base_url: str, api_key: str, timeout_ms: int = 30_000):
        self._http = CombeeHttp(base_url=base_url, api_key=api_key, timeout_ms=timeout_ms)
        self.cells = Cells(self._http)
        self.api_keys = ApiKeys(self._http)
        self.usage = Usage(self._http)
        self.credits = Credits(self._http)
        self.pricing = Pricing(self._http)

    def cell(self, cell_id: str) -> Cell:
        return Cell(self._http, cell_id)

    def close(self) -> None:
        self._http.close()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


class AsyncCombee:
    """异步客户端(httpx.AsyncClient)。"""

    def __init__(self, base_url: str, api_key: str, timeout_ms: int = 30_000):
        self._http = AsyncCombeeHttp(base_url=base_url, api_key=api_key, timeout_ms=timeout_ms)
        self.cells = Cells(self._http)
        self.api_keys = ApiKeys(self._http)
        self.usage = Usage(self._http)
        self.credits = Credits(self._http)
        self.pricing = Pricing(self._http)

    def cell(self, cell_id: str) -> Cell:
        return Cell(self._http, cell_id)

    async def aclose(self) -> None:
        await self._http.aclose()

    async def __aenter__(self):
        return self

    async def __aexit__(self, *exc):
        await self.aclose()
