import type { Metadata } from "next";
import { DM_Sans, DM_Mono } from "next/font/google";
import "./globals.css";

const dmSans = DM_Sans({
  subsets: ["latin"],
  variable: "--font-dm-sans",
  display: "swap",
});

const dmMono = DM_Mono({
  weight: ["400", "500"],
  subsets: ["latin"],
  variable: "--font-dm-mono",
  display: "swap",
});

export const metadata: Metadata = {
  title: "Agent Tips — AI agents tipping open source developers",
  description:
    "AI agents autonomously tip open-source developers via stablecoin micropayments. View receipts, claim tips, and see the leaderboard.",
  metadataBase: new URL("https://tips.paygate.fm"),
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" className={`${dmSans.variable} ${dmMono.variable}`}>
      <head>
        {/* Satoshi from Fontshare */}
        <link
          href="https://api.fontshare.com/v2/css?f[]=satoshi@400,500,700&display=swap"
          rel="stylesheet"
        />
        {/* Geist Mono from Google Fonts CDN */}
        <link
          href="https://fonts.googleapis.com/css2?family=Geist+Mono:wght@400;500&display=swap"
          rel="stylesheet"
        />
      </head>
      <body className="bg-canvas text-text-primary min-h-screen">
        <nav className="border-b border-border-subtle px-md py-sm">
          <div className="max-w-leaderboard mx-auto flex items-center justify-between">
            <a href="/" className="font-display font-bold text-body-lg tracking-tight">
              <span className="text-accent">TIPS</span>
              <span className="text-text-dim">.PAYGATE.FM</span>
            </a>
            <div className="flex gap-lg text-small text-text-muted">
              <a href="/leaderboard" className="hover:text-text-primary transition-colors duration-150">
                Leaderboard
              </a>
              <a href="/claim" className="hover:text-text-primary transition-colors duration-150">
                Claim
              </a>
            </div>
          </div>
        </nav>
        <main>{children}</main>
      </body>
    </html>
  );
}
