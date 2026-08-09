"use client";

import { useEffect, useState } from "react";
import { bffFetch } from "@/lib/bff/client";
import { Database, History, Play, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { CodeBlock } from "@/components/code-block";
import { useT } from "@/lib/i18n-context";

interface SqlResult {
  columns: string[];
  rows: Array<Array<string | number | boolean | null>>;
  rows_affected: number;
  truncated?: boolean;
}

/** 极简 SQL 高亮:关键字/字符串着色。 */
function highlight(sql: string): string {
  const esc = sql.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const kw = /\b(SELECT|FROM|WHERE|ORDER BY|AND|COUNT|OVER|DESC|AS|JOIN|LEFT|INNER|ON|CREATE|TABLE|INSERT|INTO|VALUES|UPDATE|SET|DELETE|INTEGER|PRIMARY|KEY|TEXT|NOT|NULL|DEFAULT|IF)\b/g;
  const str = /('([^']*)')/g;
  return esc
    .replace(str, '<span style="color:#98c379">$1</span>')
    .replace(kw, '<span style="color:#c678dd">$1</span>');
}

/** 真实 SQL 工作台:执行 Combee SQL API,渲染真实结果。 */
export function SqlWorkspace({ cellId }: { cellId: string }) {
  const t = useT();
  const [sql, setSql] = useState("SELECT 1");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<SqlResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tables, setTables] = useState<string[]>([]);
  const [loadingTables, setLoadingTables] = useState(true);

  // 载入真实表列表(sqlite_master;前端过滤 __sys 内部表)
  useEffect(() => {
    bffFetch<SqlResult>(`/v1/databases/${cellId}/sql`, {
      method: "POST",
      body: { sql: "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name" },
    })
      .then((r) => setTables(r.rows.map((row) => String(row[0])).filter((n) => !n.startsWith("__sys"))))
      .catch(() => setTables([]))
      .finally(() => setLoadingTables(false));
  }, [cellId]);

  function browseTable(name: string) {
    const quoted = name.replace(/"/g, '""');
    setSql(`SELECT * FROM "${quoted}" LIMIT 100`);
    setResult(null);
    setError(null);
    void run();
  }

  async function run() {
    if (!sql.trim()) return;
    setBusy(true);
    setError(null);
    try {
      // 记录查询历史(BFF → Combee;截断 SQL,不含参数)
      bffFetch("/history", { method: "POST", body: { sql: sql.slice(0, 200) } }).catch(() => undefined);
      const r = await bffFetch<SqlResult>(`/v1/databases/${cellId}/sql`, {
        method: "POST",
        body: { sql },
      });
      setResult(r);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setResult(null);
    } finally {
      setBusy(false);
    }
  }

  function saveSnippet() {
    const title = prompt(t.sql.snippetPrompt, sql.split("\n")[0].slice(0, 40) || "query");
    if (!title) return;
    bffFetch("/snippets", { method: "POST", body: { title, sql } })
      .then(() => alert(`${t.sql.snippetSaved} → ${t.account.tabs.snippets}.`))
      .catch((e) => alert(String(e)));
  }

  return (
    <div className="grid gap-4 md:grid-cols-[240px_1fr]" data-testid="sql-workspace">
      {/* 真实表浏览(DMS 风格) */}
      <div className="bg-surface border border-outline-variant rounded flex flex-col" data-testid="schema-tree">
        <div className="flex items-center justify-between px-4 py-3 border-b border-outline-variant">
          <span className="font-mono-label text-on-surface text-sm flex items-center gap-2">
            <Database className="h-4 w-4 text-tertiary" /> {t.cellDetail.sqlTables}
          </span>
        </div>
        <div className="p-2 flex flex-col gap-0.5 overflow-y-auto max-h-[420px]">
          {loadingTables && <span className="font-mono-label text-on-surface-variant text-xs px-2 py-1">{t.common.loading}</span>}
          {!loadingTables && tables.length === 0 && (
            <span className="font-mono-label text-on-surface-variant text-xs px-2 py-1">{t.cellUsage.empty}</span>
          )}
          {tables.map((name) => (
            <button
              key={name}
              data-testid={`table-${name}`}
              onClick={() => browseTable(name)}
              className="w-full text-left px-2 py-1.5 rounded font-mono-code text-sm text-on-surface hover:bg-surface-container-high transition-colors"
            >
              {name}
            </button>
          ))}
        </div>
      </div>

      <div className="flex flex-col gap-4 min-w-0">
      <div className="bg-surface border border-outline-variant rounded">
        <div className="flex items-center justify-between px-3 py-2 border-b border-outline-variant">
          <div className="flex items-center gap-1">
            <span className="font-mono-label text-xs bg-surface-container-high px-2 py-1 rounded flex items-center gap-1.5">
              query_1.sql <X className="h-3 w-3 text-on-surface-variant" />
            </span>
          </div>
          <div className="flex items-center gap-3 text-on-surface-variant">
            <button aria-label={t.sql.saveSnippet} title={t.sql.saveSnippet} className="hover:text-tertiary" onClick={saveSnippet}>
              <History className="h-4 w-4" />
            </button>
            <Trash2 className="h-4 w-4 hover:text-error cursor-pointer" onClick={() => { setSql(""); setResult(null); setError(null); }} />
            <Button
              onClick={run}
              disabled={busy}
              data-testid="sql-run"
              className="h-8 bg-tertiary text-primary-container font-mono-label text-xs font-bold hover:bg-tertiary-fixed"
            >
              <Play className="h-3.5 w-3.5 mr-1" /> {busy ? t.sql.running : t.sql.run}
            </Button>
          </div>
        </div>
        <div className="relative" data-testid="sql-editor">
          <CodeBlock
            code={sql.length ? sql + "\n" : " "}
            language="sql"
            className="sql-overlay px-3 py-3 pr-2 pointer-events-none min-h-[120px] whitespace-pre"
          />
          <textarea
            value={sql}
            onChange={(e) => setSql(e.target.value)}
            spellCheck={false}
            rows={5}
            aria-label="SQL query"
            className="sql-input absolute inset-0 w-full h-full resize-y bg-transparent p-3 font-mono-code text-sm text-transparent caret-tertiary focus:outline-none focus:ring-0"
          />
        </div>
      </div>

      {error && (
        <div className="bg-surface border border-error/40 rounded px-4 py-3 font-mono-code text-error text-xs" data-testid="sql-error">
          {error}
        </div>
      )}

      {result && (
        <div className="bg-surface border border-outline-variant rounded" data-testid="sql-result">
          <div className="flex items-center justify-between px-4 py-2 border-b border-outline-variant">
            <div className="flex items-center gap-4 font-mono-label text-xs">
              <span className="flex items-center gap-1.5 text-tertiary">
                <span className="w-2 h-2 rounded-sm clip-path-hexagon bg-tertiary" /> {t.overview.stateActive}
              </span>
              <span className="text-on-surface-variant">
                {result.columns.length > 0 ? `${result.rows.length} ${t.sql.rows}` : `${result.rows_affected} ${t.sql.rows}`}
                {result.truncated ? " (truncated)" : ""}
              </span>
            </div>
            <span className="font-mono-label text-xs text-on-surface-variant">Cell: {cellId.slice(0, 8)}…</span>
          </div>
          {result.columns.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-left font-mono-code text-sm">
                <thead>
                  <tr className="text-on-surface-variant border-b border-outline-variant">
                    {result.columns.map((c) => (
                      <th key={c} className="py-2 px-4">{c}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {result.rows.map((row, i) => (
                    <tr key={i} className="border-b border-outline-variant/60 last:border-0">
                      {row.map((v, j) => (
                        <td key={j} className="py-2 px-4 text-on-surface">{String(v)}</td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <div className="px-4 py-3 font-mono-code text-on-surface-variant text-xs">{t.sql.noResult}</div>
          )}
        </div>
      )}
      </div>
    </div>
  );
}
