import { notFound } from "next/navigation";
import type { Metadata } from "next";
import {
  getTip,
  getTipsByRecipient,
  formatUsdc,
  truncateHash,
  formatDate,
} from "@/lib/api";

interface Props {
  params: { id: string };
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  try {
    const tip = await getTip(params.id);
    const amount = formatUsdc(tip.amount_usdc);
    const pkg = tip.package_name || tip.recipient_gh;
    return {
      title: `${amount} tip to @${tip.recipient_gh}`,
      description: `${tip.sender_name || "an agent"} tipped @${tip.recipient_gh} for ${pkg}`,
      openGraph: {
        title: `${amount} tip to @${tip.recipient_gh}`,
        description: `${tip.sender_name || "an agent"} tipped @${tip.recipient_gh} for ${pkg}: "${tip.reason}"`,
        type: "article",
      },
      twitter: { card: "summary_large_image" },
    };
  } catch {
    return { title: "tip receipt" };
  }
}

export default async function ReceiptPage({ params }: Props) {
  let tip;
  try {
    tip = await getTip(params.id);
  } catch {
    notFound();
  }

  if (!tip) notFound();

  const amount = formatUsdc(tip.amount_usdc);
  const pkg = tip.package_name || null;

  let totalTipped = tip.amount_usdc;
  let agentCount = 1;
  try {
    const allTips = await getTipsByRecipient(tip.recipient_gh);
    totalTipped = allTips.reduce((sum, t) => sum + t.amount_usdc, 0);
    agentCount = new Set(allTips.map((t) => t.sender_name)).size;
  } catch {}

  const statusChar =
    tip.status === "paid" ? "●" :
    tip.status === "escrowed" ? "○" :
    tip.status === "claimed" ? "✓" : "×";

  const statusColor =
    tip.status === "paid" || tip.status === "claimed" ? "text-accent" :
    tip.status === "escrowed" ? "text-warning" : "text-text-dim";

  return (
    <div className="flex justify-center px-md py-2xl">
      <div className="w-full max-w-receipt print-in">
        <div className="border border-border-subtle">
          <div className="px-lg py-xl">
          <div className="text-center mb-xl">
            <p className="text-label text-text-dim uppercase tracking-widest mb-md">
              tip receipt
            </p>
            <p className="text-hero text-accent font-bold print-in">
              {amount}
            </p>
            <p className="text-small text-text-dim mt-xs">USDC on Tempo</p>
          </div>

          {tip.reason && (
            <div className="mb-xl px-md">
              <p className="text-body-lg text-text-primary italic text-center">
                &ldquo;{tip.reason}&rdquo;
              </p>
              {pkg && (
                <p className="text-small text-text-dim text-center mt-xs">
                  for {pkg}
                </p>
              )}
            </div>
          )}

          <pre className="text-text-dim text-small text-center select-none my-lg">
            {"── ── ── ── ── ── ── ── ── ── ──"}
          </pre>

          <div className="flex items-start justify-between mb-xl">
            <div>
              <p className="text-label text-text-dim uppercase mb-xs">from</p>
              <p className="text-body text-text-muted">
                {tip.sender_name || "agent"}
              </p>
            </div>
            <div className="text-text-dim px-md pt-sm">{"->"}</div>
            <div className="text-right">
              <p className="text-label text-text-dim uppercase mb-xs">to</p>
              <a
                href={`/${tip.recipient_gh}`}
                className="text-body text-text-primary hover:text-accent"
              >
                @{tip.recipient_gh}
              </a>
            </div>
          </div>

          <div className="flex justify-between items-center mb-lg">
            <span className="text-label text-text-dim uppercase">status</span>
            <span className={`text-small ${statusColor}`}>
              {statusChar} {tip.status}
            </span>
          </div>

          {tip.status === "escrowed" && (
            <div className="border border-border-subtle p-md mb-lg text-center">
              <p className="text-small text-warning">
                unclaimed.{" "}
                <a href="/claim" className="underline hover:text-accent">
                  claim here
                </a>
              </p>
            </div>
          )}

          <pre className="text-text-dim text-small text-center select-none my-lg">
            {"── ── ── ── ── ── ── ── ── ── ──"}
          </pre>

          <div className="space-y-xs text-small">
            {tip.tx_hash && (
              <div className="flex justify-between">
                <span className="text-text-dim">tx</span>
                <span className="text-text-muted">{truncateHash(tip.tx_hash)}</span>
              </div>
            )}
            <div className="flex justify-between">
              <span className="text-text-dim">id</span>
              <span className="text-text-muted">{tip.id}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-text-dim">time</span>
              <span className="text-text-muted">{formatDate(tip.created_at)}</span>
            </div>
          </div>

          <div className="mt-xl pt-lg border-t border-border-subtle text-center">
            <p className="text-small text-text-dim">
              @{tip.recipient_gh} has received{" "}
              <span className="text-accent">{formatUsdc(totalTipped)}</span>
              {" "}from {agentCount} agent{agentCount !== 1 ? "s" : ""}
            </p>
          </div>
        </div></div>
      </div>
    </div>
  );
}
