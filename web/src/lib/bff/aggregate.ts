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
  // cells 用平台 key + 显式 tenant_id 让平台按租户过滤(不再拉全量到 BFF);
  // usage/credits 用用户 key 查(租户聚合正确,且 Combee 对查询自身用量不计费)。
  const [cells, usage, credits] = await Promise.all([
    session.tenant_id
      ? combeeRequest<ApiCell[]>(`/v1/databases?limit=1000&tenant_id=${session.tenant_id}`, {
          apiKey: bffKey(),
        })
      : Promise.resolve([] as ApiCell[]),
    combeeRequest<UsageSummary>("/v1/usage/summary", { apiKey: session.api_key }).catch(() => null),
    combeeRequest<{ available: string }>("/v1/credits/balance", { apiKey: session.api_key }).catch(() => null),
  ]);

  // 纵深防御:平台已按租户过滤,这里再按 tenant_id 兜底过滤一次。
  const list = (Array.isArray(cells) ? cells : []).filter(
    (c) => !!session.tenant_id && c.tenant_id === session.tenant_id,
  );
  const active = list.filter((c) => c.state === "active").length;

  return {
    cellsTotal: list.length,
    cellsActive: active,
    requests: usage?.request_count ?? 0,
    storageBytes: usage?.current_storage_bytes ?? 0,
    creditsBalance: credits
      ? (Number(credits.available) / 1_000_000).toFixed(2)
      : "0.00",
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
