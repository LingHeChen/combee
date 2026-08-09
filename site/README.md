# Combee Docs (site)

docs.combee.cloud 的文档站 —— Fumadocs(Next.js)应用,中英双文。

## 快速开始

```bash
pnpm install
pnpm dev        # http://localhost:3000
pnpm build      # 生产构建
```

## 改文档前必读

- `FACTS.md` —— 文档事实检查表(当前版本 / 认证 / 支持的 KV / 错误模型 / benchmark 条件)。
- `docs-requirements.md` —— 站点构建指令(sitemap / 原则 / frontmatter / 错误模型 / 状态标记)。

两者与 `artifacts/` 中的版本保持同步。

## 结构

```text
content/docs/
├── en/   # English(默认语言,URL 无前缀:/docs/...)
└── zh/   # 中文(URL 前缀:/zh/docs/...)

app/[lang]/            # 路由(含 i18n)
lib/source.ts          # 内容源 + frontmatter schema(since/status)
proxy.ts               # locale 路由 + markdown 协商
```

## 约定

- 每页 frontmatter:`title` / `description` / `since` / `status`(`stable` | `experimental` | `planned`)。
- 每个主要示例都有 TypeScript / Python 语言切换,不维护两份重复教程。
- 示例必须真实可跑 —— 只使用 `@combee/sdk` / `combee`(PyPI)已实现的方法。
