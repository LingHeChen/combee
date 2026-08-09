"use client";

import { locales, type Locale } from "@/lib/i18n";

/** 语言切换:写 localStorage 偏好(与根路径 RootRedirect 一致)+ 跳同路径对应语言。 */
export function LangSwitcher({ locale }: { locale: Locale }) {
  return (
    <div className="flex items-center gap-1 rounded-[4px] border border-[#1f2937] px-1 py-0.5">
      {locales.map((l) => (
        <a
          key={l}
          href={`/${l}/`}
          onClick={() => {
            try {
              localStorage.setItem("combee-locale", l);
            } catch {
              /* 忽略 */
            }
          }}
          className={`px-2 py-1 text-xs transition-colors ${
            l === locale
              ? "bg-[#f59e0b] font-semibold text-[#0a0a0a]"
              : "text-[#c4c7c7] hover:text-[#fafaf9]"
          }`}
          aria-current={l === locale ? "true" : undefined}
          data-testid={`lang-${l}`}
        >
          {l === "en" ? "EN" : "中文"}
        </a>
      ))}
    </div>
  );
}
