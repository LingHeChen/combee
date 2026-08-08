# combee — Python SDK

Combee 的 Python 客户端(同步 `Combee` + 异步 `AsyncCombee`,基于 httpx)。

```python
from combee import Combee

combee = Combee(base_url="https://api.combee.example", api_key="cmb_sk_...")
cell = combee.cells.create(name="my-app")

cell.sql.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
cell.sql.execute("INSERT INTO users (name) VALUES (?)", ["Alice"])
rows = cell.sql.query("SELECT id, name FROM users")["rows"]

cell.kv.set("session:abc", "user:1", ttl=3600)
print(combee.usage.summary())
print(combee.credits.balance())
```

- 完整能力:Cell CRUD / SQL+事务 / KV 全子集(TTL/计数器/JSON)/ 备份恢复 / 复制状态 /
  API Keys / Usage / Credits / Voucher 兑换 / Pricing;
- 错误:稳定 code → 类型化异常(均携带 `request_id`);
- 内部接口(`/internal/*`、`/rpc/*`、`/admin/*`)不出现在 SDK 表面。

## 测试

```bash
python3 -m venv .venv && .venv/bin/pip install httpx pytest pytest-asyncio
cargo build -p combee-api-server            # contract tests 需要真实 server
.venv/bin/python -m pytest tests/ -q
```

## 示例

`examples/01_create_cell.py` … `examples/10_credits.py` + `async_basic.py`。

## 发布

`pip install build && python -m build && twine upload dist/*`(发布到 PyPI 时执行)。
