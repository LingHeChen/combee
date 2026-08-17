import "server-only";

// 用户辅助数据(全部存 Combee —— Session Cell 的 SQL):
// - console_profiles    :Profile(display_name/avatar/locale/timezone)+ Preferences(JSON)+ dashboard prefs(JSON)
// - console_snippets    :保存的 SQL snippets
// - console_recent_cells:最近访问的 Cells
// - console_query_history:查询历史(仅存截断 SQL,**不含参数** —— 避免敏感数据)

import { randomUUID } from "node:crypto";
import { combeeRequest } from "@/lib/combee-client";
import { bffKey, ensureUsersTable, type BffSession } from "@/lib/bff/auth";

// ---- Schema(逐条建表:Combee 拒绝多语句)----

const TABLES: string[] = [
  `CREATE TABLE IF NOT EXISTS console_profiles (
    username TEXT PRIMARY KEY,
    display_name TEXT,
    avatar TEXT,
    locale TEXT,
    timezone TEXT,
    prefs TEXT NOT NULL DEFAULT '{}',
    dash_prefs TEXT NOT NULL DEFAULT '{}'
  )`,
  `CREATE TABLE IF NOT EXISTS console_snippets (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL,
    title TEXT NOT NULL,
    sql TEXT NOT NULL,
    created_at INTEGER NOT NULL
  )`,
  `CREATE TABLE IF NOT EXISTS console_recent_cells (
    username TEXT NOT NULL,
    cell_id TEXT NOT NULL,
    last_visited INTEGER NOT NULL,
    PRIMARY KEY (username, cell_id)
  )`,
  `CREATE TABLE IF NOT EXISTS console_query_history (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL,
    sql_truncated TEXT NOT NULL,
    created_at INTEGER NOT NULL
  )`,
];

async function ensureSchema(): Promise<string> {
  const cell = await ensureUsersTable();
  for (const ddl of TABLES) {
    await combeeRequest(`/v1/databases/${cell}/sql`, {
      method: "POST",
      body: { sql: ddl },
      apiKey: cellApiKey(),
    }).catch(() => undefined);
  }
  return cell;
}

function cellApiKey(): string {
  return process.env.COMBEE_BFF_API_KEY ?? "";
}

async function sql(cell: string, query: string, params?: unknown[]): Promise<unknown[][]> {
  const res = await combeeRequest<{ rows?: unknown[][] }>(`/v1/databases/${cell}/sql`, {
    method: "POST",
    body: { sql: query, params },
    apiKey: cellApiKey(),
  });
  return res?.rows ?? [];
}

function nowSec(): number {
  return Math.floor(Date.now() / 1000);
}

// ---- Profile & Preferences ----

export interface UserProfile {
  username: string;
  display_name: string | null;
  avatar: string | null;
  locale: string;
  timezone: string;
  prefs: {
    default_range: string;
    default_region: string;
    table_page_size: number;
    ui: Record<string, unknown>;
  };
}

const DEFAULT_PREFS = {
  default_range: "30D",
  default_region: "auto",
  table_page_size: 25,
  ui: {},
};

export async function getProfile(session: BffSession): Promise<UserProfile> {
  const cell = await ensureSchema();
  const rows = await sql(
    cell,
    "SELECT display_name, avatar, locale, timezone, prefs FROM console_profiles WHERE username = ?",
    [session.username],
  );
  if (rows.length === 0) {
    return {
      username: session.username,
      display_name: null,
      avatar: null,
      locale: "en-US",
      timezone: "UTC",
      prefs: DEFAULT_PREFS,
    };
  }
  const [displayName, avatar, locale, timezone, prefsRaw] = rows[0] as [string | null, string | null, string, string, string];
  let prefs = DEFAULT_PREFS;
  try {
    const parsed = JSON.parse(prefsRaw) as Partial<UserProfile["prefs"]>;
    prefs = { ...DEFAULT_PREFS, ...parsed, ui: { ...DEFAULT_PREFS.ui, ...(parsed.ui ?? {}) } };
  } catch {
    /* 保持默认 */
  }
  return {
    username: session.username,
    display_name: displayName,
    avatar,
    locale: locale || "en-US",
    timezone: timezone || "UTC",
    prefs,
  };
}

export async function updateProfile(
  session: BffSession,
  patch: {
    display_name?: string | null;
    avatar?: string | null;
    locale?: string;
    timezone?: string;
    prefs?: Partial<UserProfile["prefs"]>;
  },
): Promise<UserProfile> {
  const cell = await ensureSchema();
  const current = await getProfile(session);
  const next: UserProfile = {
    username: session.username,
    display_name: patch.display_name !== undefined ? patch.display_name : current.display_name,
    avatar: patch.avatar !== undefined ? patch.avatar : current.avatar,
    locale: patch.locale ?? current.locale,
    timezone: patch.timezone ?? current.timezone,
    prefs: {
      ...current.prefs,
      ...(patch.prefs ?? {}),
      ui: { ...current.prefs.ui, ...(patch.prefs?.ui ?? {}) },
    },
  };
  await sql(
    cell,
    `INSERT INTO console_profiles (username, display_name, avatar, locale, timezone, prefs, dash_prefs)
     VALUES (?, ?, ?, ?, ?, ?, '{}')
     ON CONFLICT (username) DO UPDATE SET
       display_name = excluded.display_name,
       avatar = excluded.avatar,
       locale = excluded.locale,
       timezone = excluded.timezone,
       prefs = excluded.prefs`,
    [session.username, next.display_name, next.avatar, next.locale, next.timezone, JSON.stringify(next.prefs)],
  );
  return next;
}

