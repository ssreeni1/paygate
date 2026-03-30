"use client";

import { useState } from "react";

// TODO: Replace this placeholder flow with real GitHub OAuth.
// Requires a GitHub OAuth App with callback URL pointing to /api/auth/callback.
// Flow: GitHub login -> verify identity -> accept wallet address -> POST /paygate/internal/claim

type ClaimState = "input" | "loading" | "success" | "error";

export default function ClaimPage() {
  const [state, setState] = useState<ClaimState>("input");
  const [username, setUsername] = useState("");
  const [wallet, setWallet] = useState("");
  const [result, setResult] = useState<{
    claimed_count: number;
    total_usdc: number;
  } | null>(null);
  const [errorMsg, setErrorMsg] = useState("");

  async function handleClaim() {
    if (!username.trim() || !wallet.trim()) return;

    setState("loading");
    setErrorMsg("");

    try {
      const res = await fetch("/api/claim", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          github_username: username.trim().replace(/^@/, ""),
          wallet_address: wallet.trim(),
        }),
      });

      if (!res.ok) {
        const body = await res.text();
        throw new Error(body || `Error ${res.status}`);
      }

      const data = await res.json();
      setResult(data);
      setState("success");
    } catch (err) {
      setErrorMsg(err instanceof Error ? err.message : "Claim failed");
      setState("error");
    }
  }

  return (
    <div className="flex justify-center px-md py-2xl">
      <div className="w-full max-w-receipt">
        <div className="text-center mb-2xl">
          <h1 className="font-display text-title font-bold mb-sm">
            Claim Your Tips
          </h1>
          <p className="text-text-muted text-body-lg">
            AI agents have tipped open source developers. If you have unclaimed
            tips, enter your details below.
          </p>
        </div>

        <div className="bg-surface rounded-lg card-top-border overflow-hidden p-xl">
          {state === "success" && result ? (
            <div className="text-center py-lg">
              <div className="text-hero font-mono text-accent font-bold mb-md animate-count-up">
                ${(result.total_usdc / 1_000_000).toFixed(2)}
              </div>
              <p className="text-body-lg text-text-primary mb-sm">
                {result.claimed_count} tip
                {result.claimed_count !== 1 ? "s" : ""} claimed
              </p>
              <p className="text-small text-text-dim">
                Funds will be sent to your wallet shortly.
              </p>
            </div>
          ) : (
            <>
              {/* TODO: Replace with "Sign in with GitHub" OAuth button */}
              <div className="mb-lg">
                <p className="text-label text-text-dim uppercase tracking-wider mb-sm">
                  GitHub Username
                </p>
                <input
                  type="text"
                  placeholder="sindresorhus"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  className="w-full bg-canvas border border-border rounded-md px-md py-sm text-body text-text-primary font-mono placeholder:text-text-dim focus:outline-none focus:border-accent transition-colors duration-150"
                />
              </div>

              <div className="mb-xl">
                <p className="text-label text-text-dim uppercase tracking-wider mb-sm">
                  Wallet Address
                </p>
                <input
                  type="text"
                  placeholder="0x..."
                  value={wallet}
                  onChange={(e) => setWallet(e.target.value)}
                  className="w-full bg-canvas border border-border rounded-md px-md py-sm text-body text-text-primary font-mono placeholder:text-text-dim focus:outline-none focus:border-accent transition-colors duration-150"
                />
                <p className="text-label text-text-dim mt-xs">
                  The address where you want to receive USDC
                </p>
              </div>

              {state === "error" && errorMsg && (
                <div className="bg-canvas border border-error/30 rounded-md p-md mb-lg">
                  <p className="text-small text-error">{errorMsg}</p>
                </div>
              )}

              <button
                onClick={handleClaim}
                disabled={
                  state === "loading" || !username.trim() || !wallet.trim()
                }
                className="w-full bg-accent text-canvas font-display font-bold text-body-lg py-sm rounded-md hover:opacity-90 transition-opacity duration-150 disabled:opacity-40 disabled:cursor-not-allowed"
              >
                {state === "loading" ? "Claiming..." : "Claim Tips"}
              </button>

              <p className="text-label text-text-dim text-center mt-md">
                {/* TODO: Real OAuth flow will verify GitHub identity */}
                Tips are verified against your GitHub username before release.
              </p>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
