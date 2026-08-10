"use client";

import { useState } from "react";
import { useRouter, useParams } from "next/navigation";
import Link from "next/link";
import { useT } from "@/lib/i18n-context";
import { Loader2, LogIn } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import Image from "next/image";

export default function LoginPage() {
  const t = useT();
  const { locale } = useParams<{ locale: string }>();
  const router = useRouter();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const res = await fetch("/api/bff/auth/login", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ username: username.trim(), password }),
      });
      if (!res.ok) {
        const body = (await res.json().catch(() => ({}))) as { error?: string };
        throw new Error(body.error ?? "login failed");
      }
      router.push(`/${locale}/overview`);
      router.refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "login failed");
      setBusy(false);
    }
  }

  return (
    <div className="min-h-screen bg-background flex items-center justify-center p-6">
      <div className="w-full max-w-md">
        <div className="flex items-center justify-center gap-3 mb-8">
          <Image src="/combee-64.png" alt="Combee Cloud" width={40} height={40} className="h-10 w-10 object-contain" priority />
          <span className="font-mono-code uppercase tracking-widest text-on-surface font-bold text-lg">Combee Cloud</span>
        </div>
        <Card className="bg-surface border-outline-variant" data-testid="login-card">
          <CardHeader>
            <CardTitle className="font-mono-label text-on-surface flex items-center gap-2">
              <LogIn className="h-4 w-4 text-tertiary" /> {t.login.title}
            </CardTitle>

          </CardHeader>
          <CardContent>
            <form onSubmit={onSubmit} className="flex flex-col gap-5">
              <div className="grid gap-2">
                <Label htmlFor="username" className="font-mono-label text-on-surface-variant">{t.login.username}</Label>
                <Input
                  id="username"
                  data-testid="login-username"
                  placeholder="alice"
                  autoComplete="username"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  className="bg-surface-container-lowest border-outline-variant font-mono-code"
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="password" className="font-mono-label text-on-surface-variant">{t.login.password}</Label>
                <Input
                  id="password"
                  data-testid="login-password"
                  type="password"
                  autoComplete="current-password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  className="bg-surface-container-lowest border-outline-variant font-mono-code"
                />
              </div>
              {error && <p className="font-mono-code text-error text-sm" data-testid="login-error">{error}</p>}
              <Button
                type="submit"
                disabled={busy || username.trim().length < 3 || password.length < 8}
                data-testid="login-submit"
                className="bg-tertiary text-primary-container font-mono-label font-bold hover:bg-tertiary-fixed disabled:opacity-60"
              >
                {busy && <Loader2 className="h-4 w-4 animate-spin mr-1" />}
                {t.login.signIn}
              </Button>
            </form>
          </CardContent>
        </Card>
        <p className="text-center font-mono-label text-on-surface-variant mt-5">
          {t.login.noAccount}{" "}
          <Link href={`/${locale}/register`} className="text-tertiary hover:text-tertiary-fixed" data-testid="goto-register">
            {t.login.createAccount}
          </Link>
        </p>
      </div>
    </div>
  );
}
