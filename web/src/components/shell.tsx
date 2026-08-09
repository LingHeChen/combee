"use client";

import Link from "next/link";
import Image from "next/image";
import { usePathname, useParams } from "next/navigation";
import {
  BarChart3,
  Boxes,
  LifeBuoy,
  KeyRound,
  LayoutDashboard,
  Settings,
  Wallet,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n-context";
import { type Locale } from "@/lib/i18n";
import { LangSwitcher } from "./lang-switcher";
import { UserMenu } from "./user-menu";

function useLoc(): Locale {
  const { locale } = useParams<{ locale: string }>();
  return locale === "en" ? "en" : "zh";
}

export function SideNav() {
  const pathname = usePathname();
  const locale = useLoc();
  const t = useT();

  const NAV = [
    { href: `/${locale}/overview`, label: t.shell.overview, icon: LayoutDashboard },
    { href: `/${locale}/cells`, label: t.shell.cells, icon: Boxes },
    { href: `/${locale}/api-keys`, label: t.shell.apiKeys, icon: KeyRound },
    { href: `/${locale}/usage`, label: t.shell.usage, icon: BarChart3 },
    { href: `/${locale}/credits`, label: t.shell.credits, icon: Wallet },
  ];

  return (
    <nav className="hidden md:flex flex-col h-screen w-64 bg-surface-container-low border-r border-outline-variant py-6 px-4 gap-2 sticky left-0 top-0 shrink-0">
      <div className="mb-8 px-2 flex items-center gap-3">
        <Image src="/combee-64.png" alt="Combee Cloud" width={32} height={32} className="h-8 w-8 object-contain" priority />
        <div>
          <h1 className="text-headline-md font-semibold text-on-surface leading-tight">{t.shell.product}</h1>
          <span className="font-mono-label text-on-surface-variant">v0.1.0-alpha</span>
        </div>
      </div>

      <div className="flex flex-col gap-1 flex-1">
        {NAV.map((item) => {
          const active = pathname.startsWith(item.href);
          const Icon = item.icon;
          return (
            <Link
              key={item.href}
              href={item.href}
              className={cn(
                "flex items-center gap-3 px-3 py-2 rounded font-mono-label text-mono-label transition-colors duration-150",
                active
                  ? "bg-tertiary-container text-tertiary font-bold"
                  : "text-on-surface-variant font-medium hover:text-on-surface hover:bg-surface-container-highest",
              )}
            >
              <Icon className="h-4 w-4" />
              {item.label}
            </Link>
          );
        })}
      </div>

      <Link
        href={`/${locale}/cells/new`}
        className="mt-4 bg-tertiary text-primary-container px-4 py-2 font-mono-label text-mono-label font-bold rounded flex items-center justify-center gap-2 hover:bg-tertiary-fixed transition-colors"
        data-testid="nav-create-cell"
      >
        <span className="text-sm">+</span> {t.shell.createCell}
      </Link>


    </nav>
  );
}

export function TopBar() {
  const t = useT();
  return (
    <header className="flex justify-between items-center h-16 px-4 w-full bg-surface-dim/80 backdrop-blur-md border-b border-outline-variant sticky top-0 z-10 md:px-8">
      <div className="md:hidden flex items-center gap-2">
        <Image src="/combee-64.png" alt="Combee Cloud" width={32} height={32} className="h-8 w-8 object-contain" priority />
        <span className="font-mono-code uppercase tracking-widest text-on-surface font-bold">{t.shell.product}</span>
      </div>

      <div className="flex items-center gap-4">
        <LangSwitcher />
        <Link href={`/${useLoc()}/account`} aria-label="Settings" className="text-on-surface-variant hover:text-tertiary transition-colors">
          <Settings className="h-5 w-5" />
        </Link>
        <UserMenu />
      </div>
    </header>
  );
}

export default function Shell({ children }: { children: React.ReactNode }) {
  return (
    <div className="bg-background text-on-surface min-h-screen flex">
      <SideNav />
      <main className="flex-1 flex flex-col min-w-0">
        <TopBar />
        <div className="p-4 md:p-8 flex-1 max-w-[1280px] mx-auto w-full">{children}</div>
      </main>
    </div>
  );
}
