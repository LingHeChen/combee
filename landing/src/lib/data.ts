/* 代码示例(双语页面共用,代码本身不翻译)+ 截图资源路径 */

export const SCREEN_SRCS = [
  "/screens/overview.png",
  "/screens/cells.png",
  "/screens/sql.png",
  "/screens/usage.png",
];

export const TS_CODE = `import { Combee } from "@combee/sdk";

const combee = new Combee({ apiKey: process.env.COMBEE_API_KEY });

// One app, one Cell — created in a single call
const cell = await combee.cells.create({ name: "my-app" });

// SQL, right away
await cell.sql.execute(
  "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)"
);
await cell.sql.execute("INSERT INTO users (name) VALUES (?)", ["Ada"]);
const { rows } = await cell.sql.query("SELECT * FROM users");

// KV with TTL on the same Cell
await cell.kv.set("session:42", "user:7", { ttl: 3600 });
const value = await cell.kv.get("session:42");`;

export const HTTP_CODE = `POST /v1/databases                  # create a Cell
POST /v1/databases/{id}/sql          # run SQL
PUT  /v1/databases/{id}/kv/{key}     # set a KV value
GET  /v1/databases/{id}/kv/{key}     # get — TTL-aware
POST /v1/databases/{id}/backup       # snapshot to object storage`;
