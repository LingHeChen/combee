import type { Metadata } from "next";
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import "./globals.css";

export const metadata: Metadata = {
  title: "Combee Cloud",
  description: "One app, one Cell. SQL + KV included.",
  icons: {
    icon: [
      { url: "/combee-64.png", sizes: "64x64", type: "image/png" },
      { url: "/combee-192.png", sizes: "192x192", type: "image/png" },
    ],
    apple: "/combee-192.png",
  },
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en" className="dark">
      <body className="bg-background text-foreground antialiased">
        {children}
      </body>
    </html>
  );
}
