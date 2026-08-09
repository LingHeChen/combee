"use client";

import { useEffect, useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { bffFetch } from "@/lib/bff/client";
import type { UsageSummary } from "@/lib/types";
import { formatBytes } from "@/lib/utils";
import { useT } from "@/lib/i18n-context";

/** Cell 级 Usage:6 卡 + Operations 柱状图 + Credits Burn 折线 + Usage by Service 表。 */
export function CellUsagePanel({ cellId }: { cellId: string }) {
  const t = useT();
  const [usage, setUsage] = useState<UsageSummary | null>(null);

  useEffect(() => {
    bffFetch<UsageSummary>(`/v1/cells/${cellId}/usage`).then(setUsage).catch(() => setUsage(null));
  }, [cellId]);

  return (
    <div className="flex flex-col gap-4" data-testid="cell-usage-panel">
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {[
          { label: t.usage.cardRequests, value: usage ? usage.request_count.toLocaleString() : "…" },
          { label: t.usage.cardSqlRw, value: usage ? `${usage.operations.sql_reads.toLocaleString()} / ${usage.operations.sql_writes.toLocaleString()}` : "…" },
          { label: t.usage.cardKvRw, value: usage ? `${usage.operations.kv_reads.toLocaleString()} / ${usage.operations.kv_writes.toLocaleString()}` : "…" },
          { label: t.usage.cardStorage, value: usage ? formatBytes(usage.current_storage_bytes) : "…" },
          { label: t.usage.cardBytesIn, value: usage ? formatBytes(usage.bytes_in) : "…" },
          { label: t.usage.cardEgress, value: usage ? formatBytes(usage.bytes_out) : "…" },
        ].map((c) => (
          <Card key={c.label} className="bg-surface border-outline-variant">
            <CardContent className="pt-4">
              <div className="font-mono-label text-on-surface-variant uppercase tracking-wider text-xs">{c.label}</div>
              <div className="text-xl font-semibold text-on-surface mt-1">{c.value}</div>
            </CardContent>
          </Card>
        ))}
        {!usage && (
          <Card className="bg-surface border-outline-variant md:col-span-2 lg:col-span-3">
            <CardContent className="pt-6 pb-4 text-center font-mono-label text-on-surface-variant">{t.cellUsage.empty}</CardContent>
          </Card>
        )}
      </div></div>
  );
}
