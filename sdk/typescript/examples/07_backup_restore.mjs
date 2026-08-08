// 07_backup_restore:备份(需配置对象存储)。
import { client } from "./00_helpers.mjs";
const combee = client();
const cell = await combee.cells.create();
await cell.kv.set("k", "v");
await cell.backups.create();          // 全量快照
await cell.backups.createIncremental(); // WAL 增量
await cell.backups.restoreLatest();   // 恢复(破坏性)
console.log("backup/restore done");
