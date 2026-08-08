// 08_api_keys:创建 / 列出 / 撤销(明文仅返回一次)。
import { client } from "./00_helpers.mjs";
const combee = client();
const created = await combee.apiKeys.create("production");
console.log("plaintext key (once):", created.key);
console.log("keys:", (await combee.apiKeys.list()).map((k) => k.id));
await combee.apiKeys.revoke(created.id);
console.log("revoked");
