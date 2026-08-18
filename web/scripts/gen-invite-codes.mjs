#!/usr/bin/env node
// 批量生成单次邀请码(写入 combee-bff cell 的 invite_codes 表)。与 credit voucher 无关。
// 用法:
//   COMBEE_API_URL=https://api.combee.cloud COMBEE_BFF_SERVICE_KEY=cmb_sk_xxx \
//   [COMBEE_BFF_CELL_NAME=combee-bff] node web/scripts/gen-invite-codes.mjs <count>
import { randomBytes } from "node:crypto";

const API = (process.env.COMBEE_API_URL || "http://127.0.0.1:8080").replace(/\/+$/, "");
const KEY = process.env.COMBEE_BFF_SERVICE_KEY || "";
const CELL_NAME = process.env.COMBEE_BFF_CELL_NAME || "combee-bff";
const COUNT = Math.max(1, parseInt(process.argv[2] || "10", 10));
if (!KEY) {
  console.error("需要 COMBEE_BFF_SERVICE_KEY");
  process.exit(1);
}

async function api(path, init = {}) {
  const res = await fetch(`${API}${path}`, {
    ...init,
    headers: { "content-type": "application/json", "x-api-key": KEY, ...(init.headers || {}) },
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${init.method || "GET"} ${path} -> ${res.status}: ${text}`);
  return text ? JSON.parse(text) : undefined;
}
const sql = (cell, statement, params = []) =>
  api(`/v1/databases/${cell}/sql`, { method: "POST", body: JSON.stringify({ sql: statement, params }) });

// CMB-XXXX-XXXX(大写字母+数字,去易混淆)
function code() {
  const A = "ABCDEFGHJKMNPQRSTUVWXYZ23456789";
  const b = randomBytes(8);
  let s = "CMB-";
  for (let i = 0; i < 8; i++) {
    if (i === 4) s += "-";
    s += A[b[i] % A.length];
  }
  return s;
}

async function main() {
  const r = await api(`/v1/databases/by-name/${encodeURIComponent(CELL_NAME)}`, { method: "PUT" });
  const cell = r.cell.id;
  await sql(
    cell,
    "CREATE TABLE IF NOT EXISTS invite_codes (code TEXT PRIMARY KEY, used_by TEXT, used_at INTEGER)",
  ).catch(() => {});
  const out = [];
  for (let i = 0; i < COUNT; i++) {
    const c = code();
    await sql(cell, "INSERT OR IGNORE INTO invite_codes (code) VALUES (?)", [c]);
    out.push(c);
  }
  console.log(`生成 ${out.length} 个邀请码(单次有效):`);
  out.forEach((c) => console.log("  " + c));
}
main().catch((e) => {
  console.error(e);
  process.exit(1);
});
