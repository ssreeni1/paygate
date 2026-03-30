import type { Metadata } from "next";
import { JetBrains_Mono } from "next/font/google";
import "./globals.css";

const jetbrainsMono = JetBrains_Mono({
  subsets: ["latin"],
  variable: "--font-mono",
  display: "swap",
});

export const metadata: Metadata = {
  title: "agent tips",
  description: "AI agents tipping open source developers.",
  metadataBase: new URL(
    process.env.NEXT_PUBLIC_BASE_URL || "https://tips.paygate.fm"
  ),
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" className={jetbrainsMono.variable}>
      <body className="bg-canvas text-text-primary min-h-screen">
        <nav className="border-b border-border-subtle px-md py-sm">
          <div className="max-w-leaderboard mx-auto flex items-center justify-between">
            <a href="/" className="text-body text-text-dim hover:text-text-primary">
              agent-tips
            </a>
            <div className="flex gap-lg text-small text-text-dim">
              <a href="/leaderboard" className="hover:text-text-primary">
                leaderboard
              </a>
              <a href="/claim" className="hover:text-text-primary">
                claim
              </a>
            </div>
          </div>
        </nav>
        <main>{children}</main>
        <footer className="border-t border-border-subtle px-md py-sm mt-2xl">
          <div className="max-w-leaderboard mx-auto flex items-center justify-between text-label text-text-dim">
            <span>agent-tips</span>
            <div className="flex gap-lg">
              <a href="/terms" className="hover:text-text-primary">
                terms
              </a>
              <a href="/privacy" className="hover:text-text-primary">
                privacy
              </a>
            </div>
          </div>
        </footer>
      </body>
    </html>
  );
}
