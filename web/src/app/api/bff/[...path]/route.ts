import { NextRequest, NextResponse } from "next/server";
import { bffContext, bffLog } from "@/lib/bff/context";
import { combeeRequest, CombeeApiError } from "@/lib/combee-client";
import { bffKey, SESSION_COOKIE, destroySession, getSession, login, registerUser, type BffSession } from "@/lib/bff/auth";
import { aggregateOverview } from "@/lib/bff/aggregate";
import {
  appendQueryHistory,
  deleteSnippet,
  getOnboarding,
  getProfile,
  listQueryHistory,
  listRecentCells,
  listSnippets,
  markRecentCell,
  saveSnippet,
  updateProfile,
} from "@/lib/bff/user-data";

// BFF HTTP 端点:Auth / Session / Proxy / Aggregation。
// 路由:/api/bff/[...path](auth/login、auth/logout、auth/session、overview、v1/...)

type Params = { params: Promise<{ path: string[] }> };

function isPath(path: string[], ...segments: string[]): boolean {
  return path.length === segments.length && segments.every((s, i) => path[i] === s);
}

/** POST:login / logout(以及代理写操作由 DELETE 处理,POST 写走 login 分支)。 */
export async function POST(req: NextRequest, { params }: Params) {
  const path = (await params).path;
  return withLog(req, path, () => postInner(req, path));
}

async function postInner(req: NextRequest, path: string[]) {
  if (isPath(path, "auth", "login") || isPath(path, "auth", "register")) {
    const isRegister = isPath(path, "auth", "register");
    let username: string;
    let password: string;
    let accessCode: string | undefined;
    try {
      const body = await req.json();
      username = String(body?.username ?? "").trim();
      password = String(body?.password ?? "");
      accessCode = body?.access_code ? String(body.access_code).trim() : undefined;
    } catch {
      return NextResponse.json({ code: "invalid_request", error: "username and password required" }, { status: 400 });
    }
    if (!username || !password) {
      return NextResponse.json({ code: "invalid_request", error: "username and password required" }, { status: 400 });
    }
    try {
      let apiKey: string | undefined;
      if (isRegister) {
        const reg = await registerUser(username, password, accessCode);
        apiKey = reg.apiKey;
      }
      const sid = await login(username, password);
      const res = NextResponse.json(apiKey ? { ok: true, apiKey } : { ok: true });
      res.cookies.set(SESSION_COOKIE, sid, { httpOnly: true, sameSite: "lax", path: "/", maxAge: 86_400 });
      return res;
    } catch (err) {
      // 注册/登录的业务失败(invalid credentials / username taken / 校验)统一 400
      const isAuthError = err instanceof Error && !(err instanceof CombeeApiError);
      const status = err instanceof CombeeApiError ? err.status : isAuthError ? 400 : 500;
      const code = err instanceof CombeeApiError ? err.code : isAuthError ? "invalid_request" : "internal";
      return NextResponse.json({ code, error: String((err as Error).message) }, { status });
    }
  }

  if (isPath(path, "auth", "logout")) {
    await destroySession(req.cookies.get(SESSION_COOKIE)?.value);
    const res = NextResponse.json({ ok: true });
    res.cookies.set(SESSION_COOKIE, "", { httpOnly: true, path: "/", maxAge: 0 });
    return res;
  }

  if (path[0] === "profile") {
    const session = await getSession(req.cookies.get(SESSION_COOKIE)?.value);
    if (!session) return unauthorized();
    const body = await req.json().catch(() => ({}));
    const profile = await updateProfile(session, body);
    return NextResponse.json({ profile });
  }

  if (path[0] === "snippets") {
    const session = await getSession(req.cookies.get(SESSION_COOKIE)?.value);
    if (!session) return unauthorized();
    const body = (await req.json().catch(() => ({}))) as { title?: string; sql?: string };
    if (!body.title || !body.sql) {
      return NextResponse.json({ code: "invalid_request", error: "title and sql required" }, { status: 400 });
    }
    const snippet = await saveSnippet(session, body.title, body.sql);
    return NextResponse.json({ snippet }, { status: 201 });
  }

  if (path[0] === "recent") {
    const session = await getSession(req.cookies.get(SESSION_COOKIE)?.value);
    if (!session) return unauthorized();
    const body = (await req.json().catch(() => ({}))) as { cell_id?: string };
    if (!body.cell_id) return NextResponse.json({ code: "invalid_request", error: "cell_id required" }, { status: 400 });
    await markRecentCell(session, body.cell_id);
    return NextResponse.json({ ok: true });
  }

  if (path[0] === "history") {
    const session = await getSession(req.cookies.get(SESSION_COOKIE)?.value);
    if (!session) return unauthorized();
    const body = (await req.json().catch(() => ({}))) as { sql?: string };
    if (body.sql) await appendQueryHistory(session, body.sql);
    return NextResponse.json({ ok: true });
  }

  if (path[0] === "v1") {
    return proxy(req, path.join("/"));
  }

  return NextResponse.json({ code: "invalid_request", error: "unknown bff route" }, { status: 400 });
}

