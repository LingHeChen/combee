"""10_credits:余额 / 账本 / 兑换。"""
from helpers import client

combee = client()
print("balance:", combee.credits.balance())
print("transactions:", combee.credits.transactions(10)["items"])
# 兑换(需要 admin 先生成 voucher):combee.credits.redeem("CMB-XXXX-XXXX-XXXX")
