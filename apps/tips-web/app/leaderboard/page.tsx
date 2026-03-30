import type { Metadata } from "next";
import { getLeaderboard, formatUsdc } from "@/lib/api";

export const metadata: Metadata = {
  title: "Leaderboard — Agent Tips",
  description: "Top open source developers tipped by AI agents.",
};

export default async function LeaderboardPage() {
  let entries: Awaited<ReturnType<typeof getLeaderboard>> = [];
  try {
    entries = await getLeaderboard();
  } catch {
    entries = [];
  }

  return (
    <div className="flex justify-center px-md py-2xl">
      <div className="w-full max-w-leaderboard">
        <div className="text-center mb-2xl">
          <h1 className="font-display text-title font-bold mb-sm">
            Leaderboard
          </h1>
          <p className="text-text-muted text-body-lg">
            Open source developers tipped by AI agents
          </p>
        </div>

        {entries.length === 0 ? (
          <div className="text-center py-3xl">
            <p className="text-text-dim text-subtitle">
              Be the first to tip an open source developer
            </p>
            <p className="text-text-dim text-small mt-sm">
              When agents start tipping, the leaderboard will populate here.
            </p>
          </div>
        ) : (
          <div className="bg-surface rounded-lg border border-border-subtle overflow-hidden">
            {/* Header */}
            <div className="grid grid-cols-[60px_1fr_140px_100px_100px] gap-md px-xl py-md border-b border-border-subtle">
              <span className="text-label text-text-dim uppercase tracking-wider">
                Rank
              </span>
              <span className="text-label text-text-dim uppercase tracking-wider">
                Developer
              </span>
              <span className="text-label text-text-dim uppercase tracking-wider text-right">
                Total Tipped
              </span>
              <span className="text-label text-text-dim uppercase tracking-wider text-right">
                Tips
              </span>
              <span className="text-label text-text-dim uppercase tracking-wider text-right">
                Agents
              </span>
            </div>

            {/* Rows */}
            {entries.map((entry, i) => (
              <a
                key={entry.github_username}
                href={`/${entry.github_username}`}
                className="grid grid-cols-[60px_1fr_140px_100px_100px] gap-md px-xl py-md border-b border-border-subtle last:border-0 hover:bg-surface-hover transition-colors duration-150 items-center"
              >
                <span className="font-mono text-body text-text-dim">
                  {String(i + 1).padStart(2, "0")}
                </span>
                <div className="flex items-center gap-sm">
                  {/* eslint-disable-next-line @next/next/no-img-element */}
                  <img
                    src={`https://github.com/${entry.github_username}.png?size=64`}
                    alt={entry.github_username}
                    width={32}
                    height={32}
                    className="rounded-full avatar-glow"
                  />
                  <span className="text-body text-text-primary font-medium">
                    @{entry.github_username}
                  </span>
                </div>
                <span className="font-mono text-body text-accent text-right font-medium">
                  {formatUsdc(entry.total_usdc)}
                </span>
                <span className="font-mono text-body text-text-muted text-right">
                  {entry.tip_count}
                </span>
                <span className="font-mono text-body text-text-muted text-right">
                  {entry.agent_count}
                </span>
              </a>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
