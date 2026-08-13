"use client";

// 用量图表区:metric 切换 + period 区间选择 + 按 Cell 拆分,基于 shadcn chart + recharts。
import { useEffect, useMemo, useState } from "react";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";
import { bffFetch } from "@/lib/bff/client";
import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";

type MetricKey = "requests" | "sql" | "kv";
type PeriodKey = "24h" | "7d" | "30d";

// metric 组 → 底层 usage metric(可多个,求和)
const METRIC_MAP: Record<MetricKey, string[]> = {
  requests: ["requests"],
  sql: ["sql_read", "sql_write"],
  kv: ["kv_read", "kv_write"],
};

const PERIODS: Record<PeriodKey, { hours: number; interval: string }> = {
  "24h": { hours: 24, interval: "hour" },
  "7d": { hours: 24 * 7, interval: "day" },
  "30d": { hours: 24 * 30, interval: "day" },
};

function iso(hoursAgo: number): string {
  return new Date(Date.now() - hoursAgo * 3600 * 1000).toISOString();
}

const chartConfig = {
  value: { label: "Usage", color: "#d79921" },
} satisfies ChartConfig;

type T = {
  chartTitle: string;
  chartRequests: string;
  chartSqlOps: string;
  chartKvOps: string;
  consumptionByCell: string;
  noChartData: string;
  last24h: string;
  last7d: string;
  last30d: string;
  period: string;
};

export function UsageChartSection({ t }: { t: T }) {
  const [metric, setMetric] = useState<MetricKey>("requests");
  const [period, setPeriod] = useState<PeriodKey>("24h");
  const [byCell, setByCell] = useState(false);
  const [points, setPoints] = useState<Array<{ label: string; value: number }>>([]);
  const [cellRows, setCellRows] = useState<Array<{ label: string; value: number }>>([]);
  const [loading, setLoading] = useState(true);

  const metricLabel: Record<MetricKey, string> = {
    requests: t.chartRequests,
    sql: t.chartSqlOps,
    kv: t.chartKvOps,
  };

  // 租户时序(单 metric 或多 metric 求和)
  useEffect(() => {
    if (byCell) return;
    let cancelled = false;
    setLoading(true);
    const { hours, interval } = PERIODS[period];
    const from = iso(hours);
    Promise.all(
      METRIC_MAP[metric].map((m) =>
        bffFetch<Array<{ bucket_start: string; value: number }>>(
          `/v1/usage/timeseries?metric=${m}&interval=${interval}&from=${encodeURIComponent(from)}`,
        ).catch(() => []),
      ),
    ).then((series) => {
      if (cancelled) return;
      const merged = new Map<string, number>();
      series.flat().forEach((p) => {
        merged.set(p.bucket_start, (merged.get(p.bucket_start) ?? 0) + p.value);
      });
      setPoints(
        [...merged.entries()]
          .sort((a, b) => (a[0] < b[0] ? -1 : 1))
          .map(([bucket, value]) => ({ label: bucket.slice(11, 16), value })),
      );
      setLoading(false);
    });
    return () => {
      cancelled = true;
    };
  }, [metric, period, byCell]);

  // 按 Cell:列出 cells,逐个取 usage 汇总
  useEffect(() => {
    if (!byCell) return;
    let cancelled = false;
    setLoading(true);
    bffFetch<Array<{ id: string; name?: string }>>("/v1/databases?limit=1000")
      .then(async (cells) => {
        const rows = await Promise.all(
          (Array.isArray(cells) ? cells : []).map(async (c) => {
            const u = await bffFetch<{
              request_count?: number;
              operations?: { kv_reads?: number; kv_writes?: number; sql_reads?: number; sql_writes?: number };
            }>(`/v1/cells/${c.id}/usage`).catch(() => ({} as {
              request_count?: number;
              operations?: { kv_reads?: number; kv_writes?: number; sql_reads?: number; sql_writes?: number };
            }));
            const ops = u.operations ?? {};
            const val =
              metric === "requests"
                ? u.request_count ?? 0
                : metric === "sql"
                  ? (ops.sql_reads ?? 0) + (ops.sql_writes ?? 0)
                  : (ops.kv_reads ?? 0) + (ops.kv_writes ?? 0);
            return { label: c.name ?? c.id.slice(0, 8), value: val };
          }),
        );
        if (!cancelled) {
          setCellRows(rows.sort((a, b) => b.value - a.value));
          setLoading(false);
        }
      })
      .catch(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [byCell, metric]);

  const data = byCell ? cellRows : points;
  const empty = !loading && data.length === 0;

  return (
    <div>
      {/* 控制栏 */}
      <div className="flex flex-wrap items-center gap-3 mb-4">
        <div className="flex gap-1 bg-surface border border-tertiary/40 rounded p-1">
          {(Object.keys(metricLabel) as MetricKey[]).map((k) => (
            <button
              key={k}
              onClick={() => setMetric(k)}
              className={`px-3 py-1 rounded text-xs font-mono-label transition-colors ${
                metric === k ? "bg-primary text-on-primary" : "text-on-surface-variant hover:text-on-surface"
              }`}
            >
              {metricLabel[k]}
            </button>
          ))}
        </div>
        <div className="flex gap-1 bg-surface border border-tertiary/40 rounded p-1">
          {(["24h", "7d", "30d"] as PeriodKey[]).map((p) => (
            <button
              key={p}
              onClick={() => setPeriod(p)}
              className={`px-3 py-1 rounded text-xs font-mono-label transition-colors ${
                period === p ? "bg-primary text-on-primary" : "text-on-surface-variant hover:text-on-surface"
              }`}
            >
              {p === "24h" ? t.last24h : p === "7d" ? t.last7d : t.last30d}
            </button>
          ))}
        </div>
        <button
          onClick={() => setByCell((v) => !v)}
          className={`px-3 py-1 rounded text-xs font-mono-label border transition-colors ${
            byCell ? "bg-secondary-container border-tertiary" : "border-tertiary/40 text-on-surface-variant"
          }`}
        >
          {t.consumptionByCell}
        </button>
      </div>

      {empty ? (
        <p className="text-sm text-on-surface-variant">{t.noChartData}</p>
      ) : (
        <ChartContainer config={chartConfig} className="h-56 w-full">
          <BarChart data={data} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
            <CartesianGrid vertical={false} strokeDasharray="3 3" />
            <XAxis dataKey="label" tickLine={false} axisLine={false} tickMargin={8} minTickGap={20} fontSize={10} />
            <YAxis tickLine={false} axisLine={false} fontSize={10} width={40} />
            <ChartTooltip cursor={{ fill: "rgba(124,111,100,0.12)" }} content={<ChartTooltipContent />} />
            <Bar dataKey="value" fill="var(--color-value)" radius={[3, 3, 0, 0]} />
          </BarChart>
        </ChartContainer>
      )}
    </div>
  );
}
