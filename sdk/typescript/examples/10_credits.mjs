// 10_credits:余额 / 账本 / 兑换。
import { client } from "./00_helpers.mjs";
const combee = client();
console.log("balance:", await combee.credits.balance());
console.log("transactions:", (await combee.credits.transactions(10)).items);
// 兑换(需要 admin 先生成 voucher):await combee.credits.redeem("CMB-XXXX-XXXX-XXXX");
