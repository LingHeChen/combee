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
  // 统一:服务 key(internal → 不计费)+ on-behalf 租户,api-server 按目标租户 scope。
  // tenant_id 缺失(未回填)→ 不带 on-behalf 会命中平台租户,故直接返回空,绝不泄露。
  const onBehalfTenant = session.tenant_id ?? undefined;
  const [cells, usage, credits] = await Promise.all([
    onBehalfTenant
      ? combeeRequest<ApiCell[]>("/v1/databases?limit=1000", { apiKey: bffKey(), onBehalfTenant })
      : Promise.resolve([] as ApiCell[]),
    onBehalfTenant
      ? combeeRequest<UsageSummary>("/v1/usage/summary", { apiKey: bffKey(), onBehalfTenant }).catch(
          () => null,
        )
      : Promise.resolve(null),
    onBehalfTenant
      ? combeeRequest<{ available: string }>("/v1/credits/balance", {
          apiKey: bffKey(),
          onBehalfTenant,
        }).catch(() => null)
      : Promise.resolve(null),
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
