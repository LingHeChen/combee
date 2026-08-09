import "server-only";
import { AsyncLocalStorage } from "node:async_hooks";

// BFF 请求上下文:request_id 贯穿 BFF → Combee API(经 combee-client header)。
export const bffContext = new AsyncLocalStorage<{ request_id: string }>();

export function currentRequestId(): string {
  return bffContext.getStore()?.request_id ?? "";
}

/** 输出结构化 JSON 日志行(service=combee-bff;字段与 Rust 侧对齐)。 */
export function bffLog(
  level: "DEBUG" | "INFO" | "WARN" | "ERROR",
  fields: Record<string, string | number | boolean>,
): void {
  const line = JSON.stringify({
    timestamp: new Date().toISOString(),
    level,
    service: "combee-bff",
    request_id: currentRequestId() || undefined,
    ...fields,
  });
  // 敏感字段防护:调用方不得传入 password/api_key/access_code/sql params/kv value
  for (const k of Object.keys(fields)) {
    if (/password|api_key|api-key|access_code|session|voucher|secret/i.test(k)) {
      return; // 丢弃整条,防止敏感泄漏
    }
  }
  // 直写 stdout(Next 的 console 会被其日志框架接管;docker 下最终到 stdout)
  process.stdout.write(line + "\n");
  // 可选文件回退:COMBEE_BFF_LOG_FILE=/path 用于本地/容器直接验证
  if (process.env.COMBEE_BFF_LOG_FILE) {
    try {
      require("node:fs").appendFileSync(process.env.COMBEE_BFF_LOG_FILE, line + "\n");
    } catch {
      /* 忽略 */
    }
  }
}
