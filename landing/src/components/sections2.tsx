import CodeBlock from "./code-block";
import { TS_CODE, HTTP_CODE } from "@/lib/data";
import { type Dict } from "@/lib/i18n";
import { CONSOLE_REGISTER_URL } from "@/lib/config";
import { WaitlistForm } from "./waitlist-form";

/* ============ Benchmark ============ */
export function Benchmarks({ t }: { t: Dict }) {
  return (
    <section id="benchmarks" className="scroll-mt-24 border-t border-[#1f2937] py-20">
      <div className="mx-auto max-w-[1280px] px-4 md:px-8">
        <div className="mb-12 flex flex-wrap items-end justify-between gap-4">
          <div>
            <div className="mono-label mb-2">{t.bench.label}</div>
            <h2 className="text-[32px] font-semibold tracking-[-0.02em] text-[#fafaf9]">
              {t.bench.title}
            </h2>
          </div>
          <p className="max-w-sm text-sm leading-6 text-[#c4c7c7]">{t.bench.body}</p>
        </div>

        <div className="mb-12 grid grid-cols-2 gap-4 lg:grid-cols-6" data-testid="bench-big">
          {t.bench.big.map((b) => (
            <div key={b.label} className="card p-5 text-center">
              <div className="mono-data text-2xl font-bold text-[#f59e0b]">{b.value}</div>
              <div className="mono-label mt-1.5">{b.label}</div>
            </div>
          ))}
        </div>

        <div className="card max-w-full overflow-x-auto">
          <table className="w-full min-w-[560px] text-left" data-testid="bench-table">
            <thead>
              <tr className="border-b border-[#1f2937]">
                {t.bench.tableHeaders.map((h) => (
                  <th key={h} className="mono-label px-5 py-3">{h}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {t.bench.rows.map((r) => (
                <tr key={r.cells} className="border-b border-[#1f2937]/60 last:border-0">
                  <td className="mono-data px-5 py-3.5 text-[15px] text-[#fafaf9]">{r.cells}</td>
                  <td className="mono-data px-5 py-3.5 text-sm text-[#c4c7c7]">{r.active}</td>
                  <td className="px-5 py-3.5 text-sm text-[#c4c7c7]">{r.note}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </section>
  );
}

/* ============ 代码 ============ */
export function CodeShowcase({ t }: { t: Dict }) {
  return (
    <section id="code" className="scroll-mt-24 border-t border-[#1f2937] py-20">
      <div className="mx-auto max-w-[1280px] px-4 md:px-8">
        <div className="mb-12 max-w-2xl">
          <div className="mono-label mb-2">{t.code.label}</div>
          <h2 className="text-[32px] font-semibold tracking-[-0.02em] text-[#fafaf9]">
            {t.code.title}
          </h2>
        </div>
        <div className="grid gap-6 lg:grid-cols-2" data-testid="code-showcase">
          <div className="min-w-0">
            <CodeBlock code={TS_CODE} language="typescript" title={t.code.tsTitle} />
          </div>
          <div className="flex min-w-0 flex-col gap-6">
            <CodeBlock code={HTTP_CODE} language="http" title={t.code.httpTitle} />
            <div className="card p-5">
              <div className="mb-2 flex items-center gap-2">
                <span className="hex-dot live" />
                <span className="mono-label">{t.code.singleEngineTitle}</span>
              </div>
              <p className="text-sm leading-6 text-[#c4c7c7]">{t.code.singleEngineBody}</p>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}

/* ============ Alpha CTA + 定价 ============ */
export function AlphaCta({ t }: { t: Dict }) {
  return (
    <section id="alpha" className="scroll-mt-24 border-t border-[#1f2937] py-20">
      <div className="mx-auto max-w-[1280px] px-4 md:px-8">
        <div className="grid items-center gap-10 lg:grid-cols-[1fr_1.2fr]">
          <div>
            <div className="mono-label mb-2">{t.alpha.label}</div>
            <h2 className="text-[32px] font-semibold tracking-[-0.02em] text-[#fafaf9]">
              {t.alpha.title}
            </h2>
            <p className="mt-4 max-w-md text-[15px] leading-7 text-[#c4c7c7]">{t.alpha.body}</p>
            <div className="mt-8 flex flex-wrap gap-3">
              <a
                href={CONSOLE_REGISTER_URL}
                target="_blank"
                rel="noreferrer"
                className="btn-primary px-6 py-3 text-sm"
                data-testid="alpha-cta"
              >
                {t.alpha.cta}
              </a>
              <a href="#top" className="btn-secondary px-6 py-3 text-sm">
                {t.alpha.backToTop}
              </a>
            </div>
          </div>
          <div className="grid gap-4 sm:grid-cols-2" data-testid="tiers">
            {t.alpha.tiers.map((tier) => (
              <div
                key={tier.name}
                className={`card p-6 ${tier.highlight ? "border-[#f59e0b]/40" : ""}`}
              >
                <div className="mb-1 flex items-center justify-between">
                  <span className="mono-label">{tier.tag}</span>
                  <span className={`hex-dot ${tier.highlight ? "live" : "idle"}`} />
                </div>
                <h3 className="text-lg font-semibold text-[#fafaf9]">{tier.name}</h3>
                <div className="mono-data mt-1 text-sm text-[#f59e0b]">{tier.price}</div>
                <ul className="mt-4 space-y-2">
                  {tier.points.map((p) => (
                    <li key={p} className="flex gap-2 text-sm leading-5 text-[#c4c7c7]">
                      <span className="mt-1.5 h-1 w-1 shrink-0 rounded-full bg-[#f59e0b]" />
                      {p}
                    </li>
                  ))}
                </ul>
                {tier.highlight ? (
                  <a
                    href={CONSOLE_REGISTER_URL}
                    target="_blank"
                    rel="noreferrer"
                    className="mt-5 block px-4 py-2.5 text-center text-sm btn-primary"
                  >
                    {tier.cta}
                  </a>
                ) : (
                  <WaitlistForm t={t} />
                )}
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

/* ============ Footer ============ */
export function Footer({ t }: { t: Dict }) {
  return (
    <footer className="border-t border-[#1f2937] py-10">
      <div className="mx-auto flex max-w-[1280px] flex-col items-center justify-between gap-4 px-4 md:flex-row md:px-8">
        <div className="flex items-center gap-2">
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img src="/combee-96.png" alt="" width={16} height={16} className="h-4 w-4 rounded-[4px] object-contain" />
          <span className="text-sm font-medium text-[#c4c7c7]">Combee — {t.footer.tagline}</span>
        </div>
        <div className="mono-label">{t.footer.sub}</div>
      </div>
    </footer>
  );
}
