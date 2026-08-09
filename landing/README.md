# Combee Landing(单页营销站)

基于 `design/stitch_combee_landing_page_design`(High-Density Engineering Core 设计规范)开发的
单页 marketing landing page。**设计语言 1:1 采用**(非"致敬"):

- 深炭黑 `#0A0A0A` 画布 / 暖琥珀 `#F59E0B` 信号色 / Soft Cream `#FAFAF9` 正文 / Slate `#1F2937` 结构
- Geist(标题紧凑字距)+ JetBrains Mono(数据点/标签大写/代码)
- 4px 圆角标准元素、8px 大容器、1px slate 边框、琥珀 10% glow 激活态、玻璃导航 `backdrop-blur(12px)`
- 六边形状态灯(live 琥珀脉冲 / idle slate)

## 国际化(中英双语)

- 路由:`/en/`(English)与 `/zh/`(中文),`trailingSlash` 静态导出为 `en/index.html` / `zh/index.html`;
- 根路径 `/` 客户端重定向:优先 `localStorage.combee-locale`,否则按 `navigator.language`(默认中文);
- 导航栏语言切换器(EN / 中文)切换双语路由;
- 全部文案集中在 `src/lib/i18n.ts`(`Dict` 接口保证 en/zh 结构一致,单测校验无漏译);
- 中文页面自动关闭 mono-label 的 uppercase、缩小字距(`html[lang="zh-CN"]`);
- SEO:每个 locale 独立 `html lang` 与 metadata(title/description)。

## 页面结构(单页,锚点导航)

| Section | 内容 |
|---|---|
| Hero | "One app, one Cell. SQL + KV included." + 4 项真实 benchmark 数字 + CTA |
| The Console | 4 张设计稿截图(overview / cells / sql / usage,取自 `design/...` 的 `screen.png`) |
| Features | 6 卡:SQL+KV 同引擎 / 一次调用创建 / TTL 与计数器 / 对象存储备份 / 副本+自动 failover / usage metering |
| Benchmarks | 6 项大数字 + 容量表(10k / 100k / 1M),全部来自 `docs/PROJECT_STATUS.md` 实测值 |
| Code | TS SDK + REST 代码示例(prismjs 高亮,Amber/Slate/Cream 受限调色板) |
| Alpha | 定价两档:Private Alpha(invite,1,000 credits)+ Public Beta(soon) |
| Footer | 品牌行 |

## 数据来源(不虚构)

benchmark 数字与 `docs/PROJECT_STATUS.md` 一致:
KV hot GET p50/p99 ≈ 10µs/35µs;fast SET p99 ≈ 63µs;strict SET p99 ≈ 125µs;
SQL p99 ≈ 41µs;20k Cell 创建 ≈ 15ms;4+8 容器 1M×5k active p99 ≈ 64µs、命中率 100%。

## 开发

```bash
npm install
npm run dev        # http://localhost:3200
npm run build      # 静态导出到 out/(output: 'export')
```

## 测试

```bash
npm test           # vitest:展示数据完整性 7 项 + CodeBlock 高亮 3 项
npm run e2e        # playwright chromium headless:18 项断言 + 截图 /tmp/landing-shots/
npm run typecheck  # tsc 严格模式
```

E2E 覆盖(23 项):根路径重定向、/en 英文全断言、语言切换 en→zh→en、
localStorage 偏好、Hero 文案/CTA、4 截图、6 特性卡、bench 大数字+表、
代码高亮 token、两档定价、锚点跳转、zh 移动端 375px 无横向溢出、零 console 错误。

## 部署

静态导出(纯 HTML/CSS/JS),任意静态托管/CDN 直接发布 `out/`。
