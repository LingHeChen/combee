//! 核心 HTTP 客户端:认证、request-id、timeout、保守重试、错误映射、分页。

import { CombeeError, fromErrorBody } from "./errors.js";
import type { Page } from "./types.js";

export interface CombeeOptions {
  baseUrl: string;
  apiKey: string;
  timeoutMs?: number;
  userAgent?: string;
  retry?: {
    maxAttempts?: number;
    baseDelayMs?: number;
    maxDelayMs?: number;
  };
}

interface RetryPolicy {
  maxAttempts: number;
  baseDelayMs: number;
  maxDelayMs: number;
}

const DEFAULTS = {
  timeoutMs: 30_000,
  userAgent: "combee-js/0.1.0-alpha.1",
  retry: { maxAttempts: 3, baseDelayMs: 100, maxDelayMs: 2_000 },
};

export type HttpMethod = "GET" | "POST" | "PUT" | "DELETE";

/** 内部请求执行器(SDK 各资源共用)。 */
export class Http {
  private baseUrl: string;
  private apiKey: string;
  private timeoutMs: number;
  private userAgent: string;
  private retry: RetryPolicy;

  constructor(opts: CombeeOptions) {
    this.baseUrl = opts.baseUrl.replace(/\/+$/, "");
    this.apiKey = opts.apiKey;
    this.timeoutMs = opts.timeoutMs ?? DEFAULTS.timeoutMs;
    this.userAgent = opts.userAgent ?? DEFAULTS.userAgent;
    this.retry = { ...DEFAULTS.retry, ...opts.retry };
  }

  /** 可安全自动重试的方法(GET / 读类)。 */
  private isSafeRetry(method: HttpMethod, path: string): boolean {
    if (method === "GET") return true;
    // 幂等读类 POST(带明确参数查询)保守不重试;写操作一律不自动重试
    return false;
  }

  private async attempt<T>(
    method: HttpMethod,
    path: string,
    body?: unknown,
    idempotencyKey?: string,
  ): Promise<T> {
    const url = `${this.baseUrl}${path}`;
    const requestId =
      typeof crypto !== "undefined" && "randomUUID" in crypto
        ? crypto.randomUUID()
        : `sdk-${Math.random().toString(36).slice(2)}`;
    const headers: Record<string, string> = {
      "content-type": "application/json",
      "x-api-key": this.apiKey,
      "x-request-id": requestId,
      "user-agent": this.userAgent,
    };
    if (idempotencyKey) headers["idempotency-key"] = idempotencyKey;

    const resp = await fetch(url, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(this.timeoutMs),
    });
    const responseRequestId =
      resp.headers.get("x-request-id") ?? requestId;
    const text = await resp.text();
    let json: unknown = null;
    if (text.length > 0) {
      try {
        json = JSON.parse(text);
      } catch {
        json = text;
      }
    }
    if (!resp.ok) {
      const code =
        json && typeof json === "object" && "code" in (json as object)
          ? String((json as { code: unknown }).code)
          : "internal";
      const message =
        json && typeof json === "object" && "error" in (json as object)
          ? String((json as { error: unknown }).error)
          : text || `HTTP ${resp.status}`;
      throw fromErrorBody(code, message, resp.status, responseRequestId);
    }
    return json as T;
  }

  /** 统一请求入口:对安全方法做有限退避重试。 */
  async request<T>(
    method: HttpMethod,
    path: string,
    body?: unknown,
    opts?: { idempotencyKey?: string },
  ): Promise<T> {
    const safe = this.isSafeRetry(method, path);
    let attempt = 0;
    for (;;) {
      try {
        return await this.attempt<T>(method, path, body, opts?.idempotencyKey);
      } catch (err) {
        const shouldRetry =
          safe &&
          attempt + 1 < this.retry.maxAttempts &&
          err instanceof CombeeError &&
          err.status !== undefined &&
          err.status >= 500;
        if (!shouldRetry) throw err;
        attempt += 1;
        const delay = Math.min(
          this.retry.baseDelayMs * 2 ** (attempt - 1),
          this.retry.maxDelayMs,
        );
        await new Promise((r) => setTimeout(r, delay));
      }
    }
  }

  /** 游标分页 GET。 */
  async paginate<T>(
    path: string,
    limit = 100,
    cursor?: string,
  ): Promise<Page<T>> {
    const sep = path.includes("?") ? "&" : "?";
    const q = `${sep}limit=${limit}${cursor ? `&cursor=${cursor}` : ""}`;
    return this.request<Page<T>>("GET", `${path}${q}`);
  }
}
