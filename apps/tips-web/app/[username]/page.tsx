import { notFound } from "next/navigation";
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
    title: `@${params.username} — Agent Tips`,
    description: `View tips received by @${params.username} from AI agents.`,
    openGraph: {
      title: `@${params.username} — Agent Tips`,
    },
  };
}

export default async function ProfilePage({ params }: Props) {
  const { username } = params;

  let tips;
  try {
    tips = await getTipsByRecipient(username);
  } catch {
    notFound();
  }

  if (!tips || tips.length === 0) {
    return (
      <div className="flex justify-center px-md py-2xl">
        <div className="w-full max-w-content text-center">
          <div className="mb-lg">
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img
              src={`https://github.com/${username}.png?size=128`}
              alt={username}
              width={80}
              height={80}
              className="rounded-full avatar-glow mx-auto mb-md"
            />
            <h1 className="font-display text-title font-bold">@{username}</h1>
          </div>
          <p className="text-text-muted text-body-lg">
            No tips received yet.
          </p>
          <p className="text-text-dim text-small mt-sm">
            When an AI agent tips this developer, it will appear here.
          </p>
        </div>
      </div>
    );
  }

  const totalUsdc = tips.reduce((sum, t) => sum + t.amount_usdc, 0);
  const uniqueAgents = new Set(tips.map((t) => t.sender_name));
  const uniquePackages = new Set(tips.map((t) => t.package_name));

  return (
    <div className="flex justify-center px-md py-2xl">
      <div className="w-full max-w-content">
        {/* Profile header */}
        <div className="flex flex-col items-center mb-2xl">
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img
            src={`https://github.com/${username}.png?size=128`}
            alt={username}
            width={80}
            height={80}
            className="rounded-full avatar-glow mb-md"
          />
          <h1 className="font-display text-title font-bold">@{username}</h1>
          <p className="text-text-dim text-small mt-xs">Open source developer</p>
        </div>

        {/* Stats */}
        <div className="grid grid-cols-3 gap-md mb-2xl">
          <StatCard label="Total Tipped" value={formatUsdc(totalUsdc)} accent />
          <StatCard label="Tips" value={String(tips.length)} />
          <StatCard label="Agents" value={String(uniqueAgents.size)} />
        </div>

        {/* Packages */}
        <div className="mb-xl">
          <p className="text-label text-text-dim uppercase tracking-wider mb-sm">
            Packages
          </p>
          <div className="flex flex-wrap gap-sm">
            {Array.from(uniquePackages).map((pkg) => (
              <span
                key={pkg}
                className="font-mono text-label text-text-muted bg-surface px-sm py-xs rounded-md border border-border-subtle"
              >
                {pkg}
              </span>
            ))}
          </div>
        </div>

        {/* Recent tips */}
        <div>
          <p className="text-label text-text-dim uppercase tracking-wider mb-md">
            Recent Tips
          </p>
          <div className="space-y-sm">
            {tips.map((tip) => (
              <a
                key={tip.id}
                href={`/tx/${tip.id}`}
                className="block bg-surface rounded-md border border-border-subtle p-md hover:bg-surface-hover transition-colors duration-150"
              >
                <div className="flex justify-between items-start mb-xs">
                  <div>
                    <span className="font-mono text-body text-accent font-medium">
                      {formatUsdc(tip.amount_usdc)}
                    </span>
                    <span className="text-text-dim text-small ml-sm">
                      for{" "}
                      <span className="font-mono text-text-muted">
                        {tip.package_name}
                      </span>
                    </span>
                  </div>
                  <StatusBadge status={tip.status} />
                </div>
                {tip.reason && (
                  <p className="text-small text-text-muted italic mb-xs">
                    &ldquo;{tip.reason}&rdquo;
                  </p>
                )}
                <div className="flex justify-between text-label text-text-dim">
                  <span>from {tip.sender_name}</span>
                  <span className="font-mono">{truncateHash(tip.tx_hash)}</span>
                </div>
                <p className="text-label text-text-dim mt-2xs">
                  {formatDate(tip.created_at)}
                </p>
              </a>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

function StatCard({
  label,
  value,
  accent,
}: {
  label: string;
  value: string;
  accent?: boolean;
}) {
  return (
    <div className="bg-surface rounded-md border border-border-subtle p-md text-center">
      <p
        className={`font-mono text-section font-bold ${accent ? "text-accent" : "text-text-primary"}`}
      >
        {value}
      </p>
      <p className="text-label text-text-dim uppercase tracking-wider mt-xs">
        {label}
      </p>
    </div>
  );
}

function StatusBadge({ status }: { status: string }) {
  const cls =
    status === "paid"
      ? "badge-paid"
      : status === "escrowed"
        ? "badge-escrowed"
        : status === "claimed"
          ? "badge-claimed"
          : "badge-reclaimed";
  return (
    <span
      className={`${cls} font-mono text-label uppercase tracking-wider px-sm py-2xs rounded-full`}
    >
      {status}
    </span>
  );
}
