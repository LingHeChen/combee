import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** 格式化字节数。 */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return `${(bytes / 1024 ** i).toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

/** 格式化 unix 秒为本地时间。 */
export function formatTime(unixSecs: number): string {
  return new Date(unixSecs * 1000).toLocaleString();
}

/** 截断长 id。 */
export function shortId(id: string, len = 8): string {
  if (id.length <= len) return id;
  return `${id.slice(0, len)}…`;
}
