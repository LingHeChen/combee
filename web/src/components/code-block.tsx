"use client";

import { useMemo } from "react";
import Prism from "prismjs";
import "prismjs/components/prism-sql";
import "prismjs/components/prism-python";
import "prismjs/components/prism-bash";
import "prismjs/components/prism-json";
import "prismjs/components/prism-javascript";
import "prismjs/components/prism-typescript";
import "prismjs/components/prism-jsx";
import "prismjs/components/prism-tsx";

/** 统一代码高亮组件(prismjs + 设计系统深色主题)。
 * 语言:sql / python / bash / http(curl 用 bash 高亮) / json / tsx。 */
export function CodeBlock({
  code,
  language = "sql",
  className = "",
}: {
  code: string;
  language?: string;
  className?: string;
}) {
  const html = useMemo(() => {
    const grammar = Prism.languages[language];
    if (!grammar) return escapeHtml(code);
    try {
      return Prism.highlight(code, grammar, language);
    } catch {
      return escapeHtml(code);
    }
  }, [code, language]);

  return (
    <pre
      className={`code-block overflow-auto ${className}`}
      // eslint-disable-next-line react/no-danger
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}
