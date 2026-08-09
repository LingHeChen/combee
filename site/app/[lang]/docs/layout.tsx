import { source } from '@/lib/source';
import { DocsLayout } from 'fumadocs-ui/layouts/docs';
import { baseOptions } from '@/lib/layout.shared';
import { LanguageSelect } from 'fumadocs-ui/layouts/shared/slots/language-select';

export default async function Layout({
  children,
  params,
}: {
  children: React.ReactNode;
  params: Promise<{ lang: string }>;
}) {
  const { lang } = await params;

  return (
    <DocsLayout
      tree={source.getPageTree(lang)}
      {...baseOptions()}
      nav={{
        ...baseOptions().nav,
        // key:Sidebar 会把 nav.children 放进 children 数组渲染,缺 key 会触发 React 警告。
        children: <LanguageSelect key="language-select" />,
      }}
    >
      {children}
    </DocsLayout>
  );
}

export function generateStaticParams() {
  return [{ lang: 'en' }, { lang: 'zh' }];
}
