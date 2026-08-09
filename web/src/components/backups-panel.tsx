"use client";

import { useState } from "react";
import { AlertTriangle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { bffFetch } from "@/lib/bff/client";
import { useT } from "@/lib/i18n-context";

interface BackupRow {
  id: string;
  created_at: string;
  status: string;
}

/** 真实备份面板:创建快照 + 恢复(Combee 无备份列表端点,不造假列表)。 */
export function BackupsPanel({ cellId }: { cellId: string }) {
  const t = useT();
  const [items, setItems] = useState<BackupRow[]>([]);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [restoreTarget, setRestoreTarget] = useState<BackupRow | null>(null);
  const [restored, setRestored] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function runBackup() {
    setBusy(true);
    setError(null);
    try {
      const r = await bffFetch<{ id: string; created_at: number }>(`/v1/databases/${cellId}/backup`, {
        method: "POST",
        body: {},
      });
      const now = new Date().toISOString().replace("T", " ").slice(0, 19) + " UTC";
      setItems((prev) => [{ id: r.id ?? `bk-${Date.now().toString(36)}`, created_at: now, status: t.backups.statusCompleted }, ...prev]);
      setNotice(t.backups.noticeArchived);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function confirmRestore() {
    if (!restoreTarget) return;
    setBusy(true);
    setError(null);
    try {
      await bffFetch(`/v1/databases/${cellId}/restore`, { method: "POST", body: {} });
      setRestored(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setRestoreTarget(null);
    }
  }

  return (
    <div className="grid gap-4">
      <div className="flex items-center gap-3">
        <Button data-testid="backup-now" onClick={runBackup} disabled={busy} className="bg-tertiary text-primary-container font-mono-label font-bold hover:bg-tertiary-fixed">
          + {t.backups.create}
        </Button>
        {notice && <span className="font-mono-label text-tertiary">{notice}</span>}
      </div>

      {error && <div className="font-mono-code text-error text-xs" data-testid="backups-error">{error}</div>}

      <Card className="bg-surface border-outline-variant" data-testid="backups-list">
        <CardContent className="pt-4">
          {items.length === 0 ? (
            <div className="py-8 text-center font-mono-label text-on-surface-variant">{t.backups.empty}</div>
          ) : (
            <table className="w-full text-left font-mono-code text-sm">
              <thead>
                <tr className="text-on-surface-variant border-b border-outline-variant">
                  <th className="py-2 pr-4">{t.backups.backupId}</th>
                  <th className="py-2 pr-4">{t.backups.createdAt}</th>
                  <th className="py-2 pr-4">{t.backups.status}</th>
                  <th className="py-2">{t.common.delete === t.common.delete ? t.backups.restore : ""}</th>
                </tr>
              </thead>
              <tbody>
                {items.map((b) => (
                  <tr key={b.id} className="border-b border-outline-variant/60 last:border-0" data-testid="backup-row">
                    <td className="py-2 pr-4 text-tertiary">{b.id.slice(0, 16)}</td>
                    <td className="py-2 pr-4 text-on-surface-variant">{b.created_at}</td>
                    <td className="py-2 pr-4">
                      <span className="flex items-center gap-1.5 text-on-surface-variant">
                        <span className="w-2 h-2 rounded-sm bg-tertiary clip-path-hexagon" /> {b.status}
                      </span>
                    </td>
                    <td className="py-2">
                      <Button
                        variant="ghost"
                        size="sm"
                        className="font-mono-label text-tertiary hover:text-tertiary-fixed"
                        onClick={() => setRestoreTarget(b)}
                      >
                        {t.backups.restore}
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </CardContent>
      </Card>

      <Dialog open={restoreTarget !== null} onOpenChange={(o) => !o && setRestoreTarget(null)}>
        <DialogContent className="bg-surface-container border-error/40 overflow-hidden" data-testid="restore-modal">
          <div className="h-1 bg-error" />
          <DialogHeader>
            <DialogTitle className="font-mono-label text-error flex items-center gap-2">
              <AlertTriangle className="h-5 w-5" /> {t.backups.restoreConfirmTitle}
            </DialogTitle>
            <DialogDescription className="font-mono-code text-on-surface-variant text-xs">
              {t.backups.restoreConfirm} {cellId.slice(0, 12)}…
            </DialogDescription>
          </DialogHeader>
          <div className="bg-error-container/20 border border-error/40 rounded p-3 font-mono-code text-xs text-error" data-testid="restore-warning">
            <strong>{t.backups.critical}:</strong> {t.backups.destructiveWarn}
          </div>
          <DialogFooter className="flex gap-2">
            <Button variant="outline" className="font-mono-label border-outline-variant text-on-surface-variant" onClick={() => setRestoreTarget(null)}>
              {t.common.cancel}
            </Button>
            <Button className="font-mono-label bg-error text-on-error hover:bg-error/80" onClick={confirmRestore} disabled={busy}>
              {t.backups.confirmRestore}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      {restored && (
        <div className="font-mono-label text-tertiary" data-testid="restore-result">
          {t.backups.restored}
        </div>
      )}
    </div>
  );
}
