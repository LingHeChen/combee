"use client";

import { use, useEffect, useState } from "react";
import Link from "next/link";
import { useT } from "@/lib/i18n-context";
import {
  ArrowLeft,
  ChevronDown,
  MoreVertical,
  Plug,
  Trash2,
} from "lucide-react";
import Shell from "@/components/shell";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { CodeBlock } from "@/components/code-block";
import { SqlWorkspace } from "@/components/sql-workspace";
import { KvBrowser } from "@/components/kv-browser";
import { BackupsPanel } from "@/components/backups-panel";
import { ReplicationPanel } from "@/components/replication-panel";
import { CellUsagePanel } from "@/components/cell-usage-panel";
import { bffFetch } from "@/lib/bff/client";
import type { CellStat } from "@/lib/types";
import { formatBytes, formatTime, shortId } from "@/lib/utils";
import { useRouter, useParams } from "next/navigation";

export default function CellDetailPage({ params }: { params: Promise<{ id: string }> }) {
  const t = useT();
  const { locale } = useParams<{ locale: string }>();
  const { id } = use(params);
  const router = useRouter();
  const [cell, setCell] = useState<CellStat | null>(null);
  const [notFound, setNotFound] = useState(false);
  const [showDiag, setShowDiag] = useState(false);

  useEffect(() => {
    bffFetch<Array<{ id: string; state: string; created_at: number; name?: string }>>("/v1/databases?limit=1000")
      .then((list) => {
        const hit = list.find((c) => c.id === id);
        if (!hit) return setNotFound(true);
        setCell({
          id: hit.id,
          name: (hit as { name?: string }).name ?? hit.id,
          state: hit.state === "active" ? "active" : "idle",
          created_at: hit.created_at,
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
        });
      });
  }, [id]);

  if (notFound) {
    return (
      <Shell>
        <div className="flex flex-col items-center gap-4 py-24">
          <h2 className="text-2xl font-semibold text-on-surface">{t.cellDetail.notFound}</h2>
          <Button asChild variant="outline" className="font-mono-label">
            <Link href={`/${locale}/cells`}>{t.cellDetail.backToCells}</Link>
          </Button>
        </div>
      </Shell>
    );
  }

  if (!cell) {
    return (
      <Shell>
        <div className="animate-pulse space-y-4">
          <div className="h-8 w-48 bg-surface-container rounded" />
          <div className="h-32 bg-surface-container rounded" />
        </div>
      </Shell>
    );
  }

  const healthy = cell.state === "active";

  return (
    <Shell>
      <div className="flex flex-col md:flex-row justify-between items-start gap-4">
        <div>
          <Link href={`/${locale}/cells`} className="font-mono-label text-on-surface-variant hover:text-tertiary flex items-center gap-1">
            <ArrowLeft className="h-4 w-4" /> {t.shell.cells}
          </Link>
          <h2 className="text-2xl md:text-3xl font-semibold text-on-surface mt-2 flex items-center gap-3">
            {cell.name ?? shortId(cell.id)}
            <span
              data-testid="cell-health"
              className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full border text-xs font-mono-label ${
                healthy ? "border-tertiary/60 text-tertiary" : "border-outline-variant text-on-surface-variant"
              }`}
            >
              <span className={`w-2 h-2 rounded-sm clip-path-hexagon ${healthy ? "bg-tertiary animate-pulse" : "bg-on-surface-variant/50"}`} />
              {healthy ? t.cellDetail.stateHealthy : cell.state}
            </span>
          </h2>
          <p className="font-mono-code text-on-surface-variant mt-1 flex items-center gap-3">
            <span className="flex items-center gap-1"><span className="opacity-60">{t.overview.thRegion}:</span> {cell.region}</span>
            <span className="flex items-center gap-1"><span className="opacity-60">{t.cells.idLabel}:</span> {shortId(cell.id, 12)}</span>
          </p>
        </div>
        <div className="flex gap-2">
          <Button asChild variant="outline" className="font-mono-label border-outline-variant text-on-surface-variant hover:text-tertiary">
            <Link href={`/${locale}/cells/${cell.id}/connect`}>
              <Plug className="h-4 w-4 mr-1" /> {t.cellDetail.connect}
            </Link>
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="icon" aria-label="More actions" className="border-outline-variant text-on-surface-variant hover:text-tertiary">
                <MoreVertical className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="bg-surface-container border-outline-variant">

              <DropdownMenuItem
                className="font-mono-label text-error"
                data-testid="delete-cell"
                onClick={async () => {
                  if (confirm(t.cellDetail.deleteCellConfirmTemplate.replace("{name}", cell.name ?? cell.id))) {
                    await bffFetch(`/v1/databases/${cell.id}`, { method: "DELETE" });
                    router.push(`/${locale}/cells`);
                  }
                }}
              >
                <Trash2 className="h-4 w-4 mr-2" /> {t.cellDetail.delete}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>

      {/* 真实信息 */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mt-6">
        <div className="bg-surface p-5 rounded border border-secondary-container">
          <div className="font-mono-label text-on-surface-variant uppercase tracking-wider text-xs">{t.cellDetail.statStatus}</div>
          <div className="flex items-center gap-2 mt-1.5">
            <span className={`w-2 h-2 rounded-sm clip-path-hexagon ${healthy ? "bg-tertiary animate-pulse" : "bg-on-surface-variant/50"}`} />
            <span className="font-mono-code text-on-surface">{healthy ? t.overview.stateActive : t.overview.stateIdle}</span>
          </div>
        </div>
        <div className="bg-surface p-5 rounded border border-secondary-container">
          <div className="font-mono-label text-on-surface-variant uppercase tracking-wider text-xs">{t.cells.idLabel}</div>
          <div className="font-mono-code text-on-surface mt-1.5 break-all">{cell.id}</div>
        </div>
        <div className="bg-surface p-5 rounded border border-secondary-container">
          <div className="font-mono-label text-on-surface-variant uppercase tracking-wider text-xs">{t.cellDetail.created}</div>
          <div className="font-mono-code text-on-surface mt-1.5">{formatTime(cell.created_at)}</div>
        </div>
      </div>

      {/* Advanced Diagnostics */}
      <div className="mt-4 bg-surface border border-outline-variant rounded">
        <button
          onClick={() => setShowDiag((v) => !v)}
          className="w-full flex items-center justify-between px-5 py-3 font-mono-label text-on-surface-variant hover:text-on-surface transition-colors"
          data-testid="diag-toggle"
        >
          <span>{t.cellDetail.advancedDiag}</span>
          <ChevronDown className={`h-4 w-4 transition-transform ${showDiag ? "rotate-180" : ""}`} />
        </button>
        {showDiag && (
          <div className="px-5 pb-4" data-testid="diag-json">
            <CodeBlock code={JSON.stringify(cell.diagnostics, null, 2)} language="json" className="text-xs" />
          </div>
        )}
      </div>

      <Tabs defaultValue="overview" className="mt-8" data-testid="cell-tabs">
        <TabsList className="bg-surface-container-low border border-outline-variant gap-1 flex-wrap">
          <TabsTrigger value="overview" className="font-mono-label">{t.cellDetail.tabs.overview}</TabsTrigger>
          <TabsTrigger value="sql" className="font-mono-label">{t.cellDetail.tabs.sql}</TabsTrigger>
          <TabsTrigger value="kv" className="font-mono-label">{t.cellDetail.tabs.kv}</TabsTrigger>
          <TabsTrigger value="usage" className="font-mono-label">{t.cellDetail.tabs.usage}</TabsTrigger>
          <TabsTrigger value="backups" className="font-mono-label">{t.cellDetail.tabs.backups}</TabsTrigger>
          <TabsTrigger value="replication" className="font-mono-label">{t.cellDetail.tabs.replication}</TabsTrigger>
          <TabsTrigger value="settings" className="font-mono-label">{t.cellDetail.tabs.settings}</TabsTrigger>
        </TabsList>
        <TabsContent value="overview" className="mt-4">
          <div className="bg-surface border border-outline-variant rounded p-6 flex flex-col gap-3">
            <div className="font-mono-label text-on-surface-variant uppercase tracking-wider text-xs">{t.cellDetail.engine}</div>
            <div className="font-mono-code text-on-surface">{t.cellDetail.engineDesc}</div>
            <div className="font-mono-label text-on-surface-variant uppercase tracking-wider text-xs mt-4">{t.cellDetail.kvKeys}</div>
            <div className="font-mono-code text-on-surface">{cell.kv_keys.toLocaleString()} {t.cellDetail.keysSuffix}</div>
            <div className="font-mono-label text-on-surface-variant uppercase tracking-wider text-xs mt-4">{t.cellDetail.sqlTables}</div>
            <div className="font-mono-code text-on-surface">{cell.sql_tables} {t.cellDetail.tablesSuffix}</div>
          </div>
        </TabsContent>
        <TabsContent value="sql" className="mt-4">
          <SqlWorkspace cellId={cell.id} />
        </TabsContent>
        <TabsContent value="kv" className="mt-4">
          <KvBrowser cellId={cell.id} />
        </TabsContent>
        <TabsContent value="usage" className="mt-4">
          <CellUsagePanel cellId={cell.id} />
        </TabsContent>
        <TabsContent value="backups" className="mt-4">
          <BackupsPanel cellId={cell.id} />
        </TabsContent>
        <TabsContent value="replication" className="mt-4">
          <ReplicationPanel cellId={cell.id} />
        </TabsContent>
        <TabsContent value="settings" className="mt-4">
          <SettingsPanel cell={cell} />
        </TabsContent>
      </Tabs>
    </Shell>
  );
}

function SettingsPanel({ cell }: { cell: CellStat }) {
  const t = useT();
  return (
    <div className="bg-surface border border-outline-variant rounded p-6 flex flex-col gap-4">
      <div className="flex justify-between items-center border-b border-outline-variant pb-3">
        <span className="font-mono-label text-on-surface-variant">{t.cells.idLabel}</span>
        <span className="font-mono-code text-on-surface">{cell.id}</span>
      </div>
      <div className="flex justify-between items-center border-b border-outline-variant pb-3">
        <span className="font-mono-label text-on-surface-variant">{t.cellDetail.engine}</span>
        <span className="font-mono-code text-on-surface">sqlite · v1.14.2</span>
      </div>
      <div className="flex justify-between items-center border-b border-outline-variant pb-3">
        <span className="font-mono-label text-on-surface-variant">{t.cellDetail.durability}</span>
        <span className="font-mono-code text-on-surface">{t.cellDetail.durabilityNormal}</span>
      </div>
      <div className="flex justify-between items-center">
        <span className="font-mono-label text-on-surface-variant">{t.cellDetail.created}</span>
        <span className="font-mono-code text-on-surface">{formatTime(cell.created_at)}</span>
      </div>
    </div>
  );
}
