import { NextFetchEvent, NextRequest, NextResponse } from 'next/server';
import { createI18nMiddleware } from 'fumadocs-core/i18n/middleware';
import { isMarkdownPreferred, rewritePath } from 'fumadocs-core/negotiation';
import { i18n } from '@/lib/i18n';
import { docsContentRoute, docsRoute } from '@/lib/shared';

// markdown 协商:先处理默认语言(/docs)的裸路径,再交给 i18n middleware 补上 locale 前缀。
const { rewrite: rewriteDocs } = rewritePath(
  `${docsRoute}{/*path}`,
  `${docsContentRoute}{/*path}/content.md`,
);
const { rewrite: rewriteSuffix } = rewritePath(
  `${docsRoute}{/*path}.md`,
  `${docsContentRoute}{/*path}/content.md`,
);

const i18nMiddleware = createI18nMiddleware(i18n);

export default function proxy(request: NextRequest, event: NextFetchEvent) {
  const pathname = request.nextUrl.pathname;

  // 只有文档路径(/docs,含 markdown 协商)与根路径参与 locale 处理;
  // 其余(/_next/* 静态资源、/api/*、/llms.txt、og 图片、已带 locale 的 /zh、/en 等)直接放行,
  // 否则会被 i18n middleware 错误地 rewrite 成 /en/... 导致 404。
  if (!pathname.startsWith(docsRoute) && pathname !== '/') {
    return NextResponse.next();
  }

  const result = rewriteSuffix(pathname);
  if (result) {
    return NextResponse.rewrite(new URL(result, request.nextUrl));
  }

  if (isMarkdownPreferred(request)) {
    const docs = rewriteDocs(pathname);

    if (docs) {
      return NextResponse.rewrite(new URL(docs, request.nextUrl), {
        // this URL has two representations, selected by `Accept`
        headers: { Vary: 'Accept' },
      });
    }
  }

  // /docs/* → /en/docs/*(default locale);/zh/docs/* 保持原样。
  return i18nMiddleware(request, event);
}
