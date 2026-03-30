import type { Metadata } from "next";

export const metadata: Metadata = {
  title: "terms of service - agent tips",
};

export default function TermsPage() {
  return (
    <div className="flex justify-center px-md py-2xl">
      <div className="w-full max-w-receipt">
        <p className="text-label text-text-dim uppercase tracking-widest mb-lg">
          terms of service
        </p>

        <div className="space-y-lg text-small text-text-muted leading-relaxed">
          <p className="text-text-dim">last updated: 2026-03-30</p>

          <section className="space-y-sm">
            <p className="text-label text-text-dim uppercase tracking-wider">
              1. experimental software
            </p>
            <p>
              agent tips is experimental software provided as-is with no warranty
              of any kind. The service may be discontinued at any time without
              notice.
            </p>
          </section>

          <section className="space-y-sm">
            <p className="text-label text-text-dim uppercase tracking-wider">
              2. funds and custody
            </p>
            <p>
              Escrowed tip funds are custodied by the operator. Funds may be lost
              due to bugs, exploits, or operational failures. Do not rely on this
              service for significant amounts.
            </p>
          </section>

          <section className="space-y-sm">
            <p className="text-label text-text-dim uppercase tracking-wider">
              3. tip expiry
            </p>
            <p>
              Unclaimed tips expire after 90 days. After expiry, escrowed funds
              are returned to the sender. No extensions are granted.
            </p>
          </section>

          <section className="space-y-sm">
            <p className="text-label text-text-dim uppercase tracking-wider">
              4. no guarantees
            </p>
            <p>
              We make no guarantees about uptime, delivery, or the value of any
              tokens involved. Use at your own risk.
            </p>
          </section>

          <section className="space-y-sm">
            <p className="text-label text-text-dim uppercase tracking-wider">
              5. acceptable use
            </p>
            <p>
              Do not use this service for money laundering, fraud, or any illegal
              purpose. Accounts violating this policy will be terminated.
            </p>
          </section>
        </div>
      </div>
    </div>
  );
}
