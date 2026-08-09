"use client";

// BFF 浏览器端 helper:所有页面数据经 BFF(代理/聚合)。
// 浏览器只持有 httpOnly 会话 cookie,不直接触碰 Combee API。
// 401 处理策略:先确认会话是否真的失效——仅当 `/api/bff/auth/session`
// 返回未认证时才跳登录;否则 401 是接口级业务错误,抛错展示、绝不误跳登录页。

async function sessionAlive(): Promise<boolean> {
  try {
    const r = await fetch("/api/bff/auth/session", { cache: "no-store" });
    if (!r.ok) return false;
    const body = (await r.json()) as { authenticated?: boolean };
    return body.authenticated === true;
  } catch {
    return false;
  }
}

function gotoLogin() {
  if (typeof window === "undefined") return;
  // 保留当前语言前缀(middleware 也会兜底,但直接带前缀更稳)
  const locale = window.location.pathname.match(/^\/(zh|en)\//)?.[1] ?? "";
  window.location.href = `${locale ? `/${locale}` : ""}/login`;
}

export async function bffFetch<T = unknown>(
  path: string,
  opts: { method?: string; body?: unknown } = {},
): Promise<T> {
  const res = await fetch(`/api/bff${path}`, {
    method: opts.method ?? "GET",
    headers: opts.body === undefined ? undefined : { "content-type": "application/json" },
    body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
  });
  if (res.status === 401) {
    // 会话真的失效才跳登录;否则把 401 当作接口错误抛给调用方展示
    if (await sessionAlive()) {
      throw new Error(`HTTP 401 ${path}`);
    }
    gotoLogin();
    throw new Error("not authenticated");
  }
  if (!res.ok) {
    let message = `HTTP ${res.status}`;
    try {
      const body = (await res.json()) as { error?: string };
      if (body.error) message = body.error;
    } catch {
      /* ignore */
    }
    throw new Error(message);
  }
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  return (text ? JSON.parse(text) : undefined) as T;
}