// ---- Onboarding(从 Combee 数据推断)----

export interface OnboardingState {
  first_cell_created: boolean;
  api_key_created: boolean;
  first_request_made: boolean;
  completed: boolean;
  completed_at: number | null;
}

export async function getOnboarding(session: BffSession): Promise<OnboardingState> {
  // cells 用平台 key + 显式 tenant_id 让平台按租户过滤(不再拉全量到 BFF);
  // 注册即创建了用户 key(api_key_created 恒为注册状态);usage 用用户 key 查(不计费)。
  const [cells, usage] = await Promise.all([
    session.tenant_id
      ? combeeRequest<Array<{ tenant_id?: string }> | null>(
          `/v1/databases?limit=1000&tenant_id=${session.tenant_id}`,
          { apiKey: bffKey() },
        ).catch(() => null)
      : Promise.resolve(null),
    combeeRequest<{ request_count?: number } | null>("/v1/usage/summary", {
      apiKey: session.api_key,
    }).catch(() => null),
  ]);
  const firstCell = Array.isArray(cells)
    ? cells.some((c) => !!session.tenant_id && c.tenant_id === session.tenant_id)
    : false;
  const firstKey = true; // 注册流程必然为用户创建了专属 API key
  const firstRequest = (usage?.request_count ?? 0) > 0;
  const completed = firstCell && firstKey && firstRequest;
  return {
    first_cell_created: firstCell,
    api_key_created: firstKey,
    first_request_made: firstRequest,
    completed,
    completed_at: completed ? nowSec() : null,
  };
}

// ---- Snippets ----

export interface Snippet {
  id: string;
  title: string;
  sql: string;
  created_at: number;
}

export async function listSnippets(session: BffSession): Promise<Snippet[]> {
  const cell = await ensureSchema();
  const rows = await sql(
    cell,
    "SELECT id, title, sql, created_at FROM console_snippets WHERE username = ? ORDER BY created_at DESC LIMIT 200",
    [session.username],
  );
  return rows.map((r) => ({ id: r[0] as string, title: r[1] as string, sql: r[2] as string, created_at: r[3] as number }));
}

export async function saveSnippet(session: BffSession, title: string, snippetSql: string): Promise<Snippet> {
  const cell = await ensureSchema();
  const snippet: Snippet = { id: randomUUID(), title: title.slice(0, 100), sql: snippetSql.slice(0, 10_000), created_at: nowSec() };
  await sql(
    cell,
    "INSERT INTO console_snippets (id, username, title, sql, created_at) VALUES (?, ?, ?, ?, ?)",
    [snippet.id, session.username, snippet.title, snippet.sql, snippet.created_at],
  );
  return snippet;
}

export async function deleteSnippet(session: BffSession, id: string): Promise<void> {
  const cell = await ensureSchema();
  await sql(cell, "DELETE FROM console_snippets WHERE id = ? AND username = ?", [id, session.username]);
}

// ---- Recent Cells ----

export async function markRecentCell(session: BffSession, cellId: string): Promise<void> {
  const cell = await ensureSchema();
  await sql(
    cell,
    `INSERT INTO console_recent_cells (username, cell_id, last_visited) VALUES (?, ?, ?)
     ON CONFLICT (username, cell_id) DO UPDATE SET last_visited = excluded.last_visited`,
    [session.username, cellId, nowSec()],
  );
}

export async function listRecentCells(session: BffSession, limit = 10): Promise<{ cell_id: string; last_visited: number }[]> {
  const cell = await ensureSchema();
  const rows = await sql(
    cell,
    "SELECT cell_id, last_visited FROM console_recent_cells WHERE username = ? ORDER BY last_visited DESC LIMIT ?",
    [session.username, limit],
  );
  return rows.map((r) => ({ cell_id: r[0] as string, last_visited: r[1] as number }));
}

// ---- Query History(截断 SQL,不含参数)----

export async function appendQueryHistory(session: BffSession, querySql: string): Promise<void> {
  const cell = await ensureSchema();
  const truncated = querySql.trim().replace(/\s+/g, " ").slice(0, 200);
  if (!truncated) return;
  await sql(
    cell,
    "INSERT INTO console_query_history (id, username, sql_truncated, created_at) VALUES (?, ?, ?, ?)",
    [randomUUID(), session.username, truncated, nowSec()],
  );
}

export async function listQueryHistory(session: BffSession, limit = 20): Promise<{ sql: string; created_at: number }[]> {
  const cell = await ensureSchema();
  const rows = await sql(
    cell,
    "SELECT sql_truncated, created_at FROM console_query_history WHERE username = ? ORDER BY created_at DESC LIMIT ?",
    [session.username, limit],
  );
  return rows.map((r) => ({ sql: r[0] as string, created_at: r[1] as number }));
}
