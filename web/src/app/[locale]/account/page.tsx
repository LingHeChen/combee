"use client";

import { useT } from "@/lib/i18n-context";
import { useEffect, useState } from "react";
import { Copy, History, KeyRound, Loader2, ShieldCheck, Trash2, UserRound } from "lucide-react";
import Shell from "@/components/shell";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { bffFetch } from "@/lib/bff/client";
import { formatTime } from "@/lib/utils";

interface ProfileData {
  username: string;
  display_name: string | null;
  avatar: string | null;
  locale: string;
  timezone: string;
  prefs: { default_range: string; default_region: string; table_page_size: number; ui: Record<string, unknown> };
}
interface Onboarding {
  first_cell_created: boolean;
  api_key_created: boolean;
  first_request_made: boolean;
  completed: boolean;
}
interface Snippet {
  id: string;
  title: string;
  sql: string;
  created_at: number;
}
interface RecentCell {
  cell_id: string;
  last_visited: number;
}
interface HistoryItem {
  sql: string;
  created_at: number;
}
interface UserData {
  profile: ProfileData;
  onboarding: Onboarding;
  snippets: Snippet[];
  recent: RecentCell[];
  history: HistoryItem[];
}

export default function AccountPage() {
  const t = useT();
  const [data, setData] = useState<UserData | null>(null);
  const [form, setForm] = useState<{ display_name: string; locale: string; timezone: string }>({
    display_name: "",
    locale: "en-US",
    timezone: "UTC",
  });
  const [prefs, setPrefs] = useState({ default_range: "30D", default_region: "auto", table_page_size: 25 });
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [snippetTitle, setSnippetTitle] = useState("");
  const [snippetSql, setSnippetSql] = useState("");
  const [copied, setCopied] = useState(false);

  async function load() {
    const d = await bffFetch<UserData>("/profile");
    setData(d);
    setForm({ display_name: d.profile.display_name ?? "", locale: d.profile.locale, timezone: d.profile.timezone });
    setPrefs({
      default_range: d.profile.prefs.default_range,
      default_region: d.profile.prefs.default_region,
      table_page_size: d.profile.prefs.table_page_size,
    });
  }

  useEffect(() => {
    load().catch(() => undefined);
  }, []);

  async function saveProfile() {
    setSaving(true);
    setSaved(false);
    await bffFetch("/profile", {
      method: "POST",
      body: {
        display_name: form.display_name || null,
        locale: form.locale,
        timezone: form.timezone,
        prefs: { ...prefs, table_page_size: Number(prefs.table_page_size) || 25 },
      },
    });
    setSaved(true);
    setSaving(false);
  }

  async function addSnippet() {
    if (!snippetTitle.trim() || !snippetSql.trim()) return;
    await bffFetch("/snippets", { method: "POST", body: { title: snippetTitle, sql: snippetSql } });
    setSnippetTitle("");
    setSnippetSql("");
    await load();
  }

  async function removeSnippet(id: string) {
    await bffFetch(`/snippets/${id}`, { method: "DELETE" });
    await load();
  }

  return (
    <Shell>
      <div>
        <h2 className="text-2xl md:text-3xl font-semibold text-on-surface">{t.account.title}</h2>
        <p className="text-base text-on-surface-variant mt-1">{t.account.subtitle}</p>
      </div>

      <Tabs defaultValue="profile" className="mt-8">
        <TabsList className="bg-surface-container-low border border-outline-variant gap-1 flex-wrap">
          <TabsTrigger value="profile" className="font-mono-label">{t.account.tabs.profile}</TabsTrigger>
          <TabsTrigger value="preferences" className="font-mono-label">{t.account.tabs.prefs}</TabsTrigger>
          <TabsTrigger value="onboarding" className="font-mono-label">{t.account.tabs.onboarding}</TabsTrigger>
          <TabsTrigger value="snippets" className="font-mono-label">{t.account.tabs.snippets}</TabsTrigger>
          <TabsTrigger value="activity" className="font-mono-label">{t.account.tabs.activity}</TabsTrigger>
        </TabsList>

        <TabsContent value="profile" className="mt-4">
          <Card className="bg-surface border-outline-variant" data-testid="profile-card">
            <CardHeader className="pb-2">
              <CardTitle className="font-mono-label text-on-surface text-sm flex items-center gap-2">
                <UserRound className="h-4 w-4 text-tertiary" /> {t.account.profile.identity}
              </CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="py-2 flex items-center justify-between border-b border-outline-variant/60">
                <span className="font-mono-label text-on-surface-variant">{t.account.profile.username}</span>
                <span className="font-mono-code text-on-surface flex items-center gap-2">
                  {data?.profile.username ?? "—"}
                  <button
                    aria-label="Copy username"
                    className="text-on-surface-variant hover:text-tertiary"
                    onClick={() => {
                      navigator.clipboard?.writeText(data?.profile.username ?? "");
                      setCopied(true);
                      setTimeout(() => setCopied(false), 1200);
                    }}
                  >
                    {copied ? <span className="font-mono-label text-tertiary text-xs">{t.connect.copied}</span> : <Copy className="h-4 w-4" />}
                  </button>
                </span>
              </div>
              <div className="grid gap-2">
                <Label htmlFor="display-name" className="font-mono-label text-on-surface-variant">{t.account.profile.displayName}</Label>
                <Input id="display-name" data-testid="profile-display-name" value={form.display_name} onChange={(e) => setForm({ ...form, display_name: e.target.value })} className="bg-surface-container-lowest border-outline-variant" />
              </div>
              <div className="grid md:grid-cols-2 gap-4">
                <div className="grid gap-2">
                  <Label htmlFor="locale" className="font-mono-label text-on-surface-variant">{t.account.profile.locale}</Label>
                  <Input id="locale" data-testid="profile-locale" value={form.locale} onChange={(e) => setForm({ ...form, locale: e.target.value })} className="bg-surface-container-lowest border-outline-variant font-mono-code" />
                </div>
                <div className="grid gap-2">
                  <Label htmlFor="timezone" className="font-mono-label text-on-surface-variant">{t.account.profile.timezone}</Label>
                  <Input id="timezone" data-testid="profile-timezone" value={form.timezone} onChange={(e) => setForm({ ...form, timezone: e.target.value })} className="bg-surface-container-lowest border-outline-variant font-mono-code" />
                </div>
              </div>
              <div className="flex items-center gap-3">
                <Button onClick={saveProfile} disabled={saving} data-testid="profile-save" className="bg-tertiary text-primary-container font-mono-label font-bold hover:bg-tertiary-fixed">
                  {saving && <Loader2 className="h-4 w-4 animate-spin mr-1" />}
                  {t.account.profile.save}
                </Button>
                {saved && <span className="font-mono-label text-tertiary text-xs" data-testid="profile-saved">{t.account.profile.saved}.</span>}
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="preferences" className="mt-4">
          <Card className="bg-surface border-outline-variant" data-testid="prefs-card">
            <CardHeader className="pb-2">
              <CardTitle className="font-mono-label text-on-surface text-sm">{t.account.prefs.consolePrefs}</CardTitle>
              <CardDescription className="font-mono-code text-on-surface-variant text-xs">{t.account.prefs.prefsHint}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="grid md:grid-cols-3 gap-4">
                <div className="grid gap-2">
                  <Label htmlFor="default-range" className="font-mono-label text-on-surface-variant">{t.account.prefs.defaultRange}</Label>
                  <select id="default-range" data-testid="prefs-range" value={prefs.default_range} onChange={(e) => setPrefs({ ...prefs, default_range: e.target.value })} className="h-9 rounded bg-surface-container-lowest border border-outline-variant px-3 text-sm text-on-surface">
                    {["7D", "30D", "90D"].map((r) => <option key={r}>{r}</option>)}
                  </select>
                </div>
                <div className="grid gap-2">
                  <Label htmlFor="default-region" className="font-mono-label text-on-surface-variant">{t.account.prefs.defaultRegion}</Label>
                  <select id="default-region" data-testid="prefs-region" value={prefs.default_region} onChange={(e) => setPrefs({ ...prefs, default_region: e.target.value })} className="h-9 rounded bg-surface-container-lowest border border-outline-variant px-3 text-sm text-on-surface">
                    {["auto", "US East", "US West", "EU Central", "Tokyo"].map((r) => <option key={r}>{r}</option>)}
                  </select>
                </div>
                <div className="grid gap-2">
                  <Label htmlFor="page-size" className="font-mono-label text-on-surface-variant">{t.account.prefs.pageSize}</Label>
                  <Input id="page-size" data-testid="prefs-page-size" type="number" value={prefs.table_page_size} onChange={(e) => setPrefs({ ...prefs, table_page_size: Number(e.target.value) })} className="bg-surface-container-lowest border-outline-variant font-mono-code" />
                </div>
              </div>
              <div className="flex items-center gap-3">
                <Button onClick={saveProfile} disabled={saving} data-testid="prefs-save" className="bg-tertiary text-primary-container font-mono-label font-bold hover:bg-tertiary-fixed">
                  {saving && <Loader2 className="h-4 w-4 animate-spin mr-1" />}
                  {t.account.prefs.save}
                </Button>
                {saved && <span className="font-mono-label text-tertiary text-xs">{t.account.prefs.saved}.</span>}
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="onboarding" className="mt-4">
          <Card className="bg-surface border-outline-variant" data-testid="onboarding-card">
            <CardHeader className="pb-2">
              <CardTitle className="font-mono-label text-on-surface text-sm flex items-center gap-2">
                <ShieldCheck className="h-4 w-4 text-tertiary" /> {t.account.tabs.onboarding}
              </CardTitle>
            </CardHeader>
            <CardContent>
              {data ? (
                <div className="flex flex-col gap-3">
                  {[
                    { key: "first_cell_created", label: t.account.onboarding.createdCell },
                    { key: "api_key_created", label: t.account.onboarding.createdKey },
                    { key: "first_request_made", label: t.account.onboarding.firstRequest },
                  ].map((s) => {
                    const done = data.onboarding[s.key as keyof Onboarding];
                    return (
                      <div key={s.key} className="flex items-center gap-3" data-testid="onboarding-step">
                        <span className={`w-6 h-6 rounded-full border flex items-center justify-center font-mono-label text-xs ${done ? "border-tertiary bg-tertiary/15 text-tertiary" : "border-outline-variant text-on-surface-variant"}`}>
                          {done ? "✓" : "•"}
                        </span>
                        <span className={`font-mono-code text-sm ${done ? "text-on-surface" : "text-on-surface-variant"}`}>{s.label}</span>
                      </div>
                    );
                  })}
                  <div className="mt-2 font-mono-label text-xs text-on-surface-variant">
                    {data.onboarding.completed ? t.account.onboarding.completeMsg : t.account.onboarding.pendingMsg}
                  </div>
                </div>
              ) : (
                <div className="font-mono-label text-on-surface-variant">{t.common.loading}</div>
              )}
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="snippets" className="mt-4">
          <div className="grid gap-4">
            <Card className="bg-surface border-outline-variant" data-testid="snippets-card">
              <CardHeader className="pb-2">
                <CardTitle className="font-mono-label text-on-surface text-sm">{t.account.snippets.title}</CardTitle>
              </CardHeader>
              <CardContent className="flex flex-col gap-3">
                <div className="grid md:grid-cols-3 gap-2">
                  <Input data-testid="snippet-title" placeholder={t.account.snippets.savedTitle} value={snippetTitle} onChange={(e) => setSnippetTitle(e.target.value)} className="bg-surface-container-lowest border-outline-variant font-mono-code" />
                  <Input data-testid="snippet-sql" placeholder="SELECT ..." value={snippetSql} onChange={(e) => setSnippetSql(e.target.value)} className="md:col-span-1 bg-surface-container-lowest border-outline-variant font-mono-code" />
                  <Button onClick={addSnippet} disabled={!snippetTitle.trim() || !snippetSql.trim()} data-testid="snippet-add" className="bg-tertiary text-primary-container font-mono-label font-bold hover:bg-tertiary-fixed">{t.account.snippets.save}</Button>
                </div>
                <div className="flex flex-col gap-1.5">
                  {(data?.snippets ?? []).map((s) => (
                    <div key={s.id} className="flex items-center justify-between gap-3 p-2 rounded border border-outline-variant/60 hover:border-outline transition-colors" data-testid="snippet-row">
                      <div className="min-w-0">
                        <div className="font-mono-code text-on-surface text-sm truncate">{s.title}</div>
                        <div className="font-mono-code text-on-surface-variant text-xs truncate">{s.sql}</div>
                      </div>
                      <button aria-label="Delete snippet" className="text-on-surface-variant hover:text-error shrink-0" onClick={() => removeSnippet(s.id)}>
                        <Trash2 className="h-4 w-4" />
                      </button>
                    </div>
                  ))}
                  {(data?.snippets ?? []).length === 0 && <div className="font-mono-label text-on-surface-variant text-xs">{t.account.snippets.emptyHint}</div>}
                </div>
              </CardContent>
            </Card>
          </div>
        </TabsContent>

        <TabsContent value="activity" className="mt-4">
          <div className="grid md:grid-cols-2 gap-4">
            <Card className="bg-surface border-outline-variant" data-testid="recent-card">
              <CardHeader className="pb-2">
                <CardTitle className="font-mono-label text-on-surface text-sm">{t.account.activity.recentCells}</CardTitle>
              </CardHeader>
              <CardContent className="flex flex-col gap-1.5">
                {(data?.recent ?? []).map((r) => (
                  <div key={r.cell_id} className="flex items-center justify-between p-2 rounded border border-outline-variant/60 font-mono-code text-xs" data-testid="recent-row">
                    <span className="text-on-surface truncate">{r.cell_id.slice(0, 16)}…</span>
                    <span className="text-on-surface-variant shrink-0">{formatTime(r.last_visited)}</span>
                  </div>
                ))}
                {(data?.recent ?? []).length === 0 && <div className="font-mono-label text-on-surface-variant text-xs">{t.account.activity.noRecent}</div>}
              </CardContent>
            </Card>
            <Card className="bg-surface border-outline-variant" data-testid="history-card">
              <CardHeader className="pb-2">
                <CardTitle className="font-mono-label text-on-surface text-sm flex items-center gap-2">
                  <History className="h-4 w-4 text-tertiary" /> {t.account.activity.history}
                </CardTitle>
                <CardDescription className="font-mono-code text-on-surface-variant text-xs">{t.account.activity.historyHint}</CardDescription>
              </CardHeader>
              <CardContent className="flex flex-col gap-1.5">
                {(data?.history ?? []).map((h, i) => (
                  <div key={i} className="p-2 rounded border border-outline-variant/60 flex items-center justify-between gap-2" data-testid="history-row">
                    <span className="font-mono-code text-on-surface-variant text-xs truncate">{h.sql}</span>
                    <span className="font-mono-code text-on-surface-variant/60 text-[10px] shrink-0">{formatTime(h.created_at)}</span>
                  </div>
                ))}
                {(data?.history ?? []).length === 0 && <div className="font-mono-label text-on-surface-variant text-xs">{t.account.activity.noQueries}</div>}
              </CardContent>
            </Card>
          </div>
        </TabsContent>
      </Tabs>
    </Shell>
  );
}
