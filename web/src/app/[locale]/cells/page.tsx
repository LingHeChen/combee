"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { useT } from "@/lib/i18n-context";
import { useParams } from "next/navigation";
import { Plus, Search } from "lucide-react";
import Shell from "@/components/shell";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { bffFetch } from "@/lib/bff/client";
import type { CellStat } from "@/lib/types";
import { formatTime, shortId } from "@/lib/utils";

export default function CellsPage() {
  const t = useT();
  const { locale } = useParams<{ locale: string }>();
  const [cells, setCells] = useState<CellStat[]>([]);
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState("all");
  const [region, setRegion] = useState("all");

  useEffect(() => {
    bffFetch<CellStat[]>("/v1/databases?limit=1000")
      .then((list) =>
        list.map((c) => ({
          id: c.id,
          name: (c as { name?: string }).name ?? c.id,
          state: c.state,
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
      )
      .then(setCells);
  }, []);

  const regions = ["all", ...Array.from(new Set(cells.map((c) => c.region)))];
  const filtered = cells.filter(
    (c) =>
      (status === "all" || c.state === status) &&
      (region === "all" || c.region === region) &&
      (c.name ?? "").toLowerCase().includes(query.toLowerCase()),
  );

  return (
    <Shell>
      <div className="flex flex-col md:flex-row justify-between items-start md:items-end gap-4">
        <div>
          <h2 className="text-2xl md:text-3xl font-semibold text-on-surface">{t.cells.title}</h2>
          <p className="text-base text-on-surface-variant mt-1">{t.cells.subtitle}</p>
        </div>
        <Button asChild className="bg-tertiary text-primary-container px-6 py-2.5 font-mono-label font-bold hover:bg-tertiary-fixed">
          <Link href={`/${locale}/cells/new`}>
            <Plus className="h-4 w-4" /> {t.shell.createCell}
          </Link>
        </Button>
      </div>

      {/* 工具条:搜索 + 筛选 */}
      <div className="flex flex-col md:flex-row gap-3 mt-6">
        <div className="relative flex-1 max-w-sm">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-on-surface-variant" />
          <Input
            data-testid="cells-search"
            placeholder={t.cells.searchPlaceholder}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            className="pl-9 bg-surface-container-low border-outline-variant font-mono-code text-sm"
          />
        </div>
        <select
          data-testid="cells-status-filter"
          value={status}
          onChange={(e) => setStatus(e.target.value)}
          className="h-9 rounded bg-surface-container-low border border-outline-variant px-3 text-sm text-on-surface font-mono-label focus:outline-none focus:ring-1 focus:ring-tertiary"
        >
          {[t.cells.allStatus, t.overview.stateActive, t.overview.stateIdle].map((s) => (
            <option key={s} value={s === t.cells.allStatus ? "all" : s === t.overview.stateActive ? "active" : "idle"}>{s}</option>
          ))}
        </select>
        <select
          data-testid="cells-region-filter"
          value={region}
          onChange={(e) => setRegion(e.target.value)}
          className="h-9 rounded bg-surface-container-low border border-outline-variant px-3 text-sm text-on-surface font-mono-label focus:outline-none focus:ring-1 focus:ring-tertiary"
        >
          {regions.map((r) => (
            <option key={r} value={r}>{r === "all" ? t.cells.allRegions : r}</option>
          ))}
        </select>
      </div>

      <div className="mt-6 grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {filtered.map((c) => (
          <Link
            key={c.id}
            href={`/${locale}/cells/${c.id}`}
            data-testid="cell-card"
            className="bg-surface p-5 rounded border border-secondary-container hover:border-tertiary/50 hover:shadow-[0_0_15px_rgba(255,185,95,0.08)] transition-all flex flex-col gap-3 group"
          >
            <div className="flex justify-between items-center">
              <span className="font-mono-code text-tertiary group-hover:text-tertiary-fixed">{c.name ?? shortId(c.id)}</span>
              <span
                className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full border text-[10px] font-mono-label ${
                  c.state === "active" ? "border-tertiary/50 text-tertiary" : "border-outline-variant text-on-surface-variant"
                }`}
              >
                <span className={`w-1.5 h-1.5 rounded-sm clip-path-hexagon ${c.state === "active" ? "bg-tertiary animate-pulse" : "bg-on-surface-variant/50"}`} />
                {c.state === "active" ? t.overview.stateActive : t.overview.stateIdle}
              </span>
            </div>
            <div className="font-mono-label text-on-surface-variant">{t.cells.idLabel}: {shortId(c.id, 12)}</div>
            <div className="mt-auto pt-3 border-t border-outline-variant/40 font-mono-code text-xs flex items-center justify-between">
              <span className="text-on-surface-variant">{t.cellDetail.created}</span>
              <span className="text-on-surface">{formatTime(c.created_at)}</span>
            </div>
          </Link>
        ))}
        {filtered.length === 0 && (
          <div className="col-span-full py-16 text-center font-mono-label text-on-surface-variant">
            {t.cells.noMatch}
          </div>
        )}
      </div>
    </Shell>
  );
}
