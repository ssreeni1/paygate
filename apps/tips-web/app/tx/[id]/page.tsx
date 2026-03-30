import { notFound } from "next/navigation";
import type { Metadata } from "next";
import {
  getTip,
  getTipsByRecipient,
  formatUsdc,
  truncateAddress,
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
    return {
      title: `${amount} tip to @${tip.recipient_gh} for ${tip.package_name}`,
      description: tip.reason,
      openGraph: {
        title: `${amount} tip to @${tip.recipient_gh}`,
        description: `Agent ${tip.sender_name} tipped @${tip.recipient_gh} for ${tip.package_name}: "${tip.reason}"`,
        type: "article",
      },
      twitter: {
        card: "summary_large_image",
      },
    };
  } catch {
    return { title: "Tip Receipt — Agent Tips" };
  }
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
      className={`${cls} font-mono text-label uppercase tracking-wider px-[10px] py-[4px] rounded-full animate-fade-in`}
    >
      {status}
    </span>
  );
}

export default async function ReceiptPage({ params }: Props) {
  let tip;
  try {
    tip = await getTip(params.id);
  } catch {
    notFound();
  }

  const amount = formatUsdc(tip.amount_usdc);

  // Fetch social proof: all tips to this recipient
  let totalTipped = tip.amount_usdc;
  let agentCount = 1;
  try {
    const allTips = await getTipsByRecipient(tip.recipient_gh);
    totalTipped = allTips.reduce((sum, t) => sum + t.amount_usdc, 0);
    const uniqueAgents = new Set(allTips.map((t) => t.sender_name));
    agentCount = uniqueAgents.size;
  } catch {
    // Social proof is non-critical; use defaults
  }

  return (
    <div className="flex justify-center px-md py-2xl">
      <div className="w-full max-w-receipt">
        {/* Receipt card */}
        <div className="bg-surface rounded-lg card-top-border receipt-texture overflow-hidden">
          <div className="p-xl">
            {/* Amount */}
            <div className="text-center mb-xl">
              <p className="text-label font-mono text-text-dim uppercase tracking-widest mb-sm">
                Agent Tip
              </p>
              <p className="font-mono text-hero text-accent animate-count-up font-bold">
                {amount}
              </p>
              <p className="text-label font-mono text-text-dim mt-xs">USDC</p>
            </div>

            {/* Reason — italic pull quote */}
            {tip.reason && (
              <div className="border-l-2 border-accent pl-md py-xs mb-xl">
                <p className="text-body-lg text-text-primary italic">
                  &ldquo;{tip.reason}&rdquo;
                </p>
                <p className="text-small text-text-dim mt-xs">
                  for{" "}
                  <span className="font-mono text-text-muted">
                    {tip.package_name}
                  </span>
                </p>
              </div>
            )}

            {/* Parties: sender -> recipient */}
            <div className="flex items-center justify-center gap-lg mb-xl">
              {/* Sender (agent) */}
              <div className="flex flex-col items-center gap-sm">
                <div className="w-12 h-12 rounded-full avatar-agent bg-surface-hover flex items-center justify-center">
                  <span className="font-mono text-label text-text-dim">AI</span>
                </div>
                <div className="text-center">
                  <p className="text-small text-text-muted font-mono">
                    {tip.sender_name}
                  </p>
                  <p className="text-label text-text-dim">
                    {truncateAddress(tip.sender_wallet)}
                  </p>
                </div>
              </div>

              {/* Arrow */}
              <div className="text-text-dim text-body-lg font-mono">
                &rarr;
              </div>

              {/* Recipient (human) */}
              <div className="flex flex-col items-center gap-sm">
                {/* eslint-disable-next-line @next/next/no-img-element */}
                <img
                  src={`https://github.com/${tip.recipient_gh}.png?size=96`}
                  alt={tip.recipient_gh}
                  width={48}
                  height={48}
                  className="rounded-full avatar-glow"
                />
                <div className="text-center">
                  <p className="text-small text-text-primary font-medium">
                    @{tip.recipient_gh}
                  </p>
                  <p className="text-label text-text-dim">Developer</p>
                </div>
              </div>
            </div>

            {/* Status badge */}
            <div className="flex justify-center mb-lg">
              <StatusBadge status={tip.status} />
            </div>

            {/* Escrowed CTA */}
            {tip.status === "escrowed" && (
              <div className="bg-canvas rounded-md p-md mb-lg text-center border border-border-subtle">
                <p className="text-small text-warning">
                  Unclaimed — claim at{" "}
                  <a
                    href="/claim"
                    className="underline hover:text-accent transition-colors duration-150"
                  >
                    tips.paygate.fm/claim
                  </a>
                </p>
              </div>
            )}

            {/* Divider */}
            <div className="border-t border-border-subtle my-lg" />

            {/* Meta rows */}
            <div className="space-y-sm">
              <MetaRow label="Transaction" value={truncateHash(tip.tx_hash)} mono />
              <MetaRow label="Tip ID" value={tip.id} mono />
              <MetaRow label="Timestamp" value={formatDate(tip.created_at)} />
            </div>

            {/* Divider */}
            <div className="border-t border-border-subtle my-lg" />

            {/* Social proof */}
            <div className="text-center">
              <p className="text-small text-text-dim">
                This developer has been tipped{" "}
                <span className="text-accent font-mono font-medium">
                  {formatUsdc(totalTipped)}
                </span>{" "}
                by{" "}
                <span className="text-text-muted font-medium">
                  {agentCount} agent{agentCount !== 1 ? "s" : ""}
                </span>
              </p>
            </div>
          </div>

          {/* Footer branding */}
          <div className="border-t border-border-subtle px-xl py-md flex justify-between items-center">
            <p className="text-label text-text-dim font-mono">
              <span className="text-accent">TIPS</span>.PAYGATE.FM
            </p>
            <p className="text-label text-text-dim">
              Verified on-chain
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}

function MetaRow({
  label,
  value,
  mono,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="flex justify-between items-baseline">
      <span className="text-label text-text-dim uppercase tracking-wider">
        {label}
      </span>
      <span
        className={`text-small text-text-muted ${mono ? "font-mono" : ""}`}
      >
        {value}
      </span>
    </div>
  );
}
