import { getPageImageUrl, getPageMarkdownUrl, source } from '@/lib/source';
import {
  DocsBody,
  DocsDescription,
  DocsPage,
  DocsTitle,
  MarkdownCopyButton,
  ViewOptionsPopover,
} from 'fumadocs-ui/layouts/docs/page';
import { notFound } from 'next/navigation';
import { getMDXComponents } from '@/components/mdx';
import type { Metadata } from 'next';
import { createRelativeLink } from 'fumadocs-ui/mdx';
import { gitConfig } from '@/lib/shared';

const statusCopy: Record<
  string,
  { label: (locale: string) => string; className: string }
> = {
  experimental: {
    label: (locale) => (locale === 'zh' ? '实验性' : 'Experimental'),
    className: 'bg-fd-warning text-fd-warning-foreground',
  },
  planned: {
    label: (locale) => (locale === 'zh' ? '规划中' : 'Planned'),
    className: 'bg-fd-secondary text-fd-secondary-foreground',
  },
};

function StatusBadge({
  status,
  since,
  lang,
}: {
  status: string;
  since?: string;
  lang: string;
}) {
  if (status === 'stable' && !since) return null;

  return (
    <div className="flex flex-row flex-wrap gap-2 items-center mb-4">
      {status !== 'stable' && (
        <span
          className={`rounded-full px-2.5 py-0.5 text-xs font-medium ${statusCopy[status]?.className ?? ''}`}
        >
          {statusCopy[status]?.label(lang)}
        </span>
      )}
      {since && (
        <span className="rounded-full bg-fd-secondary text-fd-secondary-foreground px-2.5 py-0.5 text-xs font-medium">
          since {since}
        </span>
      )}
    </div>
  );
}

export default async function Page({
  params,
}: {
  params: Promise<{ lang: string; slug?: string[] }>;
}) {
  const { lang, slug } = await params;
  const page = source.getPage(slug, lang);
  if (!page) notFound();

  const MDX = page.data.body;
  const markdownUrl = getPageMarkdownUrl(page).url;

  return (
    <DocsPage toc={page.data.toc} full={page.data.full}>
      <DocsTitle>{page.data.title}</DocsTitle>
      <DocsDescription className="mb-0">{page.data.description}</DocsDescription>
      <StatusBadge status={page.data.status} since={page.data.since} lang={lang} />
      <div className="flex flex-row gap-2 items-center border-b pb-6">
        <MarkdownCopyButton markdownUrl={markdownUrl} />
        <ViewOptionsPopover
          markdownUrl={markdownUrl}
          githubUrl={`https://github.com/${gitConfig.user}/${gitConfig.repo}/blob/${gitConfig.branch}/content/docs/${page.path}`}
        />
      </div>
      <DocsBody>
        <MDX
          components={getMDXComponents({
            a: createRelativeLink(source, page),
          })}
        />
      </DocsBody>
    </DocsPage>
  );
}

export async function generateStaticParams() {
  return source.generateParams();
}

export async function generateMetadata({
  params,
}: {
  params: Promise<{ lang: string; slug?: string[] }>;
}): Promise<Metadata> {
  const { lang, slug } = await params;
  const page = source.getPage(slug, lang);
  if (!page) notFound();

  return {
    title: page.data.title,
    description: page.data.description,
    openGraph: {
      images: getPageImageUrl(page).url,
    },
  };
}
