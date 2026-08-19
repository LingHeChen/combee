/* Landing 外部链接配置:所有对外 URL 从环境变量读取,代码不硬编码域名。
 *
 * | env | 用途 | 默认 |
 * |---|---|---|
 * | `NEXT_PUBLIC_CONSOLE_URL` | Combee Cloud 控制台基地址(不含路径) | `https://console.combee.cloud` |
 * | `NEXT_PUBLIC_CONSOLE_REGISTER_PATH` | 控制台注册路径(相对 CONSOLE_URL) | `/zh/register` |
 * | `NEXT_PUBLIC_DOCS_URL` | 文档站地址(不含路径) | `https://docs.combee.cloud` |
 * | `NEXT_PUBLIC_ALPHA_EMAIL` | Public Beta 候补邮箱 | `alpha@combee.cloud` |
 */

export const CONSOLE_URL =
  process.env.NEXT_PUBLIC_CONSOLE_URL?.replace(/\/+$/, "") ?? "https://console.combee.cloud";

export const CONSOLE_REGISTER_PATH =
  process.env.NEXT_PUBLIC_CONSOLE_REGISTER_PATH ?? "/zh/register";

/** 控制台注册页完整地址(CTA 目标)。 */
export const CONSOLE_REGISTER_URL = `${CONSOLE_URL}${CONSOLE_REGISTER_PATH.startsWith("/") ? "" : "/"}${CONSOLE_REGISTER_PATH}`;

export const ALPHA_EMAIL = process.env.NEXT_PUBLIC_ALPHA_EMAIL ?? "alpha@combee.cloud";

export const ALPHA_MAILTO = `mailto:${ALPHA_EMAIL}`;

/** 文档站基地址(footer 链接)。 */
export const DOCS_URL =
  process.env.NEXT_PUBLIC_DOCS_URL?.replace(/\/+$/, "") ?? "https://docs.combee.cloud";

/** 工信部 ICP 备案查询页(备案号链接目标)。 */
export const BEIAN_URL = "https://beian.miit.gov.cn/";

/** Combee API 基地址(Public Beta 候补登记用)。 */
export const COMBEE_API_URL =
  process.env.NEXT_PUBLIC_COMBEE_API_URL?.replace(/\/+$/, "") ?? "https://api.combee.cloud";

/** Public Beta 候补登记端点。 */
export const WAITLIST_URL = `${COMBEE_API_URL}/v1/waitlist`;
