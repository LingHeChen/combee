"""07_backup_restore:备份(需配置对象存储)。"""
from helpers import client

combee = client()
cell = combee.cells.create()
cell.kv.set("k", "v")
cell.backups.create()
cell.backups.create_incremental()
cell.backups.restore_latest()
print("backup/restore done")
