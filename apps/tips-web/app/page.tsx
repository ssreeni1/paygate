import type { Metadata } from "next";

export const metadata: Metadata = {
  title: "Agent Tips — AI agents tipping open source developers",
  description:
    "AI agents autonomously tip open-source developers via stablecoin micropayments.",
};

export default function HomePage() {
  return (
    <div className="flex justify-center px-md py-3xl">
      <div className="w-full max-w-content text-center">
        {/* Hero */}
        <div className="mb-2xl">
          <p className="text-label font-mono text-accent uppercase tracking-widest mb-md">
            Agent Tips
          </p>
          <h1 className="font-display text-display font-bold leading-tight mb-lg">
            AI agents tip
            <br />
            open source developers
          </h1>
          <p className="text-body-lg text-text-muted max-w-md mx-auto">
            When an AI agent uses an open source package, it can autonomously
            send a USDC microtip to the developer. Every tip is verified
            on-chain.
          </p>
        </div>

        {/* CTA */}
        <div className="flex flex-col items-center gap-md mb-3xl">
          <a
            href="/leaderboard"
            className="inline-block bg-accent text-canvas font-display font-bold text-body-lg px-xl py-sm rounded-md hover:opacity-90 transition-opacity duration-150"
          >
            View Leaderboard
          </a>
          <a
            href="/claim"
            className="text-small text-text-muted hover:text-accent transition-colors duration-150"
          >
            Have unclaimed tips? Claim now
          </a>
        </div>

        {/* How it works */}
        <div className="border-t border-border-subtle pt-2xl">
          <p className="text-label font-mono text-text-dim uppercase tracking-widest mb-xl">
            How it works
          </p>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-lg text-left">
            <Step
              num="01"
              title="Agent uses a package"
              desc="An AI agent imports an open source library to complete a task."
            />
            <Step
              num="02"
              title="Agent sends a tip"
              desc="The agent autonomously sends a USDC microtip via PayGate to the package maintainer."
            />
            <Step
              num="03"
              title="Developer claims"
              desc="The developer verifies their GitHub identity and claims USDC to their wallet."
            />
          </div>
        </div>

        {/* Badge embed */}
        <div className="border-t border-border-subtle mt-2xl pt-2xl">
          <p className="text-label font-mono text-text-dim uppercase tracking-widest mb-md">
            Add to your README
          </p>
          <div className="bg-surface rounded-md border border-border-subtle p-md">
            <code className="text-small font-mono text-text-muted break-all">
              {'![Agent Tips](https://tips.paygate.fm/badge/YOUR_USERNAME)'}
            </code>
          </div>
        </div>
      </div>
    </div>
  );
}

function Step({
  num,
  title,
  desc,
}: {
  num: string;
  title: string;
  desc: string;
}) {
  return (
    <div className="bg-surface rounded-md border border-border-subtle p-lg">
      <p className="font-mono text-label text-accent mb-sm">{num}</p>
      <p className="font-display text-body-lg font-bold text-text-primary mb-xs">
        {title}
      </p>
      <p className="text-small text-text-muted">{desc}</p>
    </div>
  );
}
