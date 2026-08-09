"use client";

import { useEffect, useMemo, useState } from "react";

const PRISM_LANGS: Record<string, () => Promise<unknown>> = {
  typescript: () => import("prismjs/components/prism-typescript"),
  sql: () => import("prismjs/components/prism-sql"),
  http: () => import("prismjs/components/prism-http"),
};

export default function CodeBlock({
  code,
  language,
  title,
}: {
  code: string;
  language: keyof typeof PRISM_LANGS;
  title?: string;
}) {
  const [ready, setReady] = useState(false);

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const Prism = (await import("prismjs")).default;
        const loader = PRISM_LANGS[language];
        if (loader) await loader();
        if (alive) setReady(true);
      } catch {
        if (alive) setReady(true); // 加载失败也渲染纯文本
      }
    })();
    return () => {
      alive = false;
    };
  }, [language]);

  const html = useMemo(() => {
    if (!ready) return null;
    try {
      const Prism = require("prismjs") as typeof import("prismjs");
      return Prism.highlight(code, Prism.languages[language] ?? Prism.languages.clike, language);
    } catch {
      return null;
    }
  }, [ready, code, language]);

  return (
    <div className="code-block overflow-x-auto" data-testid="code-block">
      {title && (
        <div className="flex items-center gap-2 border-b border-[#1f2937] px-4 py-2">
          <span className="hex-dot live" />
          <span className="mono-label">{title}</span>
        </div>
      )}
      <pre className="p-4">
        <code
          className={`language-${language}`}
          // eslint-disable-next-line react/no-danger
          dangerouslySetInnerHTML={html ? { __html: html } : undefined}
        >
          {html ? null : code}
        </code>
      </pre>
    </div>
  );
}
