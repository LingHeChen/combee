"""08_api_keys:创建 / 列出 / 撤销(明文仅返回一次)。"""
from helpers import client

combee = client()
created = combee.api_keys.create("production")
print("plaintext key (once):", created["key"])
print("keys:", [k["id"] for k in combee.api_keys.list()])
combee.api_keys.revoke(created["id"])
print("revoked")
