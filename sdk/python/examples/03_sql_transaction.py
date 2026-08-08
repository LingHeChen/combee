"""03_sql_transaction:多条语句原子执行。"""
from helpers import client

combee = client()
cell = combee.cells.create()
cell.sql.execute("CREATE TABLE t (x INTEGER)")
results = cell.sql.transaction(
    [{"sql": "INSERT INTO t VALUES (?)", "params": [1]}, {"sql": "INSERT INTO t VALUES (?)", "params": [2]}]
)
print("rows affected:", [r["rows_affected"] for r in results])
