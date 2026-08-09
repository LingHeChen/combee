import "server-only";

// BFF Auth/Session(Console 用户体系)。
// 登录 = 用户名 + 密码(不再是 API key —— Console 是用来签发 API key 的,
// 用 key 登录是死锁)。用户凭据与会话全部存储在 Combee 自身:
// - console_users 表:Combee SQL(Session Cell 内)
// - 会话:Combee KV(bff:session:{sid},TTL 24h)
// 注册时由 BFF 服务账号(COMBEE_BFF_API_KEY)为用户创建专属 Combee key,
// 之后代理请求使用该用户的 key(租户隔离在 Combee 侧保持)。

import { randomUUID, scryptSync, timingSafeEqual } from "node:crypto";
import { cookies, headers } from "next/headers";
import { combeeRequest } from "@/lib/combee-client";

const COOKIE = "combee_session";
const SESSION_TTL = 86_400; // 24h

export interface BffSession {
  /** console 用户名 */
  username: string;
  /** 该用户专属的 Combee API key(代理请求用) */
  api_key: string;
  created_at: number;
}

async function ensureSessionCell(): Promise<string> {
  // ensure 语义:固定名会话 Cell,幂等复用(同名不再创建)
  if (process.env.COMBEE_BFF_CELL) return process.env.COMBEE_BFF_CELL;
  const name = process.env.COMBEE_BFF_CELL_NAME ?? "combee-bff";
  const r = await combeeRequest<{ cell: { id: string } }>(
    `/v1/databases/by-name/${encodeURIComponent(name)}`,
    { method: "PUT", apiKey: bffKey() },
  );
  return r.cell.id;
}

/** BFF 服务账号 key:dev(auth=off)可空;生产必填(用于建表/建用户 key)。 */
function bffKey(): string {
  return process.env.COMBEE_BFF_API_KEY ?? "";
}

function sessionKey(sid: string): string {
  return `bff:session:${sid}`;
}

// ---- 密码哈希(scrypt,格式 salt:hash)----

const SCRYPT_KEYLEN = 64;

function hashPassword(password: string): string {
  const salt = randomUUID().replaceAll("-", "").slice(0, 16);
  const hash = scryptSync(password, salt, SCRYPT_KEYLEN).toString("hex");
  return `${salt}:${hash}`;
}

function verifyPassword(password: string, stored: string): boolean {
  const [salt, hash] = stored.split(":");
  if (!salt || !hash) return false;
  const candidate = scryptSync(password, salt, SCRYPT_KEYLEN);
  const expected = Buffer.from(hash, "hex");
  return candidate.length === expected.length && timingSafeEqual(candidate, expected);
}

// ---- console_users 表(Combee SQL)----

interface ConsoleUserRow {
  username: string;
  password_hash: string;
  api_key: string;
  created_at: number;
}

const CREATE_TABLE_SQL = `CREATE TABLE IF NOT EXISTS console_users (
  username TEXT PRIMARY KEY,
  password_hash TEXT NOT NULL,
  api_key TEXT NOT NULL,
  created_at INTEGER NOT NULL
)`;

export async function ensureUsersTable(): Promise<string> {
  const cell = await ensureSessionCell();
  await combeeRequest(`/v1/databases/${cell}/sql`, {
    method: "POST",
    body: { sql: CREATE_TABLE_SQL },
    apiKey: bffKey(),
  }).catch(() => undefined);
  return cell;
}

async function findUser(username: string): Promise<ConsoleUserRow | null> {
  const cell = await ensureUsersTable();
  const res = await combeeRequest<{ rows: unknown[][] }>(
    `/v1/databases/${cell}/sql`,
    {
      method: "POST",
      body: {
        sql: "SELECT username, password_hash, api_key, created_at FROM console_users WHERE username = ?",
        params: [username],
      },
      apiKey: bffKey(),
    },
  ).catch(() => null);
  const rows = res?.rows ?? [];
  if (rows.length === 0) return null;
  const r = rows[0] as [string, string, string, number];
  return { username: r[0], password_hash: r[1], api_key: r[2], created_at: r[3] };
}

function normalizeUsername(raw: string): string {
  return raw.trim().toLowerCase();
}

// ---- 公开 API ----

/** 注册 console 用户:建用户专属 Combee key → 存 console_users → 返回用户。 */
/** Signup 模式:off = 关闭;code = 必须 Alpha access code(默认);open = 任意注册。 */
export function signupMode(): "off" | "code" | "open" {
  const v = (process.env.COMBEE_CONSOLE_SIGNUP ?? "code").toLowerCase();
  if (v === "false" || v === "off" || v === "0") return "off";
  if (v === "open" || v === "true") return "open";
  return "code";
}

