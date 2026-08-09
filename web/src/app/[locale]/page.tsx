"use client";

import { useState } from "react";
import { useParams } from "next/navigation";
import { ArrowRight, Boxes, Hexagon, KeyRound, Wallet } from "lucide-react";
import Link from "next/link";
import { useT } from "@/lib/i18n-context";
import Image from "next/image";
import { Button } from "@/components/ui/button";
import { CodeBlock } from "@/components/code-block";

const STEPS = [
  { icon: Boxes, title: "create-first-cell", desc: "provision" },
  { icon: KeyRound, title: "create-api-key", desc: "authenticate" },
  { icon: Hexagon, title: "install-sdk", desc: "add-sdk" },
  { icon: Wallet, title: "run-request", desc: "execute" },
];

const TS_CODE = `// 1. Install
npm install @combee/sdk

// 2. Initialize
import { Combee } from '@combee/sdk';
const combee = new Combee({ apiKey: process.env.COMBEE_API_KEY });

// 3. Create your first Cell
const cell = await combee.cells.create({ name: "my-first-app" });`;

const PY_CODE = `# 1. Install
pip install combee

# 2. Initialize
from combee import Combee
combee = Combee(base_url=COMBEE_URL, api_key=COMBEE_API_KEY)

# 3. Create your first Cell
cell = combee.cells.create(name="my-first-app")`;

export default function WelcomePage() {
  const { locale } = useParams<{ locale: string }>();
  const t = useT();
  const [tab, setTab] = useState<"ts" | "py">("ts");

  return (
    <div className="min-h-screen bg-background flex items-center justify-center p-6">
      <div className="max-w-4xl w-full grid md:grid-cols-12 gap-10">
        {/* 左侧:标题 + 步骤 */}
        <div className="md:col-span-5 flex flex-col justify-center">
          <div className="flex items-center gap-3 mb-6">
            <Image src="/combee-192.png" alt="Combee Cloud" width={40} height={40} className="h-10 w-10 object-contain" priority />
            <span className="font-mono-code uppercase tracking-widest text-on-surface font-bold">Combee Cloud</span>
          </div>
          <h1 className="text-3xl font-bold text-on-surface leading-tight">{t.welcome.title}</h1>
          <p className="font-mono-code text-on-surface-variant mt-3 text-sm">
            {t.welcome.subtitle}
          </p>

          <div className="mt-8 flex flex-col" data-testid="quickstart-steps">
            {STEPS.map((s, i) => {
              const Icon = s.icon;
              return (
                <div key={s.title} className="flex gap-4">
                  <div className="flex flex-col items-center">
                    <span
                      className={`w-7 h-7 rounded-full border flex items-center justify-center ${
                        i === 0 ? "border-tertiary bg-tertiary/15 text-tertiary" : "border-outline-variant text-on-surface-variant"
                      }`}
                    >
                      <Icon className="h-3.5 w-3.5" />
                    </span>
                    {i < STEPS.length - 1 && <span className="w-px flex-1 bg-outline-variant/60 my-1" />}
                  </div>
                  <div className="pb-6">
                    <div className={`font-mono-label ${i === 0 ? "text-tertiary" : "text-on-surface"}`}>{t.welcome.steps[i].title}</div>
                    <div className="font-mono-code text-on-surface-variant text-xs mt-0.5">{t.welcome.steps[i].desc}</div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>

        {/* 右侧:代码卡 */}
        <div className="md:col-span-7 flex flex-col justify-center">
          <div className="bg-surface border border-outline-variant rounded overflow-hidden shadow-[0_0_30px_rgba(255,185,95,0.06)]" data-testid="code-card">
            <div className="flex items-center gap-2 px-4 py-3 border-b border-outline-variant">
              <span className="w-3 h-3 rounded-full bg-error/70" />
              <span className="w-3 h-3 rounded-full bg-tertiary/70" />
              <span className="w-3 h-3 rounded-full bg-secondary/50" />
              <div className="ml-4 flex gap-1">
                {(
                  [
                    ["ts", "TypeScript"],
                    ["py", "Python"],
                  ] as const
                ).map(([k, label]) => (
                  <button
                    key={k}
                    onClick={() => setTab(k)}
                    className={`px-3 py-1 rounded font-mono-label text-xs transition-colors ${
                      tab === k ? "bg-tertiary-container text-tertiary" : "text-on-surface-variant hover:text-on-surface"
                    }`}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>
            <CodeBlock
              code={tab === "ts" ? TS_CODE : PY_CODE}
              language={tab === "ts" ? "tsx" : "python"}
              className="p-5 text-sm"
            />
          </div>

          <div className="flex gap-3 mt-6">
            <Button asChild className="bg-tertiary text-primary-container font-mono-label font-bold hover:bg-tertiary-fixed">
              <Link href={`/${locale}/overview`}>
                {t.welcome.continue} <ArrowRight className="ml-2 h-4 w-4" />
              </Link>
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
