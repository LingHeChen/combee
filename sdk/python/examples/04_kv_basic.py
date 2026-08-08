"""04_kv_basic:KV set / get / delete。"""
from helpers import client

combee = client()
cell = combee.cells.create()
cell.kv.set("greeting", "hello")
print("greeting:", cell.kv.get("greeting"))
cell.kv.delete("greeting")
print("after delete:", cell.kv.get("greeting"))
