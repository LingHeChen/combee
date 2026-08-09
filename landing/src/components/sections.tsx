import Image from "next/image";
import Link from "next/link";
import { SCREEN_SRCS } from "@/lib/data";
import { type Dict, type Locale } from "@/lib/i18n";
import { LangSwitcher } from "./lang-switcher";

/* ============ 导航(玻璃) ============ */
export function Navbar({ t, locale }: { t: Dict; locale: Locale }) {
  return (
    <header className="glass-nav fixed inset-x-0 top-0 z-50">
      <nav className="mx-auto flex h-16 max-w-[1280px] items-center justify-between px-4 md:px-8">
        <Link href={`/${locale}`} className="flex items-center gap-2.5">
          <HexLogo />
          <span className="text-[17px] font-semibold tracking-tight text-[#fafaf9]">
            Combee
          </span>
        </Link>
        <div className="hidden items-center gap-7 md:flex">
          <a href="#product" className="text-sm text-[#c4c7c7] transition-colors hover:text-[#fafaf9]">{t.nav.product}</a>
          <a href="#benchmarks" className="text-sm text-[#c4c7c7] transition-colors hover:text-[#fafaf9]">{t.nav.benchmarks}</a>
          <a href="#code" className="text-sm text-[#c4c7c7] transition-colors hover:text-[#fafaf9]">{t.nav.code}</a>
          <a href="#alpha" className="text-sm text-[#c4c7c7] transition-colors hover:text-[#fafaf9]">{t.nav.alpha}</a>
        </div>
        <div className="flex items-center gap-3">
          <LangSwitcher locale={locale} />
          <a href="#alpha" className="btn-primary px-4 py-2 text-sm" data-testid="nav-cta">
            {t.nav.cta}
          </a>
        </div>
      </nav>
    </header>
  );
}

function HexLogo() {
  return (
    // eslint-disable-next-line @next/next/no-img-element
    <img
      src="/combee-96.png"
      alt="Combee"
      width={24}
      height={24}
      className="h-6 w-6 rounded-[4px] object-contain"
      data-testid="brand-logo"
    />
  );
}

/* ============ Hero ============ */
export function Hero({ t }: { t: Dict }) {
  return (
    <section id="top" className="relative overflow-hidden pt-32 pb-20 md:pt-40">
      <div className="grid-bg absolute inset-0" aria-hidden />
      <div className="relative mx-auto max-w-[1280px] px-4 md:px-8">
        <div className="mx-auto max-w-3xl text-center">
          <div className="mb-6 inline-flex items-center gap-2 rounded-[4px] border border-[#1f2937] bg-[#111111] px-3 py-1.5">
            <span className="hex-dot live" />
            <span className="mono-label">{t.hero.badge}</span>
          </div>
          <h1 className="text-[40px] font-bold leading-[1.05] tracking-[-0.04em] text-[#fafaf9] md:text-[64px]">
            {t.hero.titleA}
            <br />
            <span className="text-[#f59e0b]">{t.hero.titleB}</span> {t.hero.titleC}
          </h1>
          <p className="mx-auto mt-6 max-w-xl text-[17px] leading-7 text-[#c4c7c7]">
            {t.hero.subtitle}
          </p>
          <div className="mt-10 flex flex-col items-center justify-center gap-3 sm:flex-row">
            <a href="#alpha" className="btn-primary w-full px-7 py-3.5 text-center text-[15px] sm:w-auto" data-testid="hero-cta">
              {t.hero.ctaPrimary}
            </a>
            <a href="#benchmarks" className="btn-secondary w-full px-7 py-3.5 text-center text-[15px] sm:w-auto">
              {t.hero.ctaSecondary}
            </a>
          </div>
          <div className="mt-12 flex flex-wrap items-center justify-center gap-x-8 gap-y-3">
            {t.hero.stats.map(([n, label]) => (
              <div key={label} className="text-center">
                <div className="mono-data text-xl font-semibold text-[#fafaf9]">{n}</div>
                <div className="mono-label mt-0.5">{label}</div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

/* ============ 产品截图秀 ============ */
export function Screens({ t }: { t: Dict }) {
  return (
    <section id="product" className="scroll-mt-24 py-20">
      <div className="mx-auto max-w-[1280px] px-4 md:px-8">
        <div className="mb-12 flex flex-wrap items-end justify-between gap-4">
          <div>
            <div className="mono-label mb-2">{t.screens.label}</div>
            <h2 className="text-[32px] font-semibold tracking-[-0.02em] text-[#fafaf9]">
              {t.screens.title}
            </h2>
          </div>
          <p className="max-w-sm text-sm leading-6 text-[#c4c7c7]">{t.screens.body}</p>
        </div>
        <div className="grid gap-6 md:grid-cols-2" data-testid="screens">
          {SCREEN_SRCS.map((src, i) => (
            <figure key={src} className="screen-frame glow-active">
              <div className="flex items-center gap-1.5 border-b border-[#1f2937] bg-[#111111] px-4 py-2.5">
                <span className="h-2.5 w-2.5 rounded-full bg-[#374151]" />
                <span className="h-2.5 w-2.5 rounded-full bg-[#374151]" />
                <span className="h-2.5 w-2.5 rounded-full bg-[#374151]" />
                <span className="mono-label ml-3">combee.cloud</span>
              </div>
              <Image
                src={src}
                alt={t.screens.captions[i]}
                width={1600}
                height={1280}
                className="h-auto w-full"
                loading="lazy"
              />
              <figcaption className="mono-label border-t border-[#1f2937] bg-[#111111] px-4 py-2.5">
                {t.screens.captions[i]}
              </figcaption>
            </figure>
          ))}
        </div>
      </div>
    </section>
  );
}

/* ============ 特性 ============ */
export function Features({ t }: { t: Dict }) {
  return (
    <section className="border-t border-[#1f2937] py-20">
      <div className="mx-auto max-w-[1280px] px-4 md:px-8">
        <div className="mb-12 max-w-2xl">
          <div className="mono-label mb-2">{t.features.label}</div>
          <h2 className="text-[32px] font-semibold tracking-[-0.02em] text-[#fafaf9]">
            {t.features.title}
          </h2>
        </div>
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3" data-testid="features">
          {t.features.items.map((f) => (
            <div key={f.title} className="card glow-active p-6">
              <div className="mb-3 flex items-center justify-between">
                <span className="hex-dot live" />
                <span className="mono-label">{f.tag}</span>
              </div>
              <h3 className="mb-2 text-[17px] font-medium text-[#fafaf9]">{f.title}</h3>
              <p className="text-sm leading-6 text-[#c4c7c7]">{f.body}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
