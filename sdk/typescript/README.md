# @combee/sdk — TypeScript SDK

Combee 的 TypeScript/JavaScript 客户端(Node ≥ 18,零运行时依赖,原生 fetch)。

```ts
import { Combee } from "@combee/sdk";

const combee = new Combee({ baseUrl: "https://api.combee.example", apiKey: "cmb_sk_..." });
const cell = await combee.cells.create({ name: "my-app" });

await cell.sql.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)");
await cell.sql.execute("INSERT INTO users (name) VALUES (?)", ["Alice"]);
const { rows } = await cell.sql.query("SELECT id, name FROM users");

await cell.kv.set("session:abc", "user:1", { ttl: 3600 });
console.log(await combee.usage.summary());
console.log(await combee.credits.balance());
```

- 完整能力:Cell CRUD / SQL+事务 / KV 全子集(TTL/计数器/JSON helper)/ 备份恢复 /
  复制状态 / API Keys / Usage / Credits / Voucher 兑换 / Pricing;
- 错误:稳定 code → 类型化异常(均携带 `requestId`);GET 类请求保守退避重试;
- 内部接口不出现在 SDK 表面。

## 测试

```bash
npm install
npm run typecheck          # tsc 严格模式
npm run test:unit          # 单元(mock fetch)
npm run test:contract      # 真实 Combee server(自动启动)
```

## 示例

`examples/01_create_cell.mjs` … `examples/10_credits.mjs`。

## 发布

```bash
npm run build
npm publish --access public   # npm 发布时执行
```
