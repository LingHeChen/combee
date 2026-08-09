import type { Metadata } from "next";
import "../globals.css";
import { I18nProvider } from "@/lib/i18n-context";
import { getDict, type Locale } from "@/lib/i18n";

export async function generateMetadata({
  params,
}: {
  params: Promise<{ locale: string }>;
}): Promise<Metadata> {
  const { locale } = await params;
  const t = getDict((locale as Locale) ?? "zh");
  return {
    title: t.meta.title,
    description: t.meta.description,
  };
}

export default async function LocaleLayout({
  children,
  params,
}: Readonly<{
  children: React.ReactNode;
  params: Promise<{ locale: string }>;
}>) {
  const { locale } = await params;
  const lang: Locale = locale === "en" ? "en" : "zh";
  return (
    // suppressHydrationWarning:Next 框架在 hydration 时可能以默认 lang 重建 <html>,
    // 而 SSR 已输出正确 lang —— React 官方推荐对语言/地区属性使用该标记,导航后属性仍正常更新。
    <html lang={lang === "zh" ? "zh-CN" : "en"} className="dark" suppressHydrationWarning>
      <body className="bg-background text-foreground antialiased">
        <I18nProvider locale={lang}>{children}</I18nProvider>
      </body>
    </html>
  );
}
