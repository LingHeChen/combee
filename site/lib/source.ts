import { loader } from 'fumadocs-core/source';
import { lucideIconsPlugin } from 'fumadocs-core/source/lucide-icons';
import { docsContentRoute, docsImageRoute, docsRoute } from './shared';
import { i18n } from './i18n';
import { defineDocs } from 'fumadocs-mdx/macro';
import { metaSchema, pageSchema } from 'fumadocs-core/source/schema';
import { z } from 'zod';

/**
 * 页面 frontmatter 规范(docs-requirements.md §7):
 * title / description / since / status(stable | experimental | planned)。
 */
const docs = defineDocs({
  dir: 'content/docs',
  docs: {
    schema: pageSchema.extend({
      since: z.string().optional(),
      status: z.enum(['stable', 'experimental', 'planned']).default('stable'),
    }),
    postprocess: {
      includeProcessedMarkdown: true,
    },
  },
  meta: {
    schema: metaSchema,
  },
});

export const source = loader({
  baseUrl: docsRoute,
  source: docs.toFumadocsSource(),
  i18n,
  plugins: [lucideIconsPlugin()],
});

export type Page = (typeof source)['$inferPage'];

export function getPageImageUrl(page: Page) {
  const segments = [...page.slugs, 'image.png'];

  return {
    segments,
    url:
      '/' +
      [page.locale, ...docsImageRoute.split('/'), ...segments].filter(Boolean).join('/'),
  };
}

export function getPageMarkdownUrl(page: Page) {
  const segments = [...page.slugs, 'content.md'];

  return {
    segments,
    url:
      '/' +
      [page.locale, ...docsContentRoute.split('/'), ...segments].filter(Boolean).join('/'),
  };
}

export async function getLLMText(page: Page) {
  const processed = await page.data.getText('processed');

  return `# ${page.data.title} (${page.url})

${processed}`;
}