export async function registerUser(
  rawUsername: string,
  password: string,
  accessCode?: string,
): Promise<{ row: ConsoleUserRow; apiKey: string }> {
  const username = normalizeUsername(rawUsername);
  if (!/^[a-z0-9._-]{3,32}$/.test(username)) {
    throw new Error("username must be 3-32 chars: a-z, 0-9, . _ -");
  }
  if (password.length < 8) {
    throw new Error("password must be at least 8 characters");
  }
  const mode = signupMode();
  if (mode === "off") {
    throw new Error("signup is disabled (invite only)");
  }
  if (mode === "code" && !accessCode?.trim()) {
    throw new Error("an Alpha access code is required to sign up");
  }
  const existing = await findUser(username);
  if (existing) throw new Error("username already taken");

  // 为用户创建专属 Combee key(服务账号;dev off 模式 bffKey 可空)
  const created = await combeeRequest<{ tenant_id: string; key: string; key_id: string }>(
    "/admin/tenants",
    { method: "POST", body: {}, apiKey: bffKey() },
  )
  // Closed Alpha:兑换邀请码(voucher)→ 用户获得初始 Alpha Credits(默认 1000)
  if (mode === "code") {
    try {
      try { require("node:fs").appendFileSync("/tmp/auth-debug.log", `${new Date().toISOString()} key-len=${created?.key?.length ?? -1}\n`); } catch {}
      const r = await combeeRequest<{ credits_added: string; already_redeemed: boolean }>(
        "/v1/credits/redeem",
        { method: "POST", body: { code: accessCode!.trim() }, apiKey: created.key },
      );
      if (r.already_redeemed) {
        throw new Error("Alpha access code already used by another account");
      }
    } catch (err) {
      try { require("node:fs").appendFileSync("/tmp/auth-debug.log", `${new Date().toISOString()} redeem-err msg=${(err as Error).message} code=${(err as { code?: string }).code} status=${(err as { status?: number }).status}\n`); } catch {}
      // 邀请码无效/已用:清理刚建的用户,报错
      const cell = await ensureUsersTable();
      await combeeRequest(`/v1/databases/${cell}/sql`, {
        method: "POST",
        body: { sql: "DELETE FROM console_users WHERE username = ?", params: [username] },
        apiKey: bffKey(),
      }).catch(() => undefined);
      await combeeRequest(`/v1/api-keys/${created.key_id}`, {
        method: "DELETE",
        apiKey: bffKey(),
      }).catch(() => undefined);
      throw new Error(`invalid or already-used Alpha access code: ${(err as Error).message}`);
    }
  }

  const cell = await ensureUsersTable();
  await combeeRequest(`/v1/databases/${cell}/sql`, {
    method: "POST",
    body: {
      sql: "INSERT INTO console_users (username, password_hash, api_key, created_at) VALUES (?, ?, ?, ?)",
      params: [username, hashPassword(password), created.key, Math.floor(Date.now() / 1000)],
    },
    apiKey: bffKey(),
  });

  const row: ConsoleUserRow = {
    username,
    password_hash: "",
    api_key: created.key,
    created_at: Math.floor(Date.now() / 1000),
  };
  return { row, apiKey: created.key };
}

/** 登录:校验用户名密码 → 会话写入 Combee KV → 返回 sid。 */
export async function login(rawUsername: string, password: string): Promise<string> {
  const username = normalizeUsername(rawUsername);
  const user = await findUser(username);
  if (!user || !verifyPassword(password, user.password_hash)) {
    throw new Error("invalid username or password");
  }
  const sid = randomUUID();
  const session: BffSession = {
    username: user.username,
    api_key: user.api_key,
    created_at: Math.floor(Date.now() / 1000),
  };
  const cell = await ensureSessionCell();
  await combeeRequest(`/v1/databases/${cell}/kv/${encodeURIComponent(sessionKey(sid))}`, {
    method: "PUT",
    body: { value: JSON.stringify(session), ttl_seconds: SESSION_TTL },
    apiKey: bffKey(),
  });
  return sid;
}

/** 读取会话(Combee KV;TTL 惰性过期)。 */
export async function getSession(sid: string | undefined): Promise<BffSession | null> {
  if (!sid) return null;
  try {
    const cell = await ensureSessionCell();
    const raw = await combeeRequest<{ value?: string }>(
      `/v1/databases/${cell}/kv/${encodeURIComponent(sessionKey(sid))}`,
      { apiKey: bffKey() },
    );
    if (!raw?.value) return null;
    return JSON.parse(raw.value) as BffSession;
  } catch {
    return null;
  }
}

/** 销毁会话。 */
export async function destroySession(sid: string | undefined): Promise<void> {
  if (!sid) return;
  try {
    const cell = await ensureSessionCell();
    await combeeRequest(`/v1/databases/${cell}/kv/${encodeURIComponent(sessionKey(sid))}`, {
      method: "DELETE",
      apiKey: bffKey(),
    });
  } catch {
    /* 忽略 */
  }
}

/** 从请求 cookie 取会话(server 端;headers() 解析)。 */
export async function sessionFromCookies(): Promise<BffSession | null> {
  const hdrs = await headers();
  const raw = hdrs.get("cookie") ?? "";
  const match = raw.match(new RegExp(`(?:^|;\\s*)${COOKIE}=([^;]+)`));
  return getSession(match?.[1]);
}

export const SESSION_COOKIE = COOKIE;
