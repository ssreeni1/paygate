"use client";

import { useState, useEffect } from "react";

interface SessionData {
  authenticated: boolean;
  user?: string;
  orgs?: string[];
}

export default function ClaimPage() {
  const [session, setSession] = useState<SessionData | null>(null);
  const [wallet, setWallet] = useState("");
  const [result, setResult] = useState<{
    claimed?: number;
    authenticated_as?: string;
    error?: string;
  } | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    fetch("/api/auth/session")
      .then((r) => r.json())
      .then(setSession)
      .catch(() => setSession({ authenticated: false }));
  }, []);

  async function handleClaim(e: React.FormEvent) {
    e.preventDefault();
    setLoading(true);
    setResult(null);

    try {
      const res = await fetch("/api/claim", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ wallet_address: wallet }),
      });
      const data = await res.json();
      if (res.ok) {
        setResult({ claimed: data.claimed, authenticated_as: data.authenticated_as });
      } else {
        setResult({ error: data.error || "claim failed" });
      }
    } catch {
      setResult({ error: "network error" });
    } finally {
      setLoading(false);
    }
  }

  // Loading state
  if (session === null) {
    return (
      <div className="flex justify-center px-md py-2xl">
        <div className="w-full max-w-receipt">
          <p className="text-body text-text-dim">loading...</p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex justify-center px-md py-2xl">
      <div className="w-full max-w-receipt">
        <p className="text-label text-text-dim uppercase tracking-widest mb-lg">
          claim tips
        </p>

        {!session.authenticated ? (
          <>
            <p className="text-body text-text-muted mb-xl">
              Sign in with GitHub to verify your identity and claim any
              tips sent to you or your organizations.
            </p>

            <a
              href="/api/auth/github"
              className="inline-block border border-border-subtle px-lg py-sm text-body text-text-muted hover:text-accent hover:border-accent transition-colors"
            >
              sign in with github
            </a>

            {/* Show error from redirect if any */}
            {typeof window !== "undefined" &&
              new URLSearchParams(window.location.search).get("error") && (
                <p className="text-body text-error mt-lg">
                  authentication failed. try again.
                </p>
              )}
          </>
        ) : (
          <>
            <div className="mb-xl">
              <p className="text-body text-text-primary">
                authenticated as{" "}
                <span className="text-accent">@{session.user}</span>
              </p>
              {session.orgs && session.orgs.length > 0 && (
                <p className="text-small text-text-dim mt-xs">
                  orgs: {session.orgs.map((o) => `@${o}`).join(", ")}
                </p>
              )}
            </div>

            <form onSubmit={handleClaim} className="space-y-md">
              <div>
                <label className="text-label text-text-dim uppercase tracking-wider block mb-xs">
                  wallet address
                </label>
                <input
                  type="text"
                  value={wallet}
                  onChange={(e) => setWallet(e.target.value)}
                  placeholder="0x..."
                  className="w-full bg-surface border border-border-subtle px-md py-sm text-body text-text-primary placeholder:text-text-dim focus:outline-none focus:border-accent"
                  required
                />
              </div>

              <button
                type="submit"
                disabled={loading}
                className="w-full border border-border-subtle px-lg py-sm text-body text-text-muted hover:text-accent hover:border-accent transition-colors disabled:opacity-50"
              >
                {loading ? "claiming..." : "claim tips"}
              </button>
            </form>

            {result && (
              <div className="mt-lg p-md border border-border-subtle">
                {result.claimed ? (
                  <p className="text-body text-accent">
                    ✓ claimed {result.claimed} tip
                    {result.claimed !== 1 ? "s" : ""} as @
                    {result.authenticated_as}
                  </p>
                ) : (
                  <p className="text-body text-error">× {result.error}</p>
                )}
              </div>
            )}

            <p className="text-small text-text-dim mt-xl">
              Tips sent to @{session.user}
              {session.orgs && session.orgs.length > 0
                ? ` and orgs (${session.orgs.join(", ")})`
                : ""}{" "}
              will be claimed.
            </p>
          </>
        )}
      </div>
    </div>
  );
}
