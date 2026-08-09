"use client";

import { useEffect, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import { LogOut } from "lucide-react";
import { useT } from "@/lib/i18n-context";

/**
 * 右上角用户菜单:头像显示当前用户名首字母(动态,非硬编码),
 * 点击展开菜单,提供"退出登录"。
 */
export function UserMenu() {
  const { locale } = useParams<{ locale: string }>();
  const t = useT();
  const router = useRouter();
  const [username, setUsername] = useState<string | null>(null);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    fetch("/api/bff/profile")
      .then((r) => (r.ok ? r.json() : null))
      .then((d) => setUsername(d?.profile?.username ?? null))
      .catch(() => {});
  }, []);

  const initial = username ? username[0].toUpperCase() : "?";

  async function logout() {
    try {
      await fetch("/api/bff/auth/logout", { method: "POST" });
    } catch {
      /* ignore */
    }
    const loc = locale === "en" ? "en" : "zh";
    router.push(`/${loc}/login`);
    router.refresh();
  }

  return (
    <div className="relative">
      <button
        onClick={() => setOpen((v) => !v)}
        data-testid="avatar"
        aria-label={username ?? "user"}
        className="w-8 h-8 rounded-full bg-surface-container-high border border-outline-variant flex items-center justify-center text-mono-label text-on-surface-variant hover:border-tertiary transition-colors"
      >
        {initial}
      </button>
      {open && (
        <div className="absolute right-0 mt-2 w-44 rounded-xl bg-surface-container-high border border-outline-variant shadow-lg p-1 z-20">
          <div className="px-3 py-2 text-xs text-on-surface-variant truncate">
            {username ?? "…"}
          </div>
          <button
            onClick={logout}
            className="w-full flex items-center gap-2 px-3 py-2 text-sm text-on-surface hover:bg-tertiary/10 rounded-lg transition-colors"
          >
            <LogOut className="h-4 w-4" />
            {t.shell.logOut}
          </button>
        </div>
      )}
    </div>
  );
}
