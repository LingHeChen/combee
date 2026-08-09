"use client";

import { createContext, useContext } from "react";
import { type Dict, type Locale, getDict } from "./i18n";

const Ctx = createContext<Dict | null>(null);

export function I18nProvider({ locale, children }: { locale: Locale; children: React.ReactNode }) {
  return <Ctx.Provider value={getDict(locale)}>{children}</Ctx.Provider>;
}

/** client 组件取当前语言字典 */
export function useT(): Dict {
  const t = useContext(Ctx);
  if (!t) throw new Error("I18nProvider is missing — wrap the layout in <I18nProvider locale=…>");
  return t;
}
