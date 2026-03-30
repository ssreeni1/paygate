import type { Metadata } from "next";
import {
  getTipsByRecipient,
  formatUsdc,
  truncateHash,
  formatDate,
} from "@/lib/api";

interface Props {
  params: { username: string };
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  return {
    title: `@${params.username} — agent tips`,
    description: `Tips received by @${params.username} from AI agents.`,
  };
}

export default async function ProfilePage({ params }: Props) {
  const { username } = params;

  let tips: Awaited<ReturnType<typeof getTipsByRecipient>> = [];
  try {
    tips = await getTipsByRecipient(username);
  } catch {
    tips = [];
  }

  const totalUsdc = tips.reduce((sum, t) => sum + t.amount_usdc, 0);
  const uniqueAgents = new Set(tips.map((t) => t.sender_name));

  return (
    <div className="flex justify-center px-md py-2xl">
      <div className="w-full max-w-content">
        {/* Header */}
        <div className="mb-xl">
          <p className="text-title text-text-primary">@{username}</p>
          {tips.length > 0 && (
            <p className="text-body text-text-dim mt-xs">
              <span className="text-accent">{formatUsdc(totalUsdc)}</span>
              {" "}from {uniqueAgents.size} agent{uniqueAgents.size !== 1 ? "s" : ""}
              {" "}across {tips.length} tip{tips.length !== 1 ? "s" : ""}
            </p>
          )}
        </div>

        <pre className="text-text-dim text-small select-none mb-lg ascii-hr">
          {"────────────────────────────────────────────"}
        </pre>

        {tips.length === 0 ? (
          <p className="text-body text-text-dim py-xl text-center">
            no tips received yet.
          </p>
        ) : (
          <div className="space-y-xs">
            {tips.map((tip) => (
              <a
                key={tip.id}
                href={`/tx/${tip.id}`}
                className="flex items-center justify-between py-sm border-b border-border-subtle hover:bg-surface-hover transition-colors"
              >
                <div className="flex-1 min-w-0">
                  <span className="text-body text-text-primary">
                    {tip.package_name || tip.recipient_gh}
                  </span>
                  <span className="text-small text-text-dim ml-md">
                    {tip.sender_name || "agent"}
                  </span>
                </div>
                <div className="flex items-center gap-md ml-md">
                  <span className="text-body text-accent">
                    {formatUsdc(tip.amount_usdc)}
                  </span>
                  <span className="text-small text-text-dim">
                    {formatDate(tip.created_at)}
                  </span>
                </div>
              </a>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
