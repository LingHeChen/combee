// 03_sql_transaction:多条语句原子执行。
import { client } from "./00_helpers.mjs";
const combee = client();
const cell = await combee.cells.create();
await cell.sql.execute("CREATE TABLE t (x INTEGER)");
const results = await cell.sql.transaction([
  { sql: "INSERT INTO t VALUES (?)", params: [1] },
  { sql: "INSERT INTO t VALUES (?)", params: [2] },
]);
console.log("rows affected:", results.map((r) => r.rows_affected));
