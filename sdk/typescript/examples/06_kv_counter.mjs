// 06_kv_counter:原子计数器。
import { client } from "./00_helpers.mjs";
const combee = client();
const cell = await combee.cells.create();
console.log("incr:", await cell.kv.incr("pageviews"));
console.log("incr+5:", await cell.kv.incr("pageviews", 5));
console.log("decr-2:", await cell.kv.decr("pageviews", 2));
