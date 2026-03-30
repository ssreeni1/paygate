import type { Metadata } from "next";

export const metadata: Metadata = {
  title: "privacy policy - agent tips",
};

export default function PrivacyPage() {
  return (
    <div className="flex justify-center px-md py-2xl">
      <div className="w-full max-w-receipt">
        <p className="text-label text-text-dim uppercase tracking-widest mb-lg">
          privacy policy
        </p>

        <div className="space-y-lg text-small text-text-muted leading-relaxed">
          <p className="text-text-dim">last updated: 2026-03-30</p>

          <section className="space-y-sm">
            <p className="text-label text-text-dim uppercase tracking-wider">
              1. data we collect
            </p>
            <p>
              We collect: GitHub username, wallet address, tip amounts, tip
              reasons, and package names. This data is required for the service
              to function.
            </p>
          </section>

          <section className="space-y-sm">
            <p className="text-label text-text-dim uppercase tracking-wider">
              2. data retention
            </p>
            <p>
              Claimed tips and associated data are stored indefinitely as
              transaction records. Unclaimed tips are retained for 90 days, after
              which they are expired and may be purged.
            </p>
          </section>

          <section className="space-y-sm">
            <p className="text-label text-text-dim uppercase tracking-wider">
              3. cookies
            </p>
            <p>
              We use a single session cookie for GitHub authentication. No
              tracking cookies. No analytics cookies. No third-party cookies.
            </p>
          </section>

          <section className="space-y-sm">
            <p className="text-label text-text-dim uppercase tracking-wider">
              4. data sharing
            </p>
            <p>
              We do not sell your data. Tip data (amounts, recipients, reasons)
              is publicly visible by design. Wallet addresses are visible
              on-chain.
            </p>
          </section>

          <section className="space-y-sm">
            <p className="text-label text-text-dim uppercase tracking-wider">
              5. third parties
            </p>
            <p>
              We use GitHub OAuth for authentication and the Tempo blockchain for
              payments. Their respective privacy policies apply to those
              interactions.
            </p>
          </section>
        </div>
      </div>
    </div>
  );
}
