import "server-only";

// BFF 聚合层:把多个 Combee 调用聚合为 UI 需要的形状(减少往返、隐藏内部细节)。

import { combeeRequest } from "@/lib/combee-client";
import { bffKey, type BffSession } from "@/lib/bff/auth";
import type { OverviewData, UsageSummary } from "@/lib/types";

interface ApiCell {
  id: string;
  tenant_id?: string;
  state: string;
  created_at: number;
}

export async function aggregateOverview(session: BffSession): Promise<OverviewData> {
  // cells 用平台 key 拉全量,再按用户租户过滤(BFF 负责归属隔离,不计费);
  // usage/credits 用用户 key 查(租户聚合正确,且 Combee 对查询自身用量不计费)。
  const [cells, usage, credits] = await Promise.all([
    combeeRequest<ApiCell[]>("/v1/databases?limit=1000", { apiKey: bffKey() }),
    combeeRequest<UsageSummary>("/v1/usage/summary", { apiKey: session.api_key }).catch(() => null),
    combeeRequest<{ available: string }>("/v1/credits/balance", { apiKey: session.api_key }).catch(() => null),
  ]);

  const list = (Array.isArray(cells) ? cells : []).filter(
    (c) => !!session.tenant_id && c.tenant_id === session.tenant_id,
  );
  const active = list.filter((c) => c.state === "active").length;

  return {
    cellsTotal: list.length,
    cellsActive: active,
    requests: usage?.request_count ?? 0,
    storageBytes: usage?.current_storage_bytes ?? 0,
    creditsBalance: credits?.available ?? "0",
    recentCells: list.slice(0, 5).map((c, i) => ({
      id: c.id,
      name: (c as { name?: string }).name ?? c.id,
      state: c.state === "active" ? "active" : "idle",
      created_at: c.created_at,
      storage_bytes: 0,
      kv_keys: 0,
      sql_tables: 0,
      region: "auto",
      requests_24h: 0,
      last_active: "—",
      storage_pct: 0,
      backup_health: "—",
      replication: "Disabled",
      diagnostics: {},
    })),
  };
}
