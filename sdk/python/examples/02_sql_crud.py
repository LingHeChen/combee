"""02_sql_crud:SQL 建表 / 插入 / 查询。"""
from helpers import client

combee = client()
cell = combee.cells.create()
cell.sql.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
cell.sql.execute("INSERT INTO users (name) VALUES (?)", ["Alice"])
rows = cell.sql.query("SELECT id, name FROM users")["rows"]
print("users:", rows)
