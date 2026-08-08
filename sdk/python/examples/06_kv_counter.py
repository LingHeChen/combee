"""06_kv_counter:原子计数器。"""
from helpers import client

combee = client()
cell = combee.cells.create()
print("incr:", cell.kv.incr("pageviews"))
print("incr+5:", cell.kv.incr("pageviews", 5))
print("decr-2:", cell.kv.decr("pageviews", 2))
