"use client";

import { use, useState } from "react";
import Link from "next/link";
import { useT } from "@/lib/i18n-context";
import { useParams } from "next/navigation";
import { ArrowLeft, Copy, Terminal } from "lucide-react";
import Shell from "@/components/shell";
import { Button } from "@/components/ui/button";
import { CodeBlock } from "@/components/code-block";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { shortId } from "@/lib/utils";

const SNIPPETS = {
  ts: `import { Combee } from "@combee/sdk";

const combee = new Combee({
  baseUrl: process.env.COMBEE_URL!,
  apiKey: process.env.COMBEE_API_KEY!,
});

const cell = combee.cell("CELL_ID");
await cell.kv.set("greeting", "hello", { ttl: 3600 });
await cell.sql.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)");`,
  py: `from combee import Combee

combee = Combee(base_url=COMBEE_URL, api_key=COMBEE_API_KEY)
cell = combee.cell("CELL_ID")

cell.kv.set("greeting", "hello", ttl=3600)
cell.sql.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")`,
  http: `curl -X PUT $COMBEE_URL/v1/databases/CELL_ID/kv/greeting \\
  -H 'content-type: application/json' \\
  -H 'x-api-key: $COMBEE_API_KEY' \\
  -d '{"value":"hello","ttl_seconds":3600}'`,
};

export default function ConnectCellPage({ params }: { params: Promise<{ id: string }> }) {
  const t = useT();
  const { locale } = useParams<{ locale: string }>();
  const { id } = use(params);
  const [tab, setTab] = useState<"ts" | "py" | "http">("ts");
  const [copiedId, setCopiedId] = useState(false);

  const code = SNIPPETS[tab].replaceAll("CELL_ID", id);
  const apiBase = process.env.NEXT_PUBLIC_COMBEE_API_URL ?? "https://api.combee.cloud";

  return (
    <Shell>
      <Link href={`/${locale}/cells/${id}`} className="font-mono-label text-on-surface-variant hover:text-tertiary flex items-center gap-1">
        <ArrowLeft className="h-4 w-4" /> {t.cellDetail.backToCells.replace("Cells", t.cellDetail.back + " " + t.shell.cells)}
      </Link>
      <h2 className="text-2xl md:text-3xl font-semibold text-on-surface mt-2">{t.connect.title}</h2>
      <p className="text-base text-on-surface-variant mt-1">{t.connect.subtitle}</p>

      {/* 信息卡 */}
      <div className="grid md:grid-cols-2 gap-4 mt-6">
        <Card className="bg-surface border-outline-variant">
          <CardContent className="pt-4 flex items-center justify-between">
            <div>
              <div className="font-mono-label text-on-surface-variant uppercase tracking-wider text-xs">{t.cells.idLabel}</div>
              <div className="font-mono-code text-on-surface mt-1 break-all">{id}</div>
            </div>
            <button
              aria-label="Copy Cell ID"
              className="text-on-surface-variant hover:text-tertiary"
              onClick={() => {
                navigator.clipboard?.writeText(id);
                setCopiedId(true);
                setTimeout(() => setCopiedId(false), 1200);
              }}
            >
              {copiedId ? <span className="font-mono-label text-tertiary text-xs">{t.connect.copied}</span> : <Copy className="h-4 w-4" />}
            </button>
          </CardContent>
        </Card>
        <Card className="bg-surface border-outline-variant">
          <CardContent className="pt-4">
            <div className="font-mono-label text-on-surface-variant uppercase tracking-wider text-xs">API Base URL</div>
            <div className="font-mono-code text-tertiary mt-1 break-all">{apiBase}</div>
          </CardContent>
        </Card>
      </div>

      {/* 代码卡 */}
      <Card className="mt-6 bg-surface border-outline-variant overflow-hidden" data-testid="connect-code">
        <CardHeader className="pb-0 pt-3 px-0">
          <div className="flex items-center gap-1 px-4">
            {(
              [
                ["ts", t.connect.ts],
                ["py", t.connect.py],
                ["http", t.connect.http],
              ] as const
            ).map(([k, label]) => (
              <button
                key={k}
                onClick={() => setTab(k)}
                className={`px-3 py-2 rounded-t font-mono-label text-xs transition-colors ${
                  tab === k ? "bg-surface-container text-tertiary border-b-2 border-tertiary" : "text-on-surface-variant hover:text-on-surface"
                }`}
              >
                {label}
              </button>
            ))}
            <div className="ml-auto pr-2">
              <button aria-label="Copy" className="text-on-surface-variant hover:text-tertiary flex items-center gap-1" onClick={() => navigator.clipboard?.writeText(code)}>
                <Copy className="h-4 w-4" />
              </button>
            </div>
          </div>
        </CardHeader>
        <CardContent className="pt-0">
          <div className="bg-surface-container-low/60 border border-outline-variant rounded-b px-4 py-3">
            <Terminal className="h-3.5 w-3.5 inline-block mr-2 text-tertiary" />
            <CodeBlock
              code={code}
              language={tab === "py" ? "python" : tab === "http" ? "bash" : "tsx"}
              className="text-xs"
            />
          </div>
        </CardContent>
      </Card>


    </Shell>
  );
}
