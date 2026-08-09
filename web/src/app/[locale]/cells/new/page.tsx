"use client";

import { useT } from "@/lib/i18n-context";
import { useState } from "react";
import { useRouter, useParams } from "next/navigation";
import { Loader2 } from "lucide-react";
import Shell from "@/components/shell";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { bffFetch } from "@/lib/bff/client";

export default function CreateCellPage() {
  const t = useT();
  const { locale } = useParams<{ locale: string }>();
  const router = useRouter();
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    const n = name.trim();
    if (!n) {
      setError(t.cellNew.nameRequired);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      // ensure:同名 Cell 幂等复用,不报 409
      const r = await bffFetch<{ cell: { id: string }; created: boolean }>(
        `/v1/databases/by-name/${encodeURIComponent(n)}`,
        { method: "PUT" },
      );
      router.push(`/${locale}/cells/${r.cell.id}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "create failed");
      setBusy(false);
    }
  }

  return (
    <Shell>
      <div>
        <h2 className="text-2xl md:text-3xl font-semibold text-on-surface">{t.cellNew.title}</h2>
        <p className="text-base text-on-surface-variant mt-1">{t.cellNew.subtitle}</p>
      </div>

      <Card className="mt-8 max-w-xl bg-surface border-secondary-container" data-testid="create-cell-form">
        <CardHeader>
          <CardTitle className="font-mono-label text-on-surface">{t.cells.newCell}</CardTitle>
          <CardDescription>{t.cellNew.lazyHint}</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={onSubmit} className="flex flex-col gap-5">
            <div className="grid gap-2">
              <label htmlFor="cell-name" className="font-mono-label text-on-surface-variant">{t.cellNew.name}</label>
              <input
                id="cell-name"
                data-testid="cell-name-input"
                placeholder={t.cellNew.namePlaceholder}
                value={name}
                onChange={(e) => setName(e.target.value)}
                className="h-9 w-full rounded bg-surface-container-lowest border border-outline-variant px-3 text-sm text-on-surface font-mono-code focus:outline-none focus:ring-1 focus:ring-tertiary"
              />
              <p className="font-mono-code text-on-surface-variant text-xs">{t.cellNew.ensureHint}</p>
            </div>
            {error && <p className="font-mono-code text-error text-sm">{error}</p>}
            <Button
              type="submit"
              disabled={busy}
              data-testid="create-cell-submit"
              className="bg-tertiary text-primary-container font-mono-label font-bold hover:bg-tertiary-fixed disabled:opacity-60"
            >
              {busy && <Loader2 className="h-4 w-4 animate-spin" />}
              {t.cellNew.create}
            </Button>
          </form>
        </CardContent>
      </Card>
    </Shell>
  );
}
