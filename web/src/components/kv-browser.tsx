"use client";

import { useState } from "react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { bffFetch } from "@/lib/bff/client";
import { useT } from "@/lib/i18n-context";

interface KvGetResult {
  exists: boolean;
  value?: string | null;
  ttl_seconds?: number | null;
}

interface KvScanResult {
  keys: string[];
  next_cursor: string;
}

/** 真实 KV 浏览器:浏览模式(scan 列 key,点 key 看值)+ 单 key 操作。 */
export function KvBrowser({ cellId }: { cellId: string }) {
  const t = useT();
  const [key, setKey] = useState("");
  const [value, setValue] = useState("");
  const [ttl, setTtl] = useState("");
  const [result, setResult] = useState<{ kind: "ok" | "err"; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [mode, setMode] = useState<"browse" | "operate">("browse");
  const [keys, setKeys] = useState<string[]>([]);
  const [nextCursor, setNextCursor] = useState("");
  const [prefix, setPrefix] = useState("");
  const [browsing, setBrowsing] = useState(false);

  async function loadKeys(cursor = "") {
    setBrowsing(true);
    try {
      const q = new URLSearchParams({ limit: "50" });
      if (prefix.trim()) q.set("prefix", prefix.trim());
      if (cursor) q.set("cursor", cursor);
      const r = await bffFetch<KvScanResult>(`/v1/databases/${cellId}/kv?${q}`);
      setKeys(r.keys);
      setNextCursor(r.next_cursor);
    } catch (e) {
      setResult({ kind: "err", text: e instanceof Error ? e.message : String(e) });
    } finally {
      setBrowsing(false);
    }
  }

  function pickKey(k: string) {
    setKey(k);
    setMode("operate");
    void doGet(k);
  }

  async function doGet(targetKey?: string) {
    const k = (targetKey ?? key).trim();
    if (!k) return setResult({ kind: "err", text: t.kv.keyRequired });
    setBusy(true);
    try {
      const r = await bffFetch<KvGetResult>(`/v1/databases/${cellId}/kv/${encodeURIComponent(k)}`);
      setResult(
        r.exists
          ? { kind: "ok", text: `value = ${JSON.stringify(r.value)}${r.ttl_seconds != null ? ` · ttl=${r.ttl_seconds}s` : ""}` }
          : { kind: "err", text: t.kv.notFound },
      );
      if (r.exists && r.value != null) setValue(r.value);
    } catch (e) {
      setResult({ kind: "err", text: e instanceof Error ? e.message : String(e) });
    } finally {
      setBusy(false);
    }
  }

  async function doSet() {
    if (!key.trim()) return setResult({ kind: "err", text: t.kv.keyRequired });
    setBusy(true);
    try {
      const body: { value: string; ttl_seconds?: number } = { value };
      if (ttl.trim() && Number(ttl) > 0) body.ttl_seconds = Number(ttl);
      await bffFetch<{ written: boolean }>(`/v1/databases/${cellId}/kv/${encodeURIComponent(key.trim())}`, {
        method: "PUT",
        body,
      });
      setResult({ kind: "ok", text: `${t.kv.set} ✓` });
    } catch (e) {
      setResult({ kind: "err", text: e instanceof Error ? e.message : String(e) });
    } finally {
      setBusy(false);
    }
  }

  async function doDelete() {
    if (!key.trim()) return setResult({ kind: "err", text: t.kv.keyRequired });
    setBusy(true);
    try {
      await bffFetch<{ deleted: boolean }>(`/v1/databases/${cellId}/kv/${encodeURIComponent(key.trim())}`, {
        method: "DELETE",
      });
      setResult({ kind: "ok", text: `${t.kv.del} ✓` });
      setValue("");
    } catch (e) {
      setResult({ kind: "err", text: e instanceof Error ? e.message : String(e) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex flex-col gap-4" data-testid="kv-browser">
      <div className="flex items-center gap-2">
        <button
          data-testid="kv-mode-browse"
          onClick={() => {
            setMode("browse");
            if (keys.length === 0) void loadKeys();
          }}
          className={`px-3 py-1.5 rounded font-mono-label text-xs ${
            mode === "browse" ? "bg-tertiary-container text-tertiary" : "text-on-surface-variant"
          }`}
        >
          {t.kv.browse}
        </button>
        <button
          data-testid="kv-mode-operate"
          onClick={() => setMode("operate")}
          className={`px-3 py-1.5 rounded font-mono-label text-xs ${
            mode === "operate" ? "bg-tertiary-container text-tertiary" : "text-on-surface-variant"
          }`}
        >
          {t.kv.singleKey}
        </button>
      </div>

      {mode === "browse" ? (
        <div className="flex flex-col gap-3" data-testid="kv-browse">
          <div className="flex gap-2">
            <Input
              data-testid="kv-prefix"
              placeholder="user:*"
              value={prefix}
              onChange={(e) => setPrefix(e.target.value)}
              className="bg-surface-container-low border-outline-variant font-mono-code text-sm"
            />
            <Button data-testid="kv-scan" onClick={() => void loadKeys()} disabled={browsing} className="bg-tertiary text-primary-container font-mono-label font-bold hover:bg-tertiary-fixed">
              {t.kv.scan}
            </Button>
          </div>
          <div className="bg-surface border border-outline-variant rounded overflow-hidden">
            {browsing && keys.length === 0 ? (
              <div className="py-6 text-center font-mono-label text-on-surface-variant text-xs">{t.common.loading}</div>
            ) : keys.length === 0 ? (
              <div className="py-6 text-center font-mono-label text-on-surface-variant text-xs">{t.cellUsage.empty}</div>
            ) : (
              <ul className="divide-y divide-outline-variant/60 font-mono-code text-sm">
                {keys.map((k) => (
                  <li key={k}>
                    <button
                      data-testid={`kv-key-${k}`}
                      onClick={() => pickKey(k)}
                      className="w-full text-left px-4 py-2 text-tertiary hover:bg-surface-container-low transition-colors"
                    >
                      {k}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
          {nextCursor && (
            <Button variant="outline" onClick={() => void loadKeys(nextCursor)} disabled={browsing} className="font-mono-label border-outline-variant text-on-surface-variant hover:text-tertiary">
              {t.credits.viewOlder}
            </Button>
          )}
        </div>
      ) : (
        <div className="grid gap-3">
          <div className="grid gap-2">
            <div className="font-mono-label text-on-surface-variant uppercase tracking-wider text-xs">{t.kv.key}</div>
            <Input
              data-testid="kv-key"
              placeholder="user:1042:session"
              value={key}
              onChange={(e) => setKey(e.target.value)}
              className="bg-surface-container-low border-outline-variant font-mono-code text-sm"
            />
          </div>
          <div className="grid gap-2">
            <div className="font-mono-label text-on-surface-variant uppercase tracking-wider text-xs">{t.kv.value}</div>
            <Input
              data-testid="kv-value"
              placeholder='{"token":"...","exp":1715424000}'
              value={value}
              onChange={(e) => setValue(e.target.value)}
              className="bg-surface-container-low border-outline-variant font-mono-code text-sm"
            />
          </div>
          <div className="grid gap-2">
            <div className="font-mono-label text-on-surface-variant uppercase tracking-wider text-xs">{t.kv.ttl}</div>
            <Input
              data-testid="kv-ttl"
              placeholder={t.kv.ttlHint}
              value={ttl}
              onChange={(e) => setTtl(e.target.value)}
              className="bg-surface-container-low border-outline-variant font-mono-code text-sm"
            />
          </div>
          <div className="flex gap-2">
            <Button data-testid="kv-get" onClick={() => void doGet()} disabled={busy} className="flex-1 bg-tertiary text-primary-container font-mono-label font-bold hover:bg-tertiary-fixed">
              {t.kv.get}
            </Button>
            <Button data-testid="kv-set" onClick={() => void doSet()} disabled={busy} variant="outline" className="flex-1 font-mono-label border-outline-variant text-on-surface-variant hover:text-tertiary">
              {t.kv.set}
            </Button>
            <Button data-testid="kv-del" onClick={() => void doDelete()} disabled={busy} variant="outline" className="flex-1 font-mono-label border-outline-variant text-error hover:text-error">
              {t.kv.del}
            </Button>
          </div>
          {result && (
            <div
              data-testid="kv-result"
              className={`rounded border px-4 py-3 font-mono-code text-xs ${
                result.kind === "ok" ? "border-tertiary/40 text-tertiary" : "border-error/40 text-error"
              }`}
            >
              {result.text}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
