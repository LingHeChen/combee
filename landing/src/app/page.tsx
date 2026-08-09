"use client";

import { useEffect } from "react";
import { defaultLocale, type Locale } from "@/lib/i18n";

/** 根路径:默认中文;仅当 localStorage.combee-locale 明确为 en 时跳英文。
 * (静态页无服务端检测,统一默认 zh,不跟随浏览器语言。) */
export default function RootRedirect() {
  useEffect(() => {
    let target: Locale = defaultLocale; // "zh"
    try {
      const saved = localStorage.getItem("combee-locale");
      if (saved === "en") {
        target = "en";
      }
    } catch {
      /* 忽略 */
    }
    window.location.replace(`/${target}/`);
  }, []);
  return null;
}
