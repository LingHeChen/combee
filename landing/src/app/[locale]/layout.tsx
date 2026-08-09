import type { Metadata } from "next";
import "../globals.css";
import { locales, type Locale, getDict } from "@/lib/i18n";

export function generateStaticParams() {
  return locales.map((locale) => ({ locale }));
}

export async function generateMetadata({
  params,
}: {
  params: Promise<{ locale: string }>;
}): Promise<Metadata> {
  const { locale } = await params;
  const t = getDict((locale as Locale) ?? "en");
  return {
    title: t.meta.title,
    description: t.meta.description,
    icons: { icon: "/combee-96.png" },
    metadataBase: new URL("https://combee.cloud"),
    openGraph: {
      title: t.meta.title,
      description: t.meta.description,
      type: "website",
    },
  };
}

export default async function LocaleLayout({
  children,
  params,
}: {
  children: React.ReactNode;
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await params;
  const lang = locale === "zh" ? "zh-CN" : "en";
  return (
    <html lang={lang} className="dark" suppressHydrationWarning>
      <head>
      </head>
      <body className="antialiased">{children}</body>
    </html>
  );
}
