// 09_usage:用量查询。
import { client } from "./00_helpers.mjs";
const combee = client();
const cell = await combee.cells.create();
await cell.kv.set("x", "1");
await new Promise((r) => setTimeout(r, 6000)); // 等 usage flush
console.log("summary:", await combee.usage.summary());
console.log("cell usage:", await combee.usage.cell(cell.id));
