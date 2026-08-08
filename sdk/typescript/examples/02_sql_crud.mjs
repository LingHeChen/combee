// 02_sql_crud:SQL 建表 / 插入 / 查询。
import { client } from "./00_helpers.mjs";
const combee = client();
const cell = await combee.cells.create();
await cell.sql.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)");
await cell.sql.execute("INSERT INTO users (name) VALUES (?)", ["Alice"]);
const { rows } = await cell.sql.query("SELECT id, name FROM users");
console.log("users:", rows);
