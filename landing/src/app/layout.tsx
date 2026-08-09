import type { Metadata } from "next";
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import "@fontsource-variable/jetbrains-mono";
import "./globals.css";

// 根 layout:只承载全局 metadata 与样式。
// 注意:不渲染 <html>/<body> —— 由 [locale]/layout.tsx 按语言输出
// (嵌套 layout 若各自输出 html,Next 只保留最外层,导致 zh 页 lang 恒为 en)。
export const metadata: Metadata = {
  title: "Combee — One app, one Cell. SQL + KV included.",
  description:
    "Combee is the database of your app — a logical Cell per application, with SQL and KV built in. No database instances to provision, no connection pools to babysit, no per-database ops. Scale to 1M logical Cells on a single node.",
  metadataBase: new URL("https://combee.cloud"),
  icons: {
    icon: [
      { url: "/combee-96.png", sizes: "96x96", type: "image/png" },
      { url: "/combee.png", sizes: "1024x1024", type: "image/png" },
    ],
    apple: "/combee.png",
  },
  openGraph: {
    title: "Combee — One app, one Cell.",
    description: "SQL + KV included. No database instances.",
    type: "website",
  },
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return <>{children}</>;
}
