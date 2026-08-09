import { RootProvider } from 'fumadocs-ui/provider/next';
import '../global.css';
import { LocaleProvider } from '@/components/locale-provider';

import type { Metadata } from 'next';

export const metadata: Metadata = {
  metadataBase: new URL('https://docs.combee.cloud'),
};

export default async function Layout({
  children,
  params,
}: {
  children: React.ReactNode;
  params: Promise<{ lang: string }>;
}) {
  const { lang } = await params;

  return (
    <html lang={lang} suppressHydrationWarning>
      <body className="flex flex-col min-h-screen">
        {/* RootProvider 必须先于 LocaleProvider:I18nProvider 内部使用 framework 的 useRouter/usePathname */}
        <RootProvider>
          <LocaleProvider lang={lang}>{children}</LocaleProvider>
        </RootProvider>
      </body>
    </html>
  );
}

export function generateStaticParams() {
  return [{ lang: 'en' }, { lang: 'zh' }];
}
