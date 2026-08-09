import Link from 'next/link';

const copy: Record<string, { tagline: string; lead: string; cta: string }> = {
  en: {
    tagline: 'Serverless data for AI apps',
    lead: 'A serverless data runtime with SQL and Redis-style KV. Create a Cell, then use the TypeScript or Python SDK.',
    cta: 'Get started',
  },
  zh: {
    tagline: '面向 AI 应用的 Serverless 数据层',
    lead: 'Serverless 数据运行时,提供 SQL 与 Redis-style KV。创建一个 Cell,然后用 TypeScript 或 Python SDK 使用它。',
    cta: '开始使用',
  },
};

export default async function HomePage({
  params,
}: {
  params: Promise<{ lang: string }>;
}) {
  const { lang } = await params;
  const t = copy[lang] ?? copy.en;

  return (
    <div className="flex flex-col justify-center items-center text-center flex-1 gap-4 px-6">
      <h1 className="text-3xl font-bold">Combee</h1>
      <p className="text-fd-muted-foreground">{t.tagline}</p>
      <p className="max-w-xl text-fd-muted-foreground">{t.lead}</p>
      <Link
        href="/docs/getting-started/quickstart"
        className="rounded-full bg-fd-primary text-fd-primary-foreground px-5 py-2 font-medium"
      >
        {t.cta}
      </Link>
    </div>
  );
}

export function generateStaticParams() {
  return [{ lang: 'en' }, { lang: 'zh' }];
}
