//! API Keys / Usage / Credits / Pricing(User Control Plane)。

import { Http } from "./http.js";
import type {
  ApiKeyInfo,
  CreatedApiKey,
  CreditBalance,
  CreditTransaction,
  Page,
  RedeemResult,
  UsageSummary,
} from "./types.js";

export class ApiKeys {
  constructor(private http: Http) {}

  async create(name?: string): Promise<CreatedApiKey> {
    // 当前服务端不存 name;保留签名以对齐 SDK_SPEC(创建时 name 仅客户端备注)
    void name;
    // 服务端返回 {key, record:{...}} → 平铺为 CreatedApiKey
    const r = await this.http.request<{ key: string; record: Omit<CreatedApiKey, "key"> }>(
      "POST",
      "/v1/api-keys",
      {},
      { idempotencyKey: `key:${name ?? Date.now()}` },
    );
    return { ...r.record, key: r.key };
  }

  async list(): Promise<ApiKeyInfo[]> {
    return this.http.request<ApiKeyInfo[]>("GET", "/v1/api-keys");
  }

  async revoke(id: string): Promise<void> {
    await this.http.request("DELETE", `/v1/api-keys/${id}`);
  }
}

export class Usage {
  constructor(private http: Http) {}

  async summary(opts?: { from?: string; to?: string }): Promise<UsageSummary> {
    const q = toQuery(opts);
    return this.http.request<UsageSummary>("GET", `/v1/usage/summary${q}`);
  }

  async cell(cellId: string, opts?: { from?: string; to?: string }): Promise<UsageSummary> {
    const q = toQuery(opts);
    return this.http.request<UsageSummary>("GET", `/v1/cells/${cellId}/usage${q}`);
  }

  async timeseries(opts: {
    metric: string;
    interval?: "minute" | "hour" | "day";
    from?: string;
    to?: string;
  }): Promise<Array<{ bucket_start: string; value: number }>> {
    const parts = [`metric=${encodeURIComponent(opts.metric)}`];
    parts.push(`interval=${opts.interval ?? "minute"}`);
    if (opts.from) parts.push(`from=${encodeURIComponent(opts.from)}`);
    if (opts.to) parts.push(`to=${encodeURIComponent(opts.to)}`);
    return this.http.request("GET", `/v1/usage/timeseries?${parts.join("&")}`);
  }
}

export class Credits {
  constructor(private http: Http) {}

  async balance(): Promise<CreditBalance> {
    return this.http.request<CreditBalance>("GET", "/v1/credits/balance");
  }

  async transactions(limit = 100, cursor?: string): Promise<Page<CreditTransaction>> {
    return this.http.paginate<CreditTransaction>("/v1/credits/transactions", limit, cursor);
  }

  async redeem(code: string): Promise<RedeemResult> {
    return this.http.request<RedeemResult>("POST", "/v1/credits/redeem", { code });
  }
}

export class Pricing {
  constructor(private http: Http) {}

  async get(): Promise<{ version: number; effective_at: number; units: Record<string, unknown> }> {
    return this.http.request("GET", "/v1/pricing");
  }
}

function toQuery(opts?: { from?: string; to?: string }): string {
  const parts: string[] = [];
  if (opts?.from) parts.push(`from=${encodeURIComponent(opts.from)}`);
  if (opts?.to) parts.push(`to=${encodeURIComponent(opts.to)}`);
  return parts.length ? `?${parts.join("&")}` : "";
}
