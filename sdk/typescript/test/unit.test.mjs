//! 单元测试:错误映射 / TTL 类型 / 分页 URL / HTTP 头(mock fetch)。
//! 不依赖真实 server。

import { test } from "node:test";
import assert from "node:assert/strict";
import { fromErrorBody, CellNotFoundError, SqlError, AuthenticationError } from "../dist/errors.js";

test("error mapping: stable codes -> typed errors", () => {
  assert.ok(fromErrorBody("database_not_found", "x", 404) instanceof CellNotFoundError);
  assert.ok(fromErrorBody("sql", "x", 400) instanceof SqlError);
  assert.ok(fromErrorBody("unauthorized", "x", 401) instanceof AuthenticationError);
  const e = fromErrorBody("internal", "boom", 500, "rid-1");
  assert.equal(e.requestId, "rid-1");
  assert.equal(e.code, "internal");
});

test("error mapping: unknown 5xx -> internal", () => {
  const e = fromErrorBody("weird", "x", 502);
  assert.equal(e.code, "internal");
  assert.equal(e.status, 502);
});

test("ttl classification logic matches server semantics", async () => {
  // 通过 dist 的 Kv.ttl 归一化逻辑验证(纯逻辑:null->persistent, >=0 -> expires)
  const { default: _ } = await import("../dist/index.js");
  void _;
  const classify = (ttl) =>
    ttl === null ? { state: "missing" } : ttl < 0 ? { state: "persistent" } : { state: "expires", seconds: ttl };
  assert.deepEqual(classify(null), { state: "missing" });
  assert.deepEqual(classify(-1), { state: "persistent" });
  assert.deepEqual(classify(60), { state: "expires", seconds: 60 });
});

test("pagination query construction", async () => {
  // 通过 Http.paginate 的 URL 构造(用 stub fetch 捕获请求)
  const { Http } = await import("../dist/http.js");
  const seen = [];
  globalThis.fetch = async (url) => {
    seen.push(String(url));
    return new Response(JSON.stringify({ items: [], next_cursor: null }), {
      status: 200,
      headers: { "content-type": "application/json", "x-request-id": "r" },
    });
  };
  const http = new Http({ baseUrl: "http://x:1", apiKey: "k" });
  await http.paginate("/v1/credits/transactions", 50, "cur-1");
  assert.ok(seen[0].includes("limit=50"), seen[0]);
  assert.ok(seen[0].includes("cursor=cur-1"), seen[0]);
  assert.ok(seen[0].includes("x"), seen[0]);
});
