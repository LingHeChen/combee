import { defineI18n } from 'fumadocs-core/i18n';

/**
 * 站点语言配置(en 为默认语言,根路径不带前缀;zh 走 /zh/...)。
 * 事实检查:`artifacts/FACTS.md` — 改文档前先读。
 */
export const i18n = defineI18n({
  defaultLanguage: 'en',
  languages: ['en', 'zh'],
  hideLocale: 'default-locale',
  fallbackLanguage: 'en',
  parser: 'dir',
});

export type Locale = (typeof i18n.languages)[number];
