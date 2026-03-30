"use client";

import { useState } from "react";

// TODO: Replace with real GitHub OAuth flow

export default function ClaimPage() {
  const [username, setUsername] = useState("");
  const [wallet, setWallet] = useState("");
  const [result, setResult] = useState<{ claimed?: number; error?: string } | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleClaim(e: React.FormEvent) {
    e.preventDefault();
    setLoading(true);
    setResult(null);

    try {
      const res = await fetch("/api/claim", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          github_username: username.replace("@", ""),
          wallet_address: wallet,
        }),
      });
      const data = await res.json();
      if (res.ok) {
        setResult({ claimed: data.claimed });
      } else {
        setResult({ error: data.error || "claim failed" });
      }
    } catch {
      setResult({ error: "network error" });
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="flex justify-center px-md py-2xl">
      <div className="w-full max-w-receipt">
        <p className="text-label text-text-dim uppercase tracking-widest mb-lg">
          claim tips
        </p>

        <p className="text-body text-text-muted mb-xl">
          If an AI agent tipped you, enter your GitHub username and a wallet
          address to claim your USDC.
        </p>

        <form onSubmit={handleClaim} className="space-y-md">
          <div>
            <label className="text-label text-text-dim uppercase tracking-wider block mb-xs">
              github username
            </label>
            <input
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="sindresorhus"
              className="w-full bg-surface border border-border-subtle px-md py-sm text-body text-text-primary placeholder:text-text-dim focus:outline-none focus:border-accent"
              required
            />
          </div>

          <div>
            <label className="text-label text-text-dim uppercase tracking-wider block mb-xs">
              wallet address
            </label>
            <input
              type="text"
              value={wallet}
              onChange={(e) => setWallet(e.target.value)}
              placeholder="0x..."
              className="w-full bg-surface border border-border-subtle px-md py-sm text-body text-text-primary placeholder:text-text-dim focus:outline-none focus:border-accent font-mono"
              required
            />
          </div>

          <button
            type="submit"
            disabled={loading}
            className="w-full border border-border-subtle px-lg py-sm text-body text-text-muted hover:text-accent hover:border-accent transition-colors disabled:opacity-50"
          >
            {loading ? "claiming..." : "claim"}
          </button>
        </form>

        {result && (
          <div className="mt-lg p-md border border-border-subtle">
            {result.claimed ? (
              <p className="text-body text-accent">
                ✓ claimed {result.claimed} tip{result.claimed !== 1 ? "s" : ""}
              </p>
            ) : (
              <p className="text-body text-error">
                × {result.error}
              </p>
            )}
          </div>
        )}

        <p className="text-small text-text-dim mt-xl">
          TODO: full GitHub OAuth flow. Currently accepts username directly.
        </p>
      </div>
    </div>
  );
}
