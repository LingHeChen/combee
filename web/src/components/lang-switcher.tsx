"use client";

import { usePathname, useParams } from "next/navigation";
import { COOKIE, type Locale } from "@/lib/i18n";

/** 语言切换器:写 combee-locale cookie 并跳到同路径的对应语言。 */
export function LangSwitcher({ compact = false }: { compact?: boolean }) {
  const pathname = usePathname();
  const { locale } = useParams<{ locale: string }>();
  const current: Locale = locale === "en" ? "en" : "zh";

  const go = (l: Locale) => {
    document.cookie = `${COOKIE}=${l};path=/;max-age=31536000;SameSite=Lax`;
    const rest = pathname.replace(/^\/(zh|en)/, "") || "/";
    window.location.href = `/${l}${rest}`;
  };

  return (
    <div className="flex items-center gap-0.5 rounded border border-outline-variant px-0.5 py-0.5 text-xs">
      {(["zh", "en"] as const).map((l) => (
        <button
          key={l}
          onClick={() => go(l)}
          className={`rounded px-2 py-1 font-mono-label transition-colors ${
            current === l
              ? "bg-tertiary text-primary-container"
              : "text-on-surface-variant hover:text-on-surface"
          }`}
          data-testid={`lang-${l}`}
        >
          {l === "zh" ? "中文" : "EN"}
        </button>
      ))}
    </div>
  );
}
