// 05_kv_ttl:TTL / persist。
import { client } from "./00_helpers.mjs";
const combee = client();
const cell = await combee.cells.create();
await cell.kv.set("session", "abc", { ttl: 60 });
console.log("ttl:", await cell.kv.ttl("session"));
await cell.kv.persist("session");
console.log("after persist:", await cell.kv.ttl("session"));
