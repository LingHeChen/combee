import Link from "next/link";
import { ArrowUpRight, Hexagon, Plus, TrendingUp } from "lucide-react";
import { getDict, type Locale } from "@/lib/i18n";
import { redirect } from "next/navigation";
import Shell from "@/components/shell";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { aggregateOverview } from "@/lib/bff/aggregate";
import { sessionFromCookies } from "@/lib/bff/auth";
import { formatBytes, formatTime, shortId } from "@/lib/utils";

export default async function OverviewPage({ params }: { params: Promise<{ locale: string }> }) {
  const { locale: loc } = await params;
  const t = getDict((loc as Locale) ?? "zh");
  const session = await sessionFromCookies();
  if (!session) redirect(`/${loc}/login`);
  let data;
  try {
    data = await aggregateOverview(session);
  } catch (err) {
    return <pre data-testid="debug-agg">{String(err)}</pre>;
  }
  const cells = data.recentCells;
  const active = cells.filter((c) => c.state === "active").length;
  const idle = cells.length - active;

  return (
    <Shell>
      <div className="flex flex-col md:flex-row justify-between items-start md:items-end gap-4">
        <div>
          <h2 className="text-2xl md:text-3xl font-semibold text-on-surface">{t.overview.title}</h2>
          <p className="text-base text-on-surface-variant mt-1">{t.overview.subtitle}</p>
        </div>
        <Button asChild className="bg-tertiary text-primary-container px-6 py-2.5 font-mono-label font-bold hover:bg-tertiary-fixed">
          <Link href={`/${loc}/cells/new`}>
            <Plus className="h-4 w-4" /> {t.shell.createCell}
          </Link>
        </Button>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4 mt-8">
        {/* Cells:12 Total + 9 Active / 3 Idle */}
        <Card className="bg-surface border-secondary-container hover:border-tertiary/50 transition-colors p-5 flex flex-col gap-4 group">
          <div className="flex justify-between items-center">
            <span className="font-mono-label text-on-surface-variant uppercase tracking-wider">{t.overview.statCells}</span>
            <Hexagon className="h-5 w-5 text-tertiary/70 group-hover:text-tertiary" />
          </div>
          <div className="flex items-end gap-2">
            <span className="text-4xl font-bold text-on-surface">{data.cellsTotal}</span>
            <span className="font-mono-label text-on-surface-variant mb-2">{t.overview.totalLabel}</span>
          </div>
          <div className="flex gap-4 mt-auto">
            <span className="flex items-center gap-1.5">
              <span className="w-2 h-2 bg-tertiary rounded-sm clip-path-hexagon" />
              <span className="font-mono-code text-on-surface">{active} {t.overview.stateActive}</span>
            </span>
            <span className="flex items-center gap-1.5">
              <span className="w-2 h-2 bg-on-surface-variant/50 rounded-sm clip-path-hexagon" />
              <span className="font-mono-code text-on-surface-variant">{idle} {t.overview.stateIdle}</span>
            </span>
          </div>
        </Card>

        {/* Requests(真实) */}
        <Card className="bg-surface border-secondary-container hover:border-tertiary/50 transition-colors p-5 flex flex-col gap-4 group">
          <div className="flex justify-between items-center">
            <span className="font-mono-label text-on-surface-variant uppercase tracking-wider">{t.overview.statRequests}</span>
            <TrendingUp className="h-5 w-5 text-tertiary/70 group-hover:text-tertiary" />
          </div>
          <div className="text-4xl font-bold text-on-surface" data-testid="stat-requests">{data.requests.toLocaleString()}</div>
        </Card>

        {/* Storage(真实) */}
        <Card className="bg-surface border-secondary-container hover:border-tertiary/50 transition-colors p-5 flex flex-col gap-4 group">
          <div className="flex justify-between items-center">
            <span className="font-mono-label text-on-surface-variant uppercase tracking-wider">{t.overview.statStorage}</span>
            <Hexagon className="h-5 w-5 text-tertiary/70 group-hover:text-tertiary" />
          </div>
          <div className="text-4xl font-bold text-on-surface" data-testid="stat-storage">{formatBytes(data.storageBytes)}</div>
        </Card>

        {/* Credits(真实) */}
        <Card className="bg-surface border-secondary-container hover:border-tertiary/50 transition-colors p-5 flex flex-col gap-4 group">
          <div className="flex justify-between items-center">
            <span className="font-mono-label text-on-surface-variant uppercase tracking-wider">{t.overview.statCredits}</span>
            <Hexagon className="h-5 w-5 text-tertiary/70 group-hover:text-tertiary" />
          </div>
          <div className="text-4xl font-bold text-on-surface" data-testid="stat-credits">{data.creditsBalance}</div>
        </Card>
      </div>

      <div className="mt-8">
        <div className="flex justify-between items-end mb-4">
          <div>
            <h3 className="text-xl font-medium text-on-surface">{t.overview.recentCells}</h3>
            <p className="text-sm text-on-surface-variant">{t.overview.recentCellsSub}</p>
          </div>
          <Link href={`/${loc}/cells`} className="font-mono-label text-tertiary hover:text-tertiary-fixed flex items-center gap-1">
            {t.overview.viewAll} <ArrowUpRight className="h-4 w-4" />
          </Link>
        </div>
        <div className="bg-surface border border-outline-variant rounded overflow-hidden">
          <table className="w-full text-left" data-testid="recent-cells-table">
            <thead>
              <tr className="font-mono-label text-on-surface-variant uppercase text-xs border-b border-outline-variant">
                <th className="px-4 py-3">{t.overview.thCell}</th>
                <th className="px-4 py-3">{t.overview.thStatus}</th>
                <th className="px-4 py-3">{t.cellDetail.created}</th>
              </tr>
            </thead>
            <tbody className="font-mono-code">
              {data.recentCells.map((c) => {
                const state = c.state === "active" ? t.overview.stateActive : t.overview.stateIdle;
                return (
                  <tr key={c.id} className="border-b border-outline-variant/60 last:border-0 hover:bg-surface-container-low transition-colors">
                    <td className="px-4 py-3">
                      <Link href={`/${loc}/cells/${c.id}`} className="text-tertiary hover:text-tertiary-fixed">
                        {shortId(c.id)}
                      </Link>
                    </td>
                    <td className="px-4 py-3">
                      <span className="inline-flex items-center gap-1.5 font-mono-label text-xs">
                        {c.state === "active" && <span className="w-1.5 h-1.5 bg-tertiary rounded-sm clip-path-hexagon animate-pulse" />}
                        <span className={c.state === "active" ? "text-tertiary" : "text-on-surface-variant"}>{state}</span>
                      </span>
                    </td>
                    <td className="px-4 py-3 text-on-surface-variant">{formatTime(c.created_at)}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>
    </Shell>
  );
}
