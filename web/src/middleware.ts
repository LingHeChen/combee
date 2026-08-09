import { NextResponse } from "next/server";
import type { NextRequest } from "next/server";

const LOCALES = ["zh", "en"] as const;
type Locale = (typeof LOCALES)[number];
const DEFAULT_LOCALE: Locale = "zh";

/** 语言重定向:默认中文;cookie combee-locale=en 时英文。
 * BFF API / 静态资源放行;无语言前缀的页面路径 307 → /{locale}/... */
export function middleware(req: NextRequest) {
  const { pathname } = req.nextUrl;

  if (
    pathname.startsWith("/api/") ||
    pathname.startsWith("/_next/") ||
    pathname === "/favicon.ico" ||
    pathname.startsWith("/combee-192.png") ||
    pathname.includes(".")
  ) {
    return NextResponse.next();
  }

  const cookie = req.cookies.get("combee-locale")?.value;
  const locale: Locale = cookie === "en" ? "en" : DEFAULT_LOCALE;

  const first = pathname.split("/")[1];
  if (first === "zh" || first === "en") {
    return NextResponse.next();
  }

  const url = req.nextUrl.clone();
  url.pathname = `/${locale}${pathname === "/" ? "/" : pathname}`;
  return NextResponse.redirect(url);
}

export const config = {
  matcher: ["/((?!_next/static|_next/image|favicon.ico|combee-192.png).*)"],
};
