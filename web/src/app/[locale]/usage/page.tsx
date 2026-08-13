import { Download } from "lucide-react";
import { getDict, type Locale } from "@/lib/i18n";
import { redirect } from "next/navigation";
import Shell from "@/components/shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { combeeRequest } from "@/lib/combee-client";
import { sessionFromCookies } from "@/lib/bff/auth";
import type { UsageSummary } from "@/lib/types";
import { formatBytes } from "@/lib/utils";
import { UsageChartSection } from "@/components/usage-chart-section";

export default async function UsagePage({ params }: { params: Promise<{ locale: string }> }) {
  const { locale: loc } = await params;
  const t = getDict((loc as Locale) ?? "zh");
  const session = await sessionFromCookies();
  if (!session) redirect(`/${loc}/login`);
  const u = await combeeRequest<UsageSummary>("/v1/usage/summary", { apiKey: session.api_key }).catch(() => null);

  const reqs = u?.request_count ?? 0;

  const cards = [
    { label: t.usage.cardRequests, value: (reqs).toLocaleString() },
    { label: t.usage.cardSqlRw, value: u ? `${u.operations.sql_reads.toLocaleString()} / ${u.operations.sql_writes.toLocaleString()}` : "—" },
    { label: t.usage.cardKvRw, value: u ? `${u.operations.kv_reads.toLocaleString()} / ${u.operations.kv_writes.toLocaleString()}` : "—" },
    { label: t.usage.cardStorage, value: u ? formatBytes(u.current_storage_bytes) : "—" },
    { label: t.usage.cardBytesIn, value: u ? formatBytes(u.bytes_in) : "—" },
    { label: t.usage.cardEgress, value: u ? formatBytes(u.bytes_out) : "—" },
  ];

  return (
    <Shell>
      <div className="flex flex-col md:flex-row justify-between items-start md:items-end gap-4">
        <div>
          <h2 className="text-2xl md:text-3xl font-semibold text-on-surface">{t.usage.title}</h2>
          <p className="text-base text-on-surface-variant mt-1">{t.usage.subtitle}</p>
        </div>

      </div>

      <div className="grid grid-cols-1 md:grid-cols-3 lg:grid-cols-6 gap-4 mt-8">
        {cards.map((c) => (
          <div key={c.label} className="bg-surface p-4 rounded border border-secondary-container hover:border-tertiary/50 transition-colors">
            <div className="font-mono-label text-on-surface-variant uppercase tracking-wider text-xs">{c.label}</div>
            <div className="text-lg font-semibold text-on-surface mt-1">{c.value}</div>
          </div>
        ))}
      </div>

      <div className="mt-8">
        <Card className="bg-surface border-tertiary/40">
          <CardHeader>
            <CardTitle className="font-mono-label text-on-surface text-sm">{t.usage.chartTitle}</CardTitle>
          </CardHeader>
          <CardContent>
            <UsageChartSection t={t.usage} />
          </CardContent>
        </Card>
      </div>

      {u && (
        <div className="mt-4 font-mono-code text-on-surface-variant text-xs">
          {t.usage.currentStorage}: {formatBytes(u.current_storage_bytes)} · {t.usage.period} {u.period.from} → {u.period.to}
        </div>
      )}
    </Shell>
  );
}
