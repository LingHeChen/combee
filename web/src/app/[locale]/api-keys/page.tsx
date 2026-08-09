"use client";

import { useT } from "@/lib/i18n-context";
import { useEffect, useState } from "react";
import { Copy, KeyRound, Plus, Trash2 } from "lucide-react";
import Shell from "@/components/shell";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { bffFetch } from "@/lib/bff/client";
import type { ApiKeyInfo, CreatedApiKey } from "@/lib/types";
import { formatTime } from "@/lib/utils";

interface KeyRow {
  id: string;
  name: string;
  prefix: string;
  last_used: string;
  created_at: number;
  revoked_at?: number | null;
}

export default function ApiKeysPage() {
  const t = useT();
  const [keys, setKeys] = useState<KeyRow[]>([]);
  const [created, setCreated] = useState<CreatedApiKey | null>(null);
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    bffFetch<ApiKeyInfo[]>("/v1/api-keys").then((ks) => {
      setKeys(
        ks.map((k) => ({
          id: k.id,
          name: k.id.slice(0, 12),
          prefix: `${k.key_hash.slice(0, 12)}••••••`,
          last_used: "—",
          created_at: k.created_at,
          revoked_at: k.revoked_at,
        })),
      );
    });
  }, []);

  async function onCreate() {
    const r = await bffFetch<{ key: string; record: ApiKeyInfo }>("/v1/api-keys", { method: "POST", body: {} });
    setCreated({ ...r.record, key: r.key });
    setOpen(true);
  }

  return (
    <Shell>
      <div className="flex flex-col md:flex-row justify-between items-start md:items-end gap-4">
        <div>
          <h2 className="text-2xl md:text-3xl font-semibold text-on-surface">{t.apiKeys.title}</h2>
          <p className="text-base text-on-surface-variant mt-1">{t.apiKeys.subtitle}</p>
        </div>
        <Button onClick={onCreate} data-testid="create-key" className="bg-tertiary text-primary-container px-6 py-2.5 font-mono-label font-bold hover:bg-tertiary-fixed">
          <Plus className="h-4 w-4" /> {t.apiKeys.create}
        </Button>
      </div>

      <Card className="mt-8 bg-surface border-outline-variant" data-testid="keys-table">
        <CardHeader className="pb-2">
          <CardTitle className="font-mono-label text-on-surface text-sm flex items-center gap-2">
            <KeyRound className="h-4 w-4 text-tertiary" /> {t.apiKeys.title}
          </CardTitle>
        </CardHeader>
        <CardContent>
          <table className="w-full text-left font-mono-code text-sm">
            <thead>
              <tr className="text-on-surface-variant border-b border-outline-variant">
                <th className="py-2 pr-4">{t.apiKeys.name}</th>
                <th className="py-2 pr-4">Prefix</th>
                <th className="py-2 pr-4">{t.overview.thLastActive}</th>
                <th className="py-2 pr-4">{t.apiKeys.createdAt}</th>
                <th className="py-2"></th>
              </tr>
            </thead>
            <tbody>
              {keys.map((k) => (
                <tr key={k.id} className="border-b border-outline-variant/60 last:border-0" data-testid="key-row">
                  <td className="py-2 pr-4">
                    <span className="flex items-center gap-2">
                      <span className="w-4 h-4 rounded-sm clip-path-hexagon bg-tertiary/60 inline-block" />
                      <span className="text-on-surface">{k.name}</span>
                    </span>
                  </td>
                  <td className="py-2 pr-4 text-on-surface-variant">{k.prefix}</td>
                  <td className="py-2 pr-4 text-on-surface-variant">{k.last_used}</td>
                  <td className="py-2 pr-4 text-on-surface-variant">{formatTime(k.created_at)}</td>
                  <td className="py-2">
                    {!k.revoked_at && (
                      <Button
                        variant="ghost"
                        size="sm"
                        aria-label={`Revoke ${k.name}`}
                        onClick={async () => {
                          await bffFetch(`/v1/api-keys/${k.id}`, { method: "DELETE" });
                          setKeys((prev) => prev.map((x) => (x.id === k.id ? { ...x, revoked_at: Date.now() / 1000 } : x)));
                        }}
                        className="text-on-surface-variant hover:text-error"
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </CardContent>
      </Card>

      {/* New API Key modal(明文一次性) */}
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="bg-surface-container border-outline-variant" data-testid="create-key-modal">
          <DialogHeader>
            <DialogTitle className="font-mono-label text-on-surface flex items-center gap-2">
              <KeyRound className="h-4 w-4 text-tertiary" /> {t.apiKeys.created}
            </DialogTitle>
            <DialogDescription className="font-mono-label text-on-surface-variant text-xs">
              Development Test
            </DialogDescription>
          </DialogHeader>
          {created && (
            <div className="flex flex-col gap-3">
              <div className="flex items-center gap-2 bg-surface-container-lowest border border-outline-variant rounded p-3">
                <code className="flex-1 font-mono-code text-xs text-on-surface break-all">{created.key}</code>
                <button
                  aria-label="Copy key"
                  className="text-on-surface-variant hover:text-tertiary"
                  onClick={() => {
                    navigator.clipboard?.writeText(created.key);
                    setCopied(true);
                    setTimeout(() => setCopied(false), 1200);
                  }}
                >
                  {copied ? <span className="font-mono-label text-tertiary text-xs">{t.connect.copied}</span> : <Copy className="h-4 w-4" />}
                </button>
              </div>
              <div className="bg-error-container/20 border border-error/40 rounded p-3 font-mono-code text-xs text-error" data-testid="copy-warning">
                <strong>{t.apiKeys.createdOnce}</strong>
              </div>
            </div>
          )}
          <DialogFooter>
            <Button onClick={() => setOpen(false)} className="bg-tertiary text-primary-container font-mono-label font-bold hover:bg-tertiary-fixed">
              {t.apiKeys.done}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Shell>
  );
}
