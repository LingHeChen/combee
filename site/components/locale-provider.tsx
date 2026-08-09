'use client';

import { I18nProvider } from 'fumadocs-ui/contexts/i18n';
import { usePathname, useRouter } from 'next/navigation';

const locales = [
  { locale: 'en', name: 'English' },
  { locale: 'zh', name: '中文' },
];

/**
 * 语言切换:en 无路径前缀(default-locale),zh 前缀 /zh。
 * /docs/x ↔ /zh/docs/x;/(home) ↔ /zh。
 */
export function LocaleProvider({
  lang,
  children,
}: {
  lang: string;
  children: React.ReactNode;
}) {
  const router = useRouter();
  const pathname = usePathname();

  const onChange = (next: string) => {
    if (next === lang) return;

    // 去掉当前 locale 前缀
    let path = pathname;
    if (pathname.startsWith('/zh')) {
      path = pathname.slice(3) || '/';
    } else if (pathname.startsWith(`/${lang}`)) {
      path = pathname.slice(lang.length + 1) || '/';
    }

    const target = next === 'en' ? path : `/${next}${path === '/' ? '' : path}`;
    router.push(target);
  };

  return (
    <I18nProvider locale={lang} locales={locales} onLocaleChange={onChange}>
      {children}
    </I18nProvider>
  );
}
