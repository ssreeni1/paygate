/** Internal API client for the PayGate gateway tip endpoints. */

const GATEWAY_URL = process.env.PAYGATE_GATEWAY_URL ?? "http://localhost:8402";
const INTERNAL_SECRET = process.env.PAYGATE_INTERNAL_SECRET ?? "";

export interface TipRecord {
  id: string;
  sender_wallet: string;
  sender_name: string;
  recipient_gh: string;
  package_name: string | null;
  /** Amount in USDC base units (6 decimals). 500000 = $0.50 */
  amount_usdc: number;
  reason: string | null;
  evidence: string | null;
  status: "paid" | "escrowed" | "claimed" | "reclaimed";
  tx_hash: string;
  created_at: string;
}

export interface LeaderboardEntry {
  github_username: string;
  total_usdc: number;
  tip_count: number;
  agent_count: number;
}

export interface ClaimResult {
  claimed_count: number;
  total_usdc: number;
  tx_hash: string | null;
}

function headers(): HeadersInit {
  return {
    Authorization: `Bearer ${INTERNAL_SECRET}`,
    "Content-Type": "application/json",
  };
}

async function fetchInternal<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${GATEWAY_URL}${path}`, {
    ...init,
    headers: { ...headers(), ...init?.headers },
    next: { revalidate: 30 },
  });
  if (!res.ok) {
    throw new Error(`Internal API error: ${res.status} ${res.statusText}`);
  }
  return res.json() as Promise<T>;
}

/** Fetch a single tip by its ID. */
export async function getTip(tipId: string): Promise<TipRecord> {
  return fetchInternal<TipRecord>(`/paygate/internal/tips/${tipId}`);
}

/** Fetch all tips for a GitHub username. */
export async function getTipsByRecipient(
  username: string
): Promise<TipRecord[]> {
  return fetchInternal<TipRecord[]>(
    `/paygate/internal/tips/by-recipient/${username}`
  );
}

/** Fetch the leaderboard. */
export async function getLeaderboard(): Promise<LeaderboardEntry[]> {
  return fetchInternal<LeaderboardEntry[]>(`/paygate/internal/leaderboard`);
}

/** Claim all escrowed tips for a GitHub user. */
export async function claimTips(
  githubUsername: string,
  walletAddress: string
): Promise<ClaimResult> {
  return fetchInternal<ClaimResult>(`/paygate/internal/claim`, {
    method: "POST",
    body: JSON.stringify({
      github_username: githubUsername,
      wallet_address: walletAddress,
    }),
  });
}

// --- Formatting helpers ---

/** Convert USDC base units (6 decimals) to display string like "$0.50". */
export function formatUsdc(baseUnits: number): string {
  const dollars = baseUnits / 1_000_000;
  return `$${dollars.toFixed(2)}`;
}

/** Truncate a hex address: 0x7F3a...Prov */
export function truncateAddress(addr: string | null | undefined): string {
  if (!addr) return "unknown";
  if (addr.length <= 12) return addr;
  return `${addr.slice(0, 6)}...${addr.slice(-4)}`;
}

/** Truncate a tx hash for display. */
export function truncateHash(hash: string | null | undefined): string {
  if (!hash) return "unknown";
  if (hash.length <= 16) return hash;
  return `${hash.slice(0, 10)}...${hash.slice(-6)}`;
}

/** Format ISO date to human-readable. */
export function formatDate(iso: string | null | undefined): string {
  if (!iso) return "unknown";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  return d.toLocaleDateString("en-US", {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}