function unauthorized() {
  return NextResponse.json({ code: "unauthorized", error: "not authenticated" }, { status: 401 });
}

/** GET:session / overview(聚合)/ v1/*(代理)。 */
export async function GET(req: NextRequest, { params }: Params) {
  const path = (await params).path;
  return withLog(req, path, () => getInner(req, path));
}

async function getInner(req: NextRequest, path: string[]) {
  if (isPath(path, "auth", "session")) {
    const session = await getSession(req.cookies.get(SESSION_COOKIE)?.value);
    if (!session) return NextResponse.json({ authenticated: false });
    return NextResponse.json({ authenticated: true, username: session.username, created_at: session.created_at });
  }

  if (isPath(path, "overview")) {
    const session = await getSession(req.cookies.get(SESSION_COOKIE)?.value);
    if (!session) return NextResponse.json({ code: "unauthorized", error: "not authenticated" }, { status: 401 });
    try {
      return NextResponse.json(await aggregateOverview(session));
    } catch (err) {
      return NextResponse.json({ code: "internal", error: String((err as Error).message) }, { status: 502 });
    }
  }

  if (path[0] === "profile") {
    const session = await getSession(req.cookies.get(SESSION_COOKIE)?.value);
    if (!session) return unauthorized();
    const profile = await getProfile(session);
    const onboarding = await getOnboarding(session);
    const [snippets, recent, history] = await Promise.all([
      listSnippets(session),
      listRecentCells(session),
      listQueryHistory(session),
    ]);
    return NextResponse.json({ profile, onboarding, snippets, recent, history });
  }

  if (path[0] === "snippets" && path.length === 2 && req.method === "DELETE") {
    const session = await getSession(req.cookies.get(SESSION_COOKIE)?.value);
    if (!session) return unauthorized();
    await deleteSnippet(session, path[1]);
    return NextResponse.json({ ok: true });
  }

  if (path[0] === "v1") {
    return proxy(req, path.join("/"));
  }

  return NextResponse.json({ code: "invalid_request", error: "unknown bff route" }, { status: 400 });
}

/** DELETE:v1/* 代理。 */
export async function PUT(req: NextRequest, { params }: Params) {
  const path = (await params).path;
  return withLog(req, path, () => putInner(req, path));
}

async function putInner(req: NextRequest, path: string[]) {
  if (path[0] !== "v1") {
    return NextResponse.json({ code: "invalid_request", error: "unknown bff route" }, { status: 400 });
  }
  return proxy(req, path.join("/"));
}

export async function DELETE(req: NextRequest, { params }: Params) {
  const path = (await params).path;
  return withLog(req, path, () => deleteInner(req, path));
}

async function deleteInner(req: NextRequest, path: string[]) {
  if (path[0] !== "v1") {
    return NextResponse.json({ code: "invalid_request", error: "unknown bff route" }, { status: 400 });
  }
  return proxy(req, path.join("/"));
}

/** 包裹请求:生成/透传 request_id + 结构化 JSON 日志。 */
async function withLog(
  req: NextRequest,
  path: string[],
  handler: () => Promise<NextResponse>,
): Promise<NextResponse> {
  const started = Date.now();
  const requestId = req.headers.get("x-request-id") ?? `req_${crypto.randomUUID().replaceAll("-", "").slice(0, 24)}`;
  return bffContext.run({ request_id: requestId }, async () => {
    let res: NextResponse;
    try {
      res = await handler();
    } catch (err) {
      bffLog("ERROR", { operation: path.join("."), error_code: "BFF_INTERNAL", latency_ms: Date.now() - started });
      return NextResponse.json({ code: "internal", error: String((err as Error).message) }, { status: 500 });
    }
    const status = res.status;
    const latency = Date.now() - started;
    if (status >= 500) {
      bffLog("ERROR", { operation: path.join("."), status, latency_ms: latency, error_code: "HTTP_5XX" });
    } else if (status === 429 || status === 401) {
      bffLog("WARN", { operation: path.join("."), status, latency_ms: latency, error_code: status === 429 ? "QUOTA_EXCEEDED" : "AUTH_FAILED" });
    } else if (process.env.COMBEE_BFF_DEBUG === "1") {
      bffLog("DEBUG", { operation: path.join("."), status, latency_ms: latency });
    }
    return res;
  });
}

