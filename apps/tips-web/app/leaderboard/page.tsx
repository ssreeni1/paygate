import type { Metadata } from "next";
import { getLeaderboard, formatUsdc } from "@/lib/api";

export const metadata: Metadata = {
  title: "leaderboard — agent tips",
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
        <p className="text-label text-text-dim uppercase tracking-widest mb-lg">
          leaderboard
        </p>

        {entries.length === 0 ? (
          <div className="py-3xl text-center">
            <p className="text-body text-text-dim">
              no tips yet. be the first.
            </p>
          </div>
        ) : (
          <div>
            {/* Header */}
            <div className="flex text-label text-text-dim uppercase tracking-wider border-b border-border-subtle pb-sm mb-sm">
              <span className="w-12">#</span>
              <span className="flex-1">developer</span>
              <span className="w-28 text-right">total</span>
              <span className="w-16 text-right">tips</span>
            </div>

            {/* Rows */}
            {entries.map((entry, i) => (
              <a
                key={entry.github_username}
                href={`/${entry.github_username}`}
                className="flex items-center py-sm border-b border-border-subtle hover:bg-surface-hover transition-colors"
              >
                <span className="w-12 text-small text-text-dim">
                  {String(i + 1).padStart(2, "0")}
                </span>
                <span className="flex-1 text-body text-text-primary">
                  @{entry.github_username}
                </span>
                <span className="w-28 text-right text-body text-accent">
                  {formatUsdc(entry.total_usdc)}
                </span>
                <span className="w-16 text-right text-small text-text-dim">
                  {entry.tip_count}
                </span>
              </a>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
