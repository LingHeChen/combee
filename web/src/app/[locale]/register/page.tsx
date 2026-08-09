"use client";

import { useState } from "react";
import { useRouter, useParams } from "next/navigation";
import Link from "next/link";
import { useT } from "@/lib/i18n-context";
import { Loader2, UserPlus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import Image from "next/image";
import { CodeBlock } from "@/components/code-block";

export default function RegisterPage() {
  const t = useT();
  const { locale } = useParams<{ locale: string }>();
  const router = useRouter();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [accessCode, setAccessCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [apiKey, setApiKey] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (password !== confirm) {
      setError(t.register.passwordMismatch);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const res = await fetch("/api/bff/auth/register", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ username: username.trim(), password, access_code: accessCode.trim() }),
      });
      if (!res.ok) {
        const body = (await res.json().catch(() => ({}))) as { error?: string };
        throw new Error(body.error ?? "registration failed");
      }
      const body = (await res.json().catch(() => ({}))) as { apiKey?: string };
      // 注册成功即已登录(register 同时签发会话)
      if (body.apiKey) {
        setApiKey(body.apiKey); // 展示一次,不再跳转
      } else {
        router.push(`/${locale}/overview`);
      }
      router.refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "registration failed");
      setBusy(false);
    }
  }

  if (apiKey) {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center p-6" data-testid="register-done">
        <div className="w-full max-w-xl">
          <div className="flex items-center justify-center gap-3 mb-6">
            <Image src="/combee-64.png" alt="Combee Cloud" width={40} height={40} className="h-10 w-10 object-contain" priority />
            <span className="font-mono-code uppercase tracking-widest text-on-surface font-bold text-lg">Combee Cloud</span>
          </div>
          <h1 className="text-2xl font-bold text-on-surface text-center">{t.register.doneWelcome}</h1>
          <p className="text-center font-mono-code text-on-surface-variant text-sm mt-2">{t.register.doneSub}</p>

          <Card className="bg-surface border-outline-variant mt-8" data-testid="api-key-card">
            <CardHeader className="pb-2">
              <CardTitle className="font-mono-label text-on-surface text-sm">{t.register.yourApiKey}</CardTitle>
              <CardDescription className="font-mono-code text-on-surface-variant text-xs">{t.register.onlyOnce}</CardDescription>
            </CardHeader>
            <CardContent className="flex items-center gap-2">
              <code className="flex-1 break-all rounded border border-outline-variant bg-surface-container-lowest px-3 py-2 font-mono-code text-sm text-tertiary" data-testid="api-key-value">
                {apiKey}
              </code>
              <Button
                variant="outline"
                className="font-mono-label border-outline-variant text-on-surface-variant hover:text-tertiary"
                data-testid="copy-api-key"
                onClick={() => {
                  try {
                    navigator.clipboard?.writeText(apiKey);
                  } catch {
                    // 无剪贴板权限时降级
                    const ta = document.createElement("textarea");
                    ta.value = apiKey;
                    document.body.appendChild(ta);
                    ta.select();
                    document.execCommand("copy");
                    ta.remove();
                  }
                  setCopied(true);
                  setTimeout(() => setCopied(false), 1500);
                }}
              >
                {copied ? t.register.copied : t.register.copy}
              </Button>
            </CardContent>
          </Card>

          <div className="mt-4">
            <CodeBlock
              code={`const combee = new Combee({ apiKey: "${apiKey.slice(0, 12)}…" });
const cell = await combee.cells.ensure("my-app");
await cell.kv.set("hello", "world");`}
              language="tsx"
              className="text-xs"
            />
          </div>

          <div className="flex justify-center mt-6">
            <Button
              className="bg-tertiary text-primary-container font-mono-label font-bold hover:bg-tertiary-fixed"
              data-testid="go-dashboard"
              onClick={() => router.push(`/${locale}/overview`)}
            >
              {t.register.goToDashboard}
            </Button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-background flex items-center justify-center p-6">
      <div className="w-full max-w-md">
        <div className="flex items-center justify-center gap-3 mb-8">
          <Image src="/combee-64.png" alt="Combee Cloud" width={40} height={40} className="h-10 w-10 object-contain" priority />
          <span className="font-mono-code uppercase tracking-widest text-on-surface font-bold text-lg">Combee Cloud</span>
        </div>
        <Card className="bg-surface border-outline-variant" data-testid="register-card">
          <CardHeader>
            <CardTitle className="font-mono-label text-on-surface flex items-center gap-2">
              <UserPlus className="h-4 w-4 text-tertiary" /> {t.register.title}
            </CardTitle>
            <CardDescription className="font-mono-code text-on-surface-variant text-xs">
              {t.register.subtitle}
            </CardDescription>
          </CardHeader>
          <CardContent>
            <form onSubmit={onSubmit} className="flex flex-col gap-5">
              <div className="grid gap-2">
                <Label htmlFor="username" className="font-mono-label text-on-surface-variant">{t.register.username}</Label>
                <Input
                  id="username"
                  data-testid="register-username"
                  placeholder="alice (3-32 chars: a-z 0-9 . _ -)"
                  autoComplete="username"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  className="bg-surface-container-lowest border-outline-variant font-mono-code"
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="password" className="font-mono-label text-on-surface-variant">{t.register.password}</Label>
                <Input
                  id="password"
                  data-testid="register-password"
                  type="password"
                  autoComplete="new-password"
                  placeholder="at least 8 characters"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  className="bg-surface-container-lowest border-outline-variant font-mono-code"
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="access_code" className="font-mono-label text-on-surface-variant flex items-center gap-2">
                  {t.register.accessCode}
                  <span className="px-1 py-0.5 rounded-full border border-tertiary/50 text-tertiary text-[10px]">{t.register.accessCodeHint}</span>
                </Label>
                <Input
                  id="access_code"
                  data-testid="register-access-code"
                  placeholder="CMB-XXXX-XXXX-XXXX"
                  autoComplete="off"
                  value={accessCode}
                  onChange={(e) => setAccessCode(e.target.value)}
                  className="bg-surface-container-lowest border-outline-variant font-mono-code"
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="confirm" className="font-mono-label text-on-surface-variant">{t.register.confirm}</Label>
                <Input
                  id="confirm"
                  data-testid="register-confirm"
                  type="password"
                  autoComplete="new-password"
                  value={confirm}
                  onChange={(e) => setConfirm(e.target.value)}
                  className="bg-surface-container-lowest border-outline-variant font-mono-code"
                />
              </div>
              {error && <p className="font-mono-code text-error text-sm" data-testid="register-error">{error}</p>}
              <Button
                type="submit"
                disabled={busy || username.trim().length < 3 || password.length < 8 || accessCode.trim().length < 12}
                data-testid="register-submit"
                className="bg-tertiary text-primary-container font-mono-label font-bold hover:bg-tertiary-fixed disabled:opacity-60"
              >
                {busy && <Loader2 className="h-4 w-4 animate-spin mr-1" />}
                {t.register.signUp}
              </Button>
            </form>
          </CardContent>
        </Card>
        <p className="text-center font-mono-label text-on-surface-variant mt-5">
          {t.register.haveAccount}{" "}
          <Link href={`/${locale}/login`} className="text-tertiary hover:text-tertiary-fixed">
            {t.register.signIn}
          </Link>
        </p>
      </div>
    </div>
  );
}