/** cell 归属校验:该 cell 必须属于当前用户租户(admin key 能看到全部,BFF 负责隔离)。
 *  返回 true 才允许代理执行 SQL/KV/备份等操作。 */
async function cellBelongsTo(session: BffSession, cellId: string): Promise<boolean> {
  if (!session.tenant_id) return false;
  const cell = await combeeRequest<{ tenant_id?: string }>(`/v1/databases/${cellId}`, {
    apiKey: bffKey(),
  }).catch(() => null);
  return !!cell && cell.tenant_id === session.tenant_id;
}

async function proxy(req: NextRequest, target: string) {
  const session = await getSession(req.cookies.get(SESSION_COOKIE)?.value);
  if (!session) {
    return NextResponse.json({ code: "unauthorized", error: "not authenticated" }, { status: 401 });
  }
  // 归属校验:cell 数据操作(/v1/databases/{id}/…)必须属于当前用户;
  // by-name 与列表/创建(无 cell id)不走校验。
  const m = target.match(/^v1\/databases\/([^/]+)/);
  if (m && m[1] !== "by-name") {
    const cellId = m[1];
    if (!(await cellBelongsTo(session, cellId))) {
      return NextResponse.json({ code: "database_not_found", error: "database not found" }, { status: 404 });
    }
  }
  const query = req.nextUrl.search;
  const method = req.method;
  // 列表请求(GET /v1/databases,无 cell id):admin key 拉全量 → 按用户租户过滤。
  // 这是 cells 列表页的入口;不做过滤会把平台 cell(combee-bff)与所有租户 cell 暴露给用户。
  if (method === "GET" && (target === "v1/databases" || target === "v1/databases/")) {
    // 用服务 key(不计费)+ 显式 tenant_id 让平台按租户过滤;不再拉全量到 BFF。
    // session.tenant_id 缺失(极老用户未回填)→ 直接返回空,绝不泄露。
    if (!session.tenant_id) return NextResponse.json([]);
    const qs = new URLSearchParams(req.nextUrl.searchParams);
    qs.set("tenant_id", session.tenant_id);
    const scoped = await combeeRequest<Array<{ tenant_id?: string }>>(
      `/v1/databases?${qs.toString()}`,
      { apiKey: bffKey() },
    ).catch(() => []);
    const list = Array.isArray(scoped) ? scoped : [];
    // 纵深防御:平台已按租户过滤,这里再按 tenant_id 兜底过滤一次。
    const filtered = list.filter((c) => c.tenant_id === session.tenant_id);
    return NextResponse.json(filtered);
  }
  let body: unknown;
  if (method !== "GET" && method !== "DELETE") {
    try {
      body = await req.json();
    } catch {
      body = undefined;
    }
  }
  // 创建 Cell(by-name ensure)是用户资源行为:用用户 key,使 Cell 归属用户租户
  // (admin key 创建会归到平台租户,导致归属隔离失效)。其余 console 操作统一
  // admin key 代理(不计费),权限已在上方校验。
  const isCellCreate =
    method === "PUT" && target.match(/^v1\/databases\/by-name\//) !== null;
  // credits/usage 查询是"用户自己的数据":用用户 key(租户聚合正确;admin key 会
  // 查到平台租户的余额/用量,导致 credits 页面显示 0)。
  const isUserScopedQuery =
    method === "GET" &&
    (/^v1\/(credits|usage)\//.test(target) || /^v1\/cells\/[^/]+\/usage/.test(target));
  const apiKey = isCellCreate || isUserScopedQuery ? session.api_key : bffKey();
  try {
    const data = await combeeRequest(`/${target}${query}`, { method, body, apiKey });
    if (data === undefined) return NextResponse.json({ ok: true });
    return NextResponse.json(data);
  } catch (err) {
    const status = err instanceof CombeeApiError ? err.status : 500;
    const code = err instanceof CombeeApiError ? err.code : "internal";
    return NextResponse.json({ code, error: String((err as Error).message) }, { status });
  }
}
