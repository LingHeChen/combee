import "server-only";

// Combee API 服务端客户端(BFF 数据面)。
// 全部前端数据存储走 Combee:Cell(SQL/KV)承载 BFF 元数据(会话等),
// 租户目录/用量/账本/定价在 Combee metadata。此处通过 Combee Public API
// (docs/API.md 冻结契约)访问。

const BASE = process.env.COMBEE_API_URL?.replace(/\/+$/, "");

export interface CombeeErrorBody {
  code: string;
  error: string;
}

export class CombeeApiError extends Error {
  code: string;
  status: number;
  constructor(code: string, message: string, status: number) {
    super(message);
    this.code = code;
    this.status = status;
  }
}

function needUrl(): string {
  if (!BASE) {
    throw new CombeeApiError(
      "bff_not_configured",
      "COMBEE_API_URL is not configured — set it to the Combee API Server URL",
      503,
    );
  }
  return BASE;
}

/** 服务端调用 Combee API(带 x-api-key;key 为空时 dev 模式放行)。 */
export async function combeeRequest<T = unknown>(
  path: string,
  opts: { method?: string; body?: unknown; apiKey?: string; idempotencyKey?: string } = {},
): Promise<T> {
  const base = needUrl();
  const headers: Record<string, string> = { "content-type": "application/json" };
  if (opts.apiKey) headers["x-api-key"] = opts.apiKey;
  // BFF 内部请求标记:平台服务账号 key 校验(api-server 匹配 → internal,不计费)。
  // 与 x-api-key(可能是用户 key,用于租户隔离)并存,两者独立。
  const bffToken = process.env.COMBEE_BFF_API_KEY ?? "";
  if (bffToken) headers["x-bff-token"] = bffToken;
  if (opts.idempotencyKey) headers["idempotency-key"] = opts.idempotencyKey;
  // request_id 贯穿:从 BFF 上下文读并透传 Combee API
  const rid = (await import("@/lib/bff/context")).currentRequestId();
  if (rid) headers["x-request-id"] = rid;

  const res = await fetch(`${base}${path}`, {
    method: opts.method ?? "GET",
    headers,
    body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
    cache: "no-store",
  });
  if (!res.ok) {
    let code = "internal";
    let message = `HTTP ${res.status}`;
    try {
      const body = (await res.json()) as Partial<CombeeErrorBody>;
      if (body.code) code = body.code;
      if (body.error) message = body.error;
    } catch {
      /* non-json */
    }
    throw new CombeeApiError(code, message, res.status);
  }
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

/** 校验 API key 是否有效(调租户资源;dev 模式放行)。 */
