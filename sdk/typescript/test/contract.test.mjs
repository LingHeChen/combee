//! Contract tests:对真实 Combee server(dev 模式)执行 SDK 全功能矩阵。
//! 运行:`npm run test:contract`(自动启动 server,结束清理)。

import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { Combee, CellNotFoundError, SqlError } from "../dist/index.js";
import { startServer, BASE_URL } from "./helpers.mjs";

let server;
let combee;

before(async () => {
  server = await startServer();
  combee = new Combee({ baseUrl: BASE_URL, apiKey: "dev-key" });
});

after(() => {
  server?.kill("SIGKILL");
});

test("cell CRUD: create / list / get / delete", async () => {
  const cell = await combee.cells.create({ name: "contract" });
  assert.ok(cell.id);

  const listed = await combee.cells.list();
  assert.ok(listed.items.some((c) => c.id === cell.id));

  const info = await combee.cells.get(cell.id);
  assert.equal(info.id, cell.id);

  await cell.delete();
  const listed2 = await combee.cells.list();
  assert.ok(!listed2.items.some((c) => c.id === cell.id), "删除后不存在");
});

test("sql: execute / query / transaction / rollback", async () => {
  const cell = await combee.cells.create();
  await cell.sql.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)");
  const ins = await cell.sql.execute("INSERT INTO users (name) VALUES (?)", ["Alice"]);
  assert.equal(ins.rows_affected, 1);

  const q = await cell.sql.query("SELECT id, name FROM users WHERE id = ?", [1]);
  assert.deepEqual(q.columns, ["id", "name"]);
  assert.equal(q.rows.length, 1);
  assert.equal(q.rows[0].name, "Alice");

  // 事务原子性:第二条失败 → 第一条也回滚
  await assert.rejects(
    () =>
      cell.sql.transaction([
        { sql: "INSERT INTO users (name) VALUES (?)", params: ["Bob"] },
        { sql: "INSERT INTO nope_table VALUES (1)" },
      ]),
    (e) => e instanceof SqlError,
  );
  const after = await cell.sql.query("SELECT COUNT(*) AS n FROM users");
  assert.equal(after.rows[0].n, 1, "事务回滚,Alice 仍唯一");
});

test("kv: full subset (get/set/ttl/mset/mget/incr)", async () => {
  const cell = await combee.cells.create();
  assert.equal(await cell.kv.get("missing"), null);

  assert.equal(await cell.kv.set("greeting", "hello", { ttl: 3600 }), true);
  assert.equal(await cell.kv.get("greeting"), "hello");
  assert.deepEqual(await cell.kv.ttl("greeting"), { state: "expires", seconds: 3600 });

  // persist → persistent
  await cell.kv.persist("greeting");
  assert.deepEqual(await cell.kv.ttl("greeting"), { state: "persistent" });

  // mset / mget 保序
  await cell.kv.mset({ a: "1", b: "2" });
  const vals = await cell.kv.mget(["a", "b", "nope"]);
  assert.deepEqual(vals, ["1", "2", null]);

  // nx 条件
  assert.equal(await cell.kv.set("a", "overwrite", { condition: "nx" }), false);
  assert.equal(await cell.kv.get("a"), "1");

  // counter
  assert.equal(await cell.kv.incr("pageviews"), 1);
  assert.equal(await cell.kv.incr("pageviews", 5), 6);
  assert.equal(await cell.kv.decr("pageviews", 2), 4);

  // delete / exists
  assert.equal(await cell.kv.exists("greeting"), true);
  assert.equal(await cell.kv.delete("greeting"), true);
  assert.equal(await cell.kv.exists("greeting"), false);
});

test("api keys lifecycle", async () => {
  const created = await combee.apiKeys.create("prod");
  assert.ok(created.id);
  assert.ok(created.key.startsWith("cmb_sk_"), "明文仅返回一次");
  const keys = await combee.apiKeys.list();
  assert.ok(keys.some((k) => k.id === created.id));
  await combee.apiKeys.revoke(created.id);
  const keys2 = await combee.apiKeys.list();
  assert.ok(!keys2.some((k) => k.id === created.id && !k.revoked_at), "撤销生效");
});

test("usage summary / timeseries / cell usage", async () => {
  const cell = await combee.cells.create();
  await cell.kv.set("u", "1");
  await cell.sql.execute("CREATE TABLE t (x INTEGER)");

  // 等待 usage flush 周期(1s)把内存计数写入 metadata
  await new Promise((r) => setTimeout(r, 1500));
  const summary = await combee.usage.summary();
  assert.ok(summary.request_count >= 3, `request_count >= 3, got ${summary.request_count}`);
  assert.ok(summary.operations.kv_writes >= 1);
  assert.ok(summary.operations.sql_writes >= 1);

  const cellUsage = await combee.usage.cell(cell.id);
  assert.ok(cellUsage.current_storage_bytes >= 0);

  const ts = await combee.usage.timeseries({ metric: "requests", interval: "minute" });
  assert.ok(Array.isArray(ts) && ts.length >= 0);
});

test("credits balance + pricing", async () => {
  const balance = await combee.credits.balance();
  assert.equal(balance.currency, "CREDIT");
  assert.ok(/^\d+$/.test(balance.available), "整数 microcredits");

  const txn = await combee.credits.transactions(10);
  assert.ok(Array.isArray(txn.items));

  const pricing = await combee.pricing.get();
  assert.ok("version" in pricing);
});

test("errors: cell not found / sql error carry requestId", async () => {
  await assert.rejects(
    () => combee.cells.get("00000000-0000-0000-0000-000000000000"),
    (e) => e instanceof CellNotFoundError && e.code === "database_not_found",
  );
  const cell = await combee.cells.create();
  await assert.rejects(
    () => cell.sql.execute("THIS IS NOT SQL"),
    (e) => e instanceof SqlError && typeof e.requestId === "string",
  );
});
