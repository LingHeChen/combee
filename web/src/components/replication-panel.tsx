"use client";

import { useEffect, useState } from "react";
import { AlertTriangle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { bffFetch } from "@/lib/bff/client";
import { useT } from "@/lib/i18n-context";

interface ReplicationStatus {
  db: string;
  replica_node?: string | null;
}

/** 真实复制面板:显示副本状态 + 手动故障转移(enable/disable 无后端端点,不造假按钮)。 */
export function ReplicationPanel({ cellId }: { cellId: string }) {
  const t = useT();
  const [status, setStatus] = useState<ReplicationStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);

  useEffect(() => {
    bffFetch<ReplicationStatus>(`/v1/databases/${cellId}/replication`)
      .then(setStatus)
      .catch(() => setStatus(null));
  }, [cellId]);

  const hasReplica = Boolean(status?.replica_node);

  async function doFailover() {
    setBusy(true);
    setError(null);
    setDone(false);
    try {
      await bffFetch<unknown>(`/v1/databases/${cellId}/failover`, { method: "POST" });
      setDone(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="grid gap-4" data-testid="replication-panel">
      <Card className="bg-surface border-outline-variant">
        <CardHeader className="pb-2">
          <CardTitle className="font-mono-label text-on-surface text-sm">{t.replication.title}</CardTitle>
        </CardHeader>
        <CardContent className="grid grid-cols-2 gap-4">
          <div>
            <div className="font-mono-label text-on-surface-variant uppercase tracking-wider text-xs">{t.cellDetail.statStatus}</div>
            <div className="flex items-center gap-1.5 mt-1">
              <span className={`w-2 h-2 rounded-sm clip-path-hexagon ${hasReplica ? "bg-tertiary animate-pulse" : "bg-on-surface-variant/50"}`} />
              <span className="font-mono-code text-on-surface">
                {hasReplica ? t.cellDetail.stateHealthy : t.replication.noReplica}
              </span>
            </div>
          </div>
          <div>
            <div className="font-mono-label text-on-surface-variant uppercase tracking-wider text-xs">{t.replication.replica}</div>
            <div className="font-mono-code text-on-surface mt-1 truncate">{status?.replica_node ?? "—"}</div>
          </div>
        </CardContent>
      </Card>

      {error && <div className="font-mono-code text-error text-xs" data-testid="replication-error">{error}</div>}
      {done && <div className="font-mono-label text-tertiary text-xs" data-testid="replication-done">{t.replication.failoverDone}</div>}

      <Card className="bg-surface border-error/30">
        <CardHeader className="pb-2">
          <CardTitle className="font-mono-label text-error text-sm flex items-center gap-2">
            <AlertTriangle className="h-4 w-4" /> {t.replication.advancedOps}
          </CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <p className="font-mono-code text-on-surface-variant text-sm">{t.replication.manualFailoverDesc}</p>
          <div className="flex gap-2">
            <Button
              variant="outline"
              data-testid="initiate-failover"
              disabled={busy}
              className="font-mono-label border-error text-error hover:text-error disabled:opacity-40"
              onClick={doFailover}
            >
              {t.replication.initiateFailover}
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
