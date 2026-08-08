"""async_basic:异步客户端(AsyncCombee)。"""
import asyncio
from combee import AsyncCombee


async def main() -> None:
    combee = AsyncCombee(base_url="http://127.0.0.1:8080", api_key="dev-key")
    cell = await combee.cells.acreate(name="async-app")
    await cell.sql.aexecute("CREATE TABLE t (x INTEGER)")
    await cell.kv.aset("k", "v")
    print("async get:", await cell.kv.aget("k"))
    await combee.aclose()


asyncio.run(main())
