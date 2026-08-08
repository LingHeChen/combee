// 01_create_cell:创建 Cell(懒创建,零 IO)。
import { client } from "./00_helpers.mjs";
const combee = client();
const cell = await combee.cells.create({ name: "my-app" });
console.log("cell id:", cell.id);
