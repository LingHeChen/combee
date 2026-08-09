"use client";

import { useT } from "@/lib/i18n-context";
import { useEffect, useState } from "react";
import { ArrowDownLeft, ArrowUpRight, Wallet } from "lucide-react";
import Shell from "@/components/shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { bffFetch } from "@/lib/bff/client";
import type { CreditBalance, CreditTransaction } from "@/lib/types";
import { formatTime } from "@/lib/utils";

export default function CreditsPage() {
  const t = useT();
  const [balance, setBalance] = useState<CreditBalance | null>(null);
  const [txns, setTxns] = useState<CreditTransaction[]>([]);
  const [code, setCode] = useState("");
  const [redeemed, setRedeemed] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    bffFetch<CreditBalance>("/v1/credits/balance").then(setBalance);
    bffFetch<{ items: CreditTransaction[] }>("/v1/credits/transactions?limit=100").then((p) => setTxns(p.items));
  }, []);

  const credits = balance ? (Number(balance.available) / 1_000_000).toFixed(2) : "0.00";

  return (
    <Shell>
      <div className="flex flex-col md:flex-row justify-between items-start md:items-end gap-4">
        <div>
          <h2 className="text-2xl md:text-3xl font-semibold text-on-surface">{t.credits.title}</h2>
          <p className="text-base text-on-surface-variant mt-1">{t.credits.subtitle}</p>
        </div>
        <div className="flex gap-2">
          <Button
            data-testid="redeem-open"
            className="bg-tertiary text-primary-container font-mono-label font-bold hover:bg-tertiary-fixed"
            onClick={() => document.getElementById("redeem-form")?.scrollIntoView({ behavior: "smooth" })}
          >
            {t.credits.redeem}
          </Button>
        </div>
      </div>

      {/* Bento */}
      <div className="grid md:grid-cols-1 gap-4 mt-8">
        <Card className="bg-surface border-tertiary/40 relative overflow-hidden max-w-xl" data-testid="balance-card">
          <div className="absolute -right-6 -top-10 w-32 h-32 border-2 border-tertiary/15 rotate-45" />
          <CardHeader className="pb-2">
            <CardTitle className="font-mono-label text-on-surface text-sm flex items-center gap-2">
              <Wallet className="h-4 w-4 text-tertiary" /> {t.credits.available}
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-4xl font-bold text-on-surface">{credits} <span className="text-lg text-on-surface-variant font-mono-label">CRD</span></div>
            <div className="font-mono-code text-on-surface-variant text-xs mt-2">reserved: {balance?.reserved ?? "0"} µcr · updated {balance ? formatTime(balance.updated_at) : "—"}</div>
          </CardContent>
        </Card>
      </div>

      {/* Redeem */}
      <Card className="mt-6 bg-surface border-outline-variant" id="redeem-form" data-testid="redeem-card">
        <CardHeader className="pb-2">
          <CardTitle className="font-mono-label text-on-surface text-sm">{t.credits.redeem}</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div className="flex gap-2 max-w-xl">
            <Input
              data-testid="voucher-code"
              placeholder="CMB-XXXX-XXXX-XXXX"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              className="bg-surface-container-lowest border-outline-variant font-mono-code"
            />
            <Button
              data-testid="redeem-button"
              disabled={code.trim().length < 12}
              onClick={async () => {
                try {
                  const r = await bffFetch<{ credits_added: string; balance: string }>("/v1/credits/redeem", {
                    method: "POST",
                    body: { code: code.trim() },
                  });
                  setRedeemed(`${t.credits.redeemed}: +${(Number(r.credits_added) / 1_000_000).toFixed(2)} CRD`);
                  setCode("");
                  bffFetch<CreditBalance>("/v1/credits/balance").then(setBalance);
                  bffFetch<{ items: CreditTransaction[] }>("/v1/credits/transactions?limit=100").then((p) => setTxns(p.items));
                } catch (e) {
                  setRedeemed(null);
                  setError(e instanceof Error ? e.message : String(e));
                }
              }}
              className="bg-tertiary text-primary-container font-mono-label font-bold hover:bg-tertiary-fixed"
            >
              Redeem
            </Button>
          </div>
          {redeemed && <div className="font-mono-label text-tertiary" data-testid="redeem-result">{redeemed}</div>}
          {error && <div className="font-mono-label text-error text-xs" data-testid="redeem-error">{error}</div>}
        </CardContent>
      </Card>

      {/* 交易表 */}
      <Card className="mt-8 bg-surface border-outline-variant" data-testid="ledger">
        <CardHeader className="pb-2 flex flex-row items-center justify-between">
          <CardTitle className="font-mono-label text-on-surface text-sm">{t.credits.transactions}</CardTitle>
          <button className="font-mono-label text-xs text-on-surface-variant hover:text-tertiary">{t.credits.viewOlder}</button>
        </CardHeader>
        <CardContent>
          <table className="w-full text-left font-mono-code text-sm">
            <thead>
              <tr className="text-on-surface-variant border-b border-outline-variant">
                <th className="py-2 pr-4">{t.credits.thDate}</th>
                <th className="py-2 pr-4">{t.credits.type}</th>
                <th className="py-2 pr-4">{t.credits.thDescription}</th>
                <th className="py-2 pr-4 text-right">{t.credits.amount}</th>
                <th className="py-2 text-right">{t.credits.thBalance}</th>
              </tr>
            </thead>
            <tbody>
              {txns.map((t) => {
                const amount = Number(t.amount_units) / 1_000_000;
                return (
                  <tr key={t.id} className="border-b border-outline-variant/60 last:border-0" data-testid="txn-row">
                    <td className="py-2 pr-4 text-on-surface-variant">{formatTime(t.created_at)}</td>
                    <td className="py-2 pr-4">
                      <span className={`px-2 py-0.5 rounded-full border text-xs ${t.txn_type === "usage" ? "border-outline-variant text-on-surface-variant" : "border-tertiary/50 text-tertiary"}`}>
                        {t.txn_type}
                      </span>
                    </td>
                    <td className="py-2 pr-4 text-on-surface-variant">{t.description ?? "—"}</td>
                    <td className={`py-2 pr-4 text-right flex items-center justify-end gap-1 ${amount < 0 ? "text-error" : "text-tertiary"}`}>
                      {amount < 0 ? <ArrowUpRight className="h-3.5 w-3.5" /> : <ArrowDownLeft className="h-3.5 w-3.5" />}
                      {amount.toFixed(2)}
                    </td>
                    <td className="py-2 text-right text-on-surface-variant">
                      {t.balance_after ? (Number(t.balance_after) / 1_000_000).toFixed(2) : "—"}
                    </td>
                  </tr>
                );
              })}
              {txns.length === 0 && (
                <tr><td colSpan={5} className="py-4 text-on-surface-variant">{t.credits.empty}</td></tr>
              )}
            </tbody>
          </table>
        </CardContent>
      </Card>
    </Shell>
  );
}
