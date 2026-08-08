"""09_usage:用量查询。"""
import time
from helpers import client

combee = client()
cell = combee.cells.create()
cell.kv.set("x", "1")
time.sleep(6)  # 等 usage flush
print("summary:", combee.usage.summary())
print("cell usage:", combee.usage.cell(cell.id))
