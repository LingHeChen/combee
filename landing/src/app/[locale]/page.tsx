import { Navbar, Hero, Screens, Features } from "@/components/sections";
import { Benchmarks, CodeShowcase, AlphaCta, Footer } from "@/components/sections2";
import { getDict, type Locale } from "@/lib/i18n";

export default async function LocalePage({
  params,
}: {
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await params;
  const t = getDict((locale as Locale) ?? "en");
  return (
    <main data-testid="landing-page">
      <Navbar t={t} locale={locale as Locale} />
      <Hero t={t} />
      <Screens t={t} />
      <Features t={t} />
      <Benchmarks t={t} />
      <CodeShowcase t={t} />
      <AlphaCta t={t} />
      <Footer t={t} />
    </main>
  );
}
