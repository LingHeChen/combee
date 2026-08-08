// 04_kv_basic:KV set / get / delete。
import { client } from "./00_helpers.mjs";
const combee = client();
const cell = await combee.cells.create();
await cell.kv.set("greeting", "hello");
console.log("greeting:", await cell.kv.get("greeting"));
await cell.kv.delete("greeting");
console.log("after delete:", await cell.kv.get("greeting"));
