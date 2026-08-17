#!/usr/bin/env node
// 为存量 console_users 行回填 uid(稳定 UUID,与 username 解耦)。
// 幂等:只刷 uid 为空的行,可安全重跑;--dry-run 只列不写。
//
// 用法(在能访问 API 的机器上跑,如服务器):
//   COMBEE_API_URL=https://api.combee.cloud \
//   COMBEE_BFF_SERVICE_KEY=cmb_sk_xxx \
//   [COMBEE_BFF_CELL_NAME=combee-bff] [COMBEE_BFF_CELL=<cell-id>] \
//   node web/scripts/backfill-console-uid.mjs [--dry-run]
import { randomUUID } from "node:crypto";

const API = (process.env.COMBEE_API_URL || "http://127.0.0.1:8080").replace(/\/+$/, "");
const KEY = process.env.COMBEE_BFF_SERVICE_KEY || "";
const CELL_ID = process.env.COMBEE_BFF_CELL || "";
const CELL_NAME = process.env.COMBEE_BFF_CELL_NAME || "combee-bff";
const DRY = process.argv.includes("--dry-run");

if (!KEY) {
  console.error("错误:需要 COMBEE_BFF_SERVICE_KEY(BFF 服务账号 key)");
  process.exit(1);
}

async function api(path, init = {}) {
  const res = await fetch(`${API}${path}`, {
    ...init,
    headers: { "content-type": "application/json", "x-api-key": KEY, ...(init.headers || {}) },
  });
  const text = await res.text();
  let body;
  try {
    body = text ? JSON.parse(text) : undefined;
  } catch {
    body = text;
  }
  if (!res.ok) throw new Error(`${init.method || "GET"} ${path} -> ${res.status}: ${text}`);
  return body;
}

const sql = (cell, statement, params = []) =>
  api(`/v1/databases/${cell}/sql`, { method: "POST", body: JSON.stringify({ sql: statement, params }) });

async function resolveCell() {
  if (CELL_ID) return CELL_ID;
  const r = await api(`/v1/databases/by-name/${encodeURIComponent(CELL_NAME)}`, { method: "PUT" });
  return r.cell.id;
}

async function main() {
  const cell = await resolveCell();
  console.log(`cell=${cell}  api=${API}  dry-run=${DRY}`);

  // 幂等确保列 + 唯一索引(已存在则忽略)
  await sql(cell, "ALTER TABLE console_users ADD COLUMN uid TEXT").catch(() => {});
  await sql(
    cell,
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_console_users_uid ON console_users(uid)",
  ).catch(() => {});

  const res = await sql(cell, "SELECT username FROM console_users WHERE uid IS NULL OR uid = ''");
  const rows = res?.rows ?? [];
  console.log(`待回填用户数:${rows.length}`);

  let done = 0;
  for (const row of rows) {
    const username = row[0];
    const uid = randomUUID();
    if (DRY) {
      console.log(`  [dry] ${username} <- ${uid}`);
      continue;
    }
    // 幂等写:仅当仍为空时才更新(并发/重跑安全)
    await sql(
      cell,
      "UPDATE console_users SET uid = ? WHERE username = ? AND (uid IS NULL OR uid = '')",
      [uid, username],
    );
    console.log(`  ${username} <- ${uid}`);
    done++;
  }

  console.log(DRY ? "dry-run 完成(未写入)" : `完成,回填 ${done} 个用户`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
