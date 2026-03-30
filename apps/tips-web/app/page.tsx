import type { Metadata } from "next";

export const metadata: Metadata = {
  title: "agent tips",
  description: "AI agents tipping open source developers.",
};

export default function HomePage() {
  return (
    <div className="flex justify-center px-md py-3xl">
      <div className="w-full max-w-content">
        <div className="mb-2xl">
          <pre className="text-text-dim text-small mb-lg select-none">
{`
   ┌─────────────────────────────────┐
   │         agent  tips             │
   └─────────────────────────────────┘
`}
          </pre>

          <p className="text-title text-text-primary mb-lg">
            AI agents tip open source developers.
          </p>

          <p className="text-body text-text-muted leading-relaxed mb-xl">
            When an AI agent uses an open source package, it sends
            a USDC microtip to the developer. Every tip is verified
            on-chain. No signup required.
          </p>
        </div>

        <pre className="text-text-dim text-small select-none mb-xl ascii-hr">
          {"────────────────────────────────────────────"}
        </pre>

        <div className="mb-2xl">
          <p className="text-label text-text-dim uppercase tracking-widest mb-lg">
            how it works
          </p>
          <div className="space-y-md text-body text-text-muted">
            <p>
              <span className="text-text-dim mr-sm">01</span>
              {"  "}agent uses an open source package
            </p>
            <p>
              <span className="text-text-dim mr-sm">02</span>
              {"  "}agent sends a USDC tip to the maintainer
            </p>
            <p>
              <span className="text-text-dim mr-sm">03</span>
              {"  "}developer claims with their GitHub identity
            </p>
          </div>
        </div>

        <pre className="text-text-dim text-small select-none mb-xl ascii-hr">
          {"────────────────────────────────────────────"}
        </pre>

        <div className="flex gap-lg mb-2xl">
          <a
            href="/leaderboard"
            className="text-body text-text-muted border border-border-subtle px-lg py-sm hover:text-accent hover:border-accent transition-colors"
          >
            leaderboard
          </a>
          <a
            href="/claim"
            className="text-body text-text-muted border border-border-subtle px-lg py-sm hover:text-accent hover:border-accent transition-colors"
          >
            claim tips
          </a>
        </div>

        <div className="mt-2xl">
          <p className="text-label text-text-dim uppercase tracking-widest mb-sm">
            add to your readme
          </p>
          <pre className="text-small text-text-muted bg-surface border border-border-subtle p-md overflow-x-auto">
{`![agent tips](https://tips.paygate.fm/badge/YOUR_USERNAME)`}
          </pre>
        </div>
      </div>
    </div>
  );
}
