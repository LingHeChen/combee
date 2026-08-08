"""05_kv_ttl:TTL / persist。"""
from helpers import client

combee = client()
cell = combee.cells.create()
cell.kv.set("session", "abc", ttl=60)
print("ttl:", cell.kv.ttl("session"))
cell.kv.persist("session")
print("after persist:", cell.kv.ttl("session"))
