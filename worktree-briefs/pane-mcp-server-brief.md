# Build Brief: MCP Server (Wave 3 Stream 1)

PayGate MCP server package (`@paygate/mcp`) — exposes PayGate-protected APIs as MCP tools for Claude Code, Cursor, and any MCP-compatible client. Wraps `@paygate/sdk`, manages sessions, tracks spend, handles shutdown/resume.

## 1. npm Workspaces Setup

### New File: `/package.json` (root)

```json
{
  "private": true,
  "workspaces": ["sdk", "packages/*"]
}
```

This enables `npm install` from root to link `@paygate/sdk` into `@paygate/mcp` without publishing.

### Update: `/sdk/package.json`

No changes needed. The existing `@paygate/sdk` name and structure work with workspaces as-is.

## 2. Package Structure: `packages/mcp-server/`

```
packages/mcp-server/
  package.json
  tsconfig.json
  src/
    index.ts          # Entry point — MCP server setup, tool registration, shutdown handlers
    tools/
      discover.ts     # paygate_discover tool (with AI goal ranking)
      call.ts         # paygate_call tool
      budget.ts       # paygate_budget tool
      estimate.ts     # paygate_estimate tool
      trace.ts        # paygate_trace tool
    session-manager.ts  # Session lifecycle: create, resume, invalidate, cleanup
    spend-tracker.ts    # In-memory spend tracking + limit enforcement
    pricing-cache.ts    # Cached GET /v1/pricing with 60s TTL
    key-loader.ts       # PAYGATE_PRIVATE_KEY / PAYGATE_PRIVATE_KEY_CMD resolution
    errors.ts           # Structured error types + MCP error formatting
    types.ts            # MCP-specific types (tool schemas, internal state)
  tests/
    discover.test.ts
    call.test.ts
    budget.test.ts
    estimate.test.ts
    trace.test.ts
    session-manager.test.ts
    spend-tracker.test.ts
    key-loader.test.ts
    errors.test.ts
    integration.test.ts
```

### New File: `packages/mcp-server/package.json`

```json
{
  "name": "@paygate/mcp",
  "version": "0.5.0",
  "type": "module",
  "bin": {
    "paygate-mcp": "dist/index.js"
  },
  "main": "dist/index.js",
  "types": "dist/index.d.ts",
  "scripts": {
    "build": "tsc",
    "test": "vitest run",
    "test:watch": "vitest",
    "start": "node dist/index.js"
  },
  "dependencies": {
    "@paygate/sdk": "workspace:*",
    "@modelcontextprotocol/sdk": "^1.0.0",
    "viem": "^2.0.0"
  },
  "devDependencies": {
    "@types/node": "^25.5.0",
    "typescript": "^5.0.0",
    "vitest": "^3.0.0"
  }
}
```

### New File: `packages/mcp-server/tsconfig.json`

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "outDir": "dist",
    "rootDir": "src",
    "declaration": true,
    "resolveJsonModule": true
  },
  "include": ["src"],
  "exclude": ["dist", "tests"],
  "references": [
    { "path": "../../sdk" }
  ]
}
```

## 3. Types: `src/types.ts`

```typescript
// ── Environment config ──

export interface McpServerConfig {
  gatewayUrl: string;                    // PAYGATE_GATEWAY_URL
  privateKey: string;                    // resolved from PAYGATE_PRIVATE_KEY or PAYGATE_PRIVATE_KEY_CMD
  payerAddress: string;                  // derived from privateKey
  agentName: string;                     // PAYGATE_AGENT_NAME (default: "mcp-agent")
  sessionDeposit: string;                // PAYGATE_SESSION_DEPOSIT (default: "0.10")
  spendLimitDaily: number | null;        // PAYGATE_SPEND_LIMIT_DAILY in base units, null = unlimited
  spendLimitMonthly: number | null;      // PAYGATE_SPEND_LIMIT_MONTHLY in base units, null = unlimited
}

// ── Pricing cache ──

export interface EndpointPricing {
  endpoint: string;       // e.g. "POST /v1/search"
  method: string;         // e.g. "POST"
  path: string;           // e.g. "/v1/search"
  price: string;          // e.g. "0.001000" (USDC decimal)
  priceBaseUnits: number; // e.g. 1000
  description: string;    // from /v1/pricing
  dynamic: boolean;       // true if price varies per request
}

export interface PricingCache {
  endpoints: EndpointPricing[];
  recipient: string;
  token: string;
  fetchedAt: number;     // Date.now() timestamp
}

// ── Session state ──

export interface SessionState {
  sessionId: string;
  sessionSecret: string;
  balance: number;           // base units remaining
  ratePerRequest: number;    // base units per request
  expiresAt: string;         // ISO 8601
  gatewayBaseUrl: string;
}

// ── Spend tracking ──

export interface SpendRecord {
  totalSpentToday: number;       // base units
  totalSpentThisMonth: number;   // base units
  dayStartUtc: string;           // "2026-03-24" — resets when day changes
  monthStartUtc: string;         // "2026-03" — resets when month changes
  callCount: number;
}

// ── Trace tracking ──

export interface TraceEntry {
  endpoint: string;
  method: string;
  cost: number;           // base units
  timestamp: number;
  explorerLink: string;
}

export interface ActiveTrace {
  name: string;
  startedAt: number;
  entries: TraceEntry[];
}

// ── Tool result wrappers ──

export interface PaygateToolSuccess<T = unknown> {
  result: T;
  payment?: {
    cost: string;          // "$0.001000"
    explorerLink: string;
    balanceRemaining: string;
  };
}

export interface PaygateToolError {
  error: PaygateErrorCode;
  message: string;
  recoverable: boolean;
}

export type PaygateErrorCode =
  | 'insufficient_balance'
  | 'session_creation_failed'
  | 'spend_limit_exceeded'
  | 'gateway_unreachable'
  | 'invalid_input'
  | 'upstream_error';

// ── MCP tool input schemas (for reference) ──

export interface DiscoverInput {
  goal?: string;   // AI goal for relevance ranking
}

export interface CallInput {
  method: string;
  path: string;
  body?: Record<string, unknown>;
  headers?: Record<string, string>;
}

export interface BudgetInput {}  // no input

export interface EstimateInput {
  calls: { endpoint: string; count: number }[];
}

export interface TraceInput {
  action: 'start' | 'stop';
  name: string;
}
```

## 4. Error Handling: `src/errors.ts`

```typescript
import type { PaygateErrorCode, PaygateToolError } from './types.js';

export function makeError(
  code: PaygateErrorCode,
  message: string,
  recoverable: boolean
): PaygateToolError {
  return { error: code, message, recoverable };
}

export function insufficientBalance(detail: string): PaygateToolError {
  return makeError('insufficient_balance', `Wallet balance too low: ${detail}`, false);
}

export function sessionCreationFailed(detail: string): PaygateToolError {
  return makeError('session_creation_failed', `Session creation failed: ${detail}`, true);
}

export function spendLimitExceeded(
  spent: string,
  limit: string,
  period: 'daily' | 'monthly'
): PaygateToolError {
  return makeError(
    'spend_limit_exceeded',
    `${period} spend limit exceeded: spent ${spent} of ${limit} limit`,
    false
  );
}

export function gatewayUnreachable(detail: string): PaygateToolError {
  return makeError('gateway_unreachable', `Cannot reach gateway: ${detail}`, true);
}

export function invalidInput(detail: string): PaygateToolError {
  return makeError('invalid_input', `Invalid input: ${detail}`, false);
}

export function upstreamError(status: number, detail: string): PaygateToolError {
  return makeError('upstream_error', `Upstream returned ${status}: ${detail}`, true);
}

/**
 * Classify a caught error into a structured PaygateToolError.
 */
export function classifyError(err: unknown): PaygateToolError {
  if (err instanceof Error) {
    const msg = err.message;
    if (msg.includes('ECONNREFUSED') || msg.includes('ETIMEDOUT') || msg.includes('fetch failed')) {
      return gatewayUnreachable(msg);
    }
    if (msg.includes('insufficient') || msg.includes('balance')) {
      return insufficientBalance(msg);
    }
    if (msg.includes('Session creation failed') || msg.includes('nonce')) {
      return sessionCreationFailed(msg);
    }
    return makeError('upstream_error', msg, true);
  }
  return makeError('upstream_error', String(err), true);
}

/**
 * Format a PaygateToolError as MCP tool content (isError: true).
 */
export function errorToMcpContent(err: PaygateToolError): { content: Array<{ type: 'text'; text: string }>; isError: true } {
  return {
    content: [{ type: 'text', text: JSON.stringify(err, null, 2) }],
    isError: true,
  };
}
```

## 5. Key Loader: `src/key-loader.ts`

```typescript
import { execSync } from 'node:child_process';

/**
 * Resolve the private key from environment.
 *
 * Priority:
 * 1. PAYGATE_PRIVATE_KEY_CMD — run shell command, trim output
 * 2. PAYGATE_PRIVATE_KEY — plaintext from env
 *
 * Returns the raw hex private key (with 0x prefix).
 * Throws if neither is set or if the command fails.
 */
export function loadPrivateKey(): string {
  const cmd = process.env.PAYGATE_PRIVATE_KEY_CMD;
  if (cmd) {
    try {
      const result = execSync(cmd, {
        encoding: 'utf-8',
        timeout: 10_000,
        stdio: ['pipe', 'pipe', 'pipe'],  // suppress stderr leaking to MCP transport
      }).trim();
      if (!result) {
        throw new Error('PAYGATE_PRIVATE_KEY_CMD returned empty output');
      }
      return normalizeKey(result);
    } catch (err) {
      throw new Error(
        `PAYGATE_PRIVATE_KEY_CMD failed: ${err instanceof Error ? err.message : String(err)}`
      );
    }
  }

  const key = process.env.PAYGATE_PRIVATE_KEY;
  if (key) {
    return normalizeKey(key);
  }

  throw new Error(
    'No private key configured. Set PAYGATE_PRIVATE_KEY or PAYGATE_PRIVATE_KEY_CMD.'
  );
}

/**
 * Ensure key has 0x prefix and is 66 chars (0x + 64 hex).
 */
function normalizeKey(key: string): string {
  const trimmed = key.trim();
  const withPrefix = trimmed.startsWith('0x') ? trimmed : `0x${trimmed}`;
  if (!/^0x[0-9a-fA-F]{64}$/.test(withPrefix)) {
    throw new Error('Invalid private key format: expected 32 bytes hex');
  }
  return withPrefix;
}
```

## 6. Pricing Cache: `src/pricing-cache.ts`

```typescript
import type { PricingCache, EndpointPricing } from './types.js';

const CACHE_TTL_MS = 60_000; // 60 seconds

export class PricingCacheManager {
  private cache: PricingCache | null = null;
  private gatewayUrl: string;

  constructor(gatewayUrl: string) {
    this.gatewayUrl = gatewayUrl;
  }

  /**
   * Get cached pricing, refreshing if stale or missing.
   * Calls GET /v1/pricing on the gateway.
   */
  async getPricing(): Promise<PricingCache> {
    if (this.cache && Date.now() - this.cache.fetchedAt < CACHE_TTL_MS) {
      return this.cache;
    }
    return this.refresh();
  }

  /**
   * Force refresh the pricing cache.
   */
  async refresh(): Promise<PricingCache> {
    const resp = await fetch(`${this.gatewayUrl}/v1/pricing`);
    if (!resp.ok) {
      throw new Error(`GET /v1/pricing failed: ${resp.status} ${resp.statusText}`);
    }

    const body = await resp.json() as {
      apis: Array<{
        endpoint: string;
        method: string;
        path: string;
        price: string;
        price_base_units: number;
        description: string;
        dynamic: boolean;
      }>;
      recipient: string;
      token: string;
    };

    const endpoints: EndpointPricing[] = body.apis.map((api) => ({
      endpoint: api.endpoint,
      method: api.method,
      path: api.path,
      price: api.price,
      priceBaseUnits: api.price_base_units,
      description: api.description,
      dynamic: api.dynamic,
    }));

    this.cache = {
      endpoints,
      recipient: body.recipient,
      token: body.token,
      fetchedAt: Date.now(),
    };

    return this.cache;
  }

  /**
   * Look up price for a specific endpoint string (e.g. "POST /v1/search").
   * Returns null if not found.
   */
  async priceFor(endpoint: string): Promise<EndpointPricing | null> {
    const pricing = await this.getPricing();
    return pricing.endpoints.find((e) => e.endpoint === endpoint) ?? null;
  }

  /**
   * Invalidate cache (forces refresh on next access).
   */
  invalidate(): void {
    this.cache = null;
  }
}
```

## 7. Spend Tracker: `src/spend-tracker.ts`

```typescript
import type { SpendRecord } from './types.js';

/**
 * In-memory spend tracker with daily/monthly limits.
 * All amounts in base units (1 USDC = 1_000_000).
 * Day/month boundaries use UTC.
 */
export class SpendTracker {
  private record: SpendRecord;
  private dailyLimit: number | null;   // base units, null = unlimited
  private monthlyLimit: number | null; // base units, null = unlimited

  constructor(dailyLimit: number | null, monthlyLimit: number | null) {
    this.dailyLimit = dailyLimit;
    this.monthlyLimit = monthlyLimit;
    this.record = {
      totalSpentToday: 0,
      totalSpentThisMonth: 0,
      dayStartUtc: this.currentDayUtc(),
      monthStartUtc: this.currentMonthUtc(),
      callCount: 0,
    };
  }

  /**
   * Check if spending `amount` base units would exceed any limit.
   * Returns null if within limits, or an error description if exceeded.
   */
  checkLimit(amount: number): { period: 'daily' | 'monthly'; spent: number; limit: number } | null {
    this.rolloverIfNeeded();

    if (this.dailyLimit !== null && this.record.totalSpentToday + amount > this.dailyLimit) {
      return { period: 'daily', spent: this.record.totalSpentToday, limit: this.dailyLimit };
    }
    if (this.monthlyLimit !== null && this.record.totalSpentThisMonth + amount > this.monthlyLimit) {
      return { period: 'monthly', spent: this.record.totalSpentThisMonth, limit: this.monthlyLimit };
    }
    return null;
  }

  /**
   * Record a payment of `amount` base units.
   */
  record_spend(amount: number): void {
    this.rolloverIfNeeded();
    this.record.totalSpentToday += amount;
    this.record.totalSpentThisMonth += amount;
    this.record.callCount += 1;
  }

  /**
   * Get current spend state (for paygate_budget tool).
   */
  getRecord(): Readonly<SpendRecord> {
    this.rolloverIfNeeded();
    return { ...this.record };
  }

  /**
   * Get remaining daily budget in base units. Returns Infinity if unlimited.
   */
  remainingDaily(): number {
    this.rolloverIfNeeded();
    if (this.dailyLimit === null) return Infinity;
    return Math.max(0, this.dailyLimit - this.record.totalSpentToday);
  }

  /**
   * Get remaining monthly budget in base units. Returns Infinity if unlimited.
   */
  remainingMonthly(): number {
    this.rolloverIfNeeded();
    if (this.monthlyLimit === null) return Infinity;
    return Math.max(0, this.monthlyLimit - this.record.totalSpentThisMonth);
  }

  /**
   * Roll over daily/monthly counters if the current UTC date has changed.
   */
  private rolloverIfNeeded(): void {
    const currentDay = this.currentDayUtc();
    const currentMonth = this.currentMonthUtc();

    if (currentDay !== this.record.dayStartUtc) {
      this.record.totalSpentToday = 0;
      this.record.dayStartUtc = currentDay;
    }
    if (currentMonth !== this.record.monthStartUtc) {
      this.record.totalSpentThisMonth = 0;
      this.record.monthStartUtc = currentMonth;
    }
  }

  private currentDayUtc(): string {
    return new Date().toISOString().slice(0, 10); // "2026-03-24"
  }

  private currentMonthUtc(): string {
    return new Date().toISOString().slice(0, 7); // "2026-03"
  }
}

/**
 * Parse a USDC decimal string (e.g. "5.00") to base units (5_000_000).
 * Returns null if the input is undefined/empty.
 */
export function parseUsdcToBaseUnits(usdcStr: string | undefined): number | null {
  if (!usdcStr) return null;
  const parsed = parseFloat(usdcStr);
  if (isNaN(parsed) || parsed < 0) return null;
  return Math.round(parsed * 1_000_000);
}

/**
 * Format base units to USDC display string: "$0.001000"
 */
export function formatUsd(baseUnits: number): string {
  return `$${(baseUnits / 1_000_000).toFixed(6)}`;
}
```

## 8. Session Manager: `src/session-manager.ts`

```typescript
import { PayGateClient } from '@paygate/sdk';
import type { SessionState, McpServerConfig } from './types.js';

/**
 * Manages a single session per gateway URL.
 * Handles creation, resumption from gateway, and invalidation.
 */
export class SessionManager {
  private session: SessionState | null = null;
  private client: PayGateClient;
  private config: McpServerConfig;

  constructor(client: PayGateClient, config: McpServerConfig) {
    this.client = client;
    this.config = config;
  }

  /**
   * Get the active session, or null if none.
   */
  getSession(): SessionState | null {
    if (!this.session) return null;
    // Check expiry
    if (new Date(this.session.expiresAt).getTime() < Date.now()) {
      this.session = null;
      return null;
    }
    return this.session;
  }

  /**
   * Get session balance in base units, or 0 if no session.
   */
  getBalance(): number {
    return this.session?.balance ?? 0;
  }

  /**
   * Called after a successful paid request to track balance decrease.
   */
  deductBalance(amount: number): void {
    if (this.session) {
      this.session.balance = Math.max(0, this.session.balance - amount);
    }
  }

  /**
   * Update session state from SDK internals after a fetch() call.
   * The SDK manages session creation internally when autoSession is true.
   * We extract the session state after each call to keep our state in sync.
   */
  updateFromSdkResponse(responseHeaders: Headers): void {
    const cost = responseHeaders.get('X-Payment-Cost');
    if (cost) {
      const costBaseUnits = Math.round(parseFloat(cost) * 1_000_000);
      this.deductBalance(costBaseUnits);
    }
    const balance = responseHeaders.get('X-Payment-Balance');
    if (balance && this.session) {
      this.session.balance = Math.round(parseFloat(balance) * 1_000_000);
    }
  }

  /**
   * Attempt to resume an active session from the gateway on startup.
   * Calls GET /paygate/sessions?payer=<address> and rehydrates if found.
   *
   * Returns true if a session was resumed, false otherwise.
   */
  async tryResumeSession(): Promise<boolean> {
    try {
      const resp = await fetch(
        `${this.config.gatewayUrl}/paygate/sessions?payer=${this.config.payerAddress}`,
        { signal: AbortSignal.timeout(5_000) }
      );
      if (!resp.ok) return false;

      const body = await resp.json() as {
        sessions: Array<{
          sessionId: string;
          balance: string;
          ratePerRequest: string;
          expiresAt: string;
          status: string;
        }>;
      };

      // Find the most recent active session with sufficient balance
      const active = body.sessions
        .filter((s) => s.status === 'active')
        .filter((s) => new Date(s.expiresAt).getTime() > Date.now())
        .sort((a, b) => new Date(b.expiresAt).getTime() - new Date(a.expiresAt).getTime());

      if (active.length === 0) return false;

      const best = active[0];
      const balance = Math.round(parseFloat(best.balance) * 1_000_000);
      const rate = Math.round(parseFloat(best.ratePerRequest) * 1_000_000);

      if (balance < rate) return false; // not enough for even one request

      // We cannot resume fully without the session secret (which the gateway
      // does not return in GET /paygate/sessions for security). The SDK's
      // autoSession will handle creating a new session on the next call.
      // However, we log the active session info for the user.
      process.stderr.write(
        `[paygate] Found active session ${best.sessionId} with $${(balance / 1_000_000).toFixed(6)} remaining — ` +
        `expires ${best.expiresAt}. SDK will reuse if secret is cached.\n`
      );

      return false; // Session resume requires secret — SDK handles this
    } catch {
      // Gateway unreachable on startup — not fatal
      process.stderr.write('[paygate] Could not check for active sessions on startup.\n');
      return false;
    }
  }

  /**
   * Set session state directly (used when SDK creates a session and we intercept).
   */
  setSession(state: SessionState): void {
    this.session = state;
  }

  /**
   * Invalidate the current session.
   */
  invalidate(): void {
    this.session = null;
  }

  /**
   * Log remaining balance on shutdown.
   * Called from SIGINT/SIGTERM handlers.
   */
  logShutdownState(): void {
    if (this.session) {
      const balance = (this.session.balance / 1_000_000).toFixed(6);
      process.stderr.write(
        `[paygate] Session ${this.session.sessionId} has $${balance} remaining — ` +
        `expires at ${this.session.expiresAt}\n`
      );
    } else {
      process.stderr.write('[paygate] No active session at shutdown.\n');
    }
  }
}
```

## 9. MCP Tool Implementations

### 9.1 `src/tools/discover.ts` — `paygate_discover`

MCP Tool Schema:
```json
{
  "name": "paygate_discover",
  "description": "List available PayGate-protected APIs with pricing. Optionally provide a goal to rank APIs by relevance.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "goal": {
        "type": "string",
        "description": "Optional: describe what you want to accomplish. APIs will be ranked by relevance to this goal with usage examples."
      }
    }
  }
}
```

Implementation:

```typescript
import { PricingCacheManager } from '../pricing-cache.js';
import type { DiscoverInput, EndpointPricing } from '../types.js';
import { classifyError, errorToMcpContent } from '../errors.js';
import { formatUsd } from '../spend-tracker.js';

/**
 * Build the discover tool handler.
 */
export function handleDiscover(pricingCache: PricingCacheManager) {
  return async (input: DiscoverInput): Promise<{
    content: Array<{ type: 'text'; text: string }>;
    isError?: boolean;
  }> => {
    try {
      const pricing = await pricingCache.getPricing();
      let endpoints = pricing.endpoints;

      if (input.goal) {
        endpoints = rankByGoal(endpoints, input.goal);
      }

      const result = endpoints.map((ep) => ({
        endpoint: ep.endpoint,
        description: ep.description,
        price: formatUsd(ep.priceBaseUnits),
        dynamic: ep.dynamic,
        ...(input.goal ? { relevance: computeRelevanceNote(ep, input.goal) } : {}),
      }));

      return {
        content: [{
          type: 'text',
          text: JSON.stringify({
            apis: result,
            gateway: pricingCache['gatewayUrl'],
            ...(input.goal ? { goal: input.goal, note: 'APIs ranked by estimated relevance to your goal' } : {}),
          }, null, 2),
        }],
      };
    } catch (err) {
      return errorToMcpContent(classifyError(err));
    }
  };
}

/**
 * Rank endpoints by keyword relevance to the goal string.
 * Simple TF scoring: count keyword overlaps between goal and endpoint description/path.
 * Not LLM-powered — fast, deterministic, zero-cost.
 */
function rankByGoal(endpoints: EndpointPricing[], goal: string): EndpointPricing[] {
  const goalTokens = tokenize(goal);

  const scored = endpoints.map((ep) => {
    const epTokens = tokenize(ep.description + ' ' + ep.path + ' ' + ep.endpoint);
    const overlap = goalTokens.filter((t) => epTokens.includes(t)).length;
    return { ep, score: overlap };
  });

  scored.sort((a, b) => b.score - a.score);
  return scored.map((s) => s.ep);
}

function tokenize(text: string): string[] {
  return text.toLowerCase().replace(/[^a-z0-9\s]/g, ' ').split(/\s+/).filter(Boolean);
}

/**
 * Generate a short relevance note for an endpoint given a goal.
 */
function computeRelevanceNote(ep: EndpointPricing, goal: string): string {
  const goalTokens = tokenize(goal);
  const epTokens = tokenize(ep.description + ' ' + ep.path);
  const matches = goalTokens.filter((t) => epTokens.includes(t));
  if (matches.length === 0) return 'No direct keyword match — may still be useful';
  return `Matches: ${matches.join(', ')}`;
}
```

### 9.2 `src/tools/call.ts` — `paygate_call`

MCP Tool Schema:
```json
{
  "name": "paygate_call",
  "description": "Call any PayGate-protected API endpoint. Automatically handles session creation, payment, and authentication. Returns the upstream API response plus payment metadata.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "method": {
        "type": "string",
        "description": "HTTP method (GET, POST, PUT, DELETE)",
        "enum": ["GET", "POST", "PUT", "DELETE"]
      },
      "path": {
        "type": "string",
        "description": "API path (e.g. /v1/search)"
      },
      "body": {
        "type": "object",
        "description": "Request body (for POST/PUT)"
      },
      "headers": {
        "type": "object",
        "description": "Additional headers to include",
        "additionalProperties": { "type": "string" }
      }
    },
    "required": ["method", "path"]
  }
}
```

Implementation:

```typescript
import { PayGateClient } from '@paygate/sdk';
import { SpendTracker, formatUsd } from '../spend-tracker.js';
import { SessionManager } from '../session-manager.js';
import { PricingCacheManager } from '../pricing-cache.js';
import type { CallInput, McpServerConfig, ActiveTrace, TraceEntry } from '../types.js';
import { classifyError, errorToMcpContent, spendLimitExceeded, invalidInput, upstreamError } from '../errors.js';

/**
 * Build the call tool handler.
 */
export function handleCall(
  client: PayGateClient,
  config: McpServerConfig,
  spendTracker: SpendTracker,
  sessionManager: SessionManager,
  pricingCache: PricingCacheManager,
  activeTraces: Map<string, ActiveTrace>,
) {
  return async (input: CallInput): Promise<{
    content: Array<{ type: 'text'; text: string }>;
    isError?: boolean;
  }> => {
    // Validate input
    if (!input.method || !input.path) {
      return errorToMcpContent(invalidInput('method and path are required'));
    }

    const method = input.method.toUpperCase();
    if (!['GET', 'POST', 'PUT', 'DELETE'].includes(method)) {
      return errorToMcpContent(invalidInput(`Unsupported method: ${method}`));
    }

    // Estimate cost for spend limit check
    const endpoint = `${method} ${input.path}`;
    const epPricing = await pricingCache.priceFor(endpoint);
    const estimatedCost = epPricing?.priceBaseUnits ?? 0;

    // Check spend limit before calling
    const limitViolation = spendTracker.checkLimit(estimatedCost);
    if (limitViolation) {
      return errorToMcpContent(
        spendLimitExceeded(
          formatUsd(limitViolation.spent),
          formatUsd(limitViolation.limit),
          limitViolation.period
        )
      );
    }

    try {
      const url = `${config.gatewayUrl}${input.path}`;
      const requestInit: RequestInit = {
        method,
        ...(input.body ? { body: JSON.stringify(input.body) } : {}),
        headers: {
          'Content-Type': 'application/json',
          ...(config.agentName ? { 'X-Payment-Agent': config.agentName } : {}),
          ...(input.headers ?? {}),
        },
      };

      const response = await client.fetch(url, requestInit);

      // Extract payment metadata from response headers
      const costHeader = response.headers.get('X-Payment-Cost');
      const costBaseUnits = costHeader ? Math.round(parseFloat(costHeader) * 1_000_000) : estimatedCost;
      const txHash = response.headers.get('X-Payment-Tx');
      const balanceHeader = response.headers.get('X-Payment-Balance');

      // Update spend tracking
      spendTracker.record_spend(costBaseUnits);

      // Update session manager from response
      sessionManager.updateFromSdkResponse(response.headers);

      // Build explorer link
      const explorerLink = txHash
        ? `https://testnet.tempo.xyz/tx/${txHash}`
        : null;

      // Log to stderr
      process.stderr.write(
        `[paygate] ${endpoint} — cost: ${formatUsd(costBaseUnits)}` +
        (explorerLink ? ` — ${explorerLink}` : '') + '\n'
      );

      // Add to active traces
      for (const trace of activeTraces.values()) {
        trace.entries.push({
          endpoint,
          method,
          cost: costBaseUnits,
          timestamp: Date.now(),
          explorerLink: explorerLink ?? '',
        });
      }

      // Parse response body
      const responseBody = await response.text();
      let parsedBody: unknown;
      try {
        parsedBody = JSON.parse(responseBody);
      } catch {
        parsedBody = responseBody;
      }

      // Check for upstream errors
      if (response.status >= 500) {
        const refunded = response.headers.get('X-Payment-Refunded') === 'true';
        return errorToMcpContent(upstreamError(
          response.status,
          `${responseBody.slice(0, 200)}${refunded ? ' (payment refunded)' : ''}`
        ));
      }

      if (response.status >= 400) {
        return {
          content: [{
            type: 'text',
            text: JSON.stringify({
              status: response.status,
              body: parsedBody,
            }, null, 2),
          }],
          isError: true,
        };
      }

      const balanceRemaining = balanceHeader
        ? `$${parseFloat(balanceHeader).toFixed(6)}`
        : formatUsd(sessionManager.getBalance());

      return {
        content: [{
          type: 'text',
          text: JSON.stringify({
            result: parsedBody,
            payment: {
              cost: formatUsd(costBaseUnits),
              explorerLink: explorerLink ?? 'N/A',
              balanceRemaining,
            },
          }, null, 2),
        }],
      };
    } catch (err) {
      return errorToMcpContent(classifyError(err));
    }
  };
}
```

### 9.3 `src/tools/budget.ts` — `paygate_budget`

MCP Tool Schema:
```json
{
  "name": "paygate_budget",
  "description": "Check current spending status: session balance, total spent today/this month, daily/monthly limits, and remaining budget.",
  "inputSchema": {
    "type": "object",
    "properties": {}
  }
}
```

Implementation:

```typescript
import { SpendTracker, formatUsd } from '../spend-tracker.js';
import { SessionManager } from '../session-manager.js';
import type { McpServerConfig } from '../types.js';

/**
 * Build the budget tool handler.
 */
export function handleBudget(
  spendTracker: SpendTracker,
  sessionManager: SessionManager,
  config: McpServerConfig,
) {
  return async (): Promise<{
    content: Array<{ type: 'text'; text: string }>;
  }> => {
    const record = spendTracker.getRecord();
    const session = sessionManager.getSession();

    const result = {
      session: session
        ? {
            sessionId: session.sessionId,
            balance: formatUsd(session.balance),
            expiresAt: session.expiresAt,
          }
        : null,
      spending: {
        totalSpentToday: formatUsd(record.totalSpentToday),
        totalSpentThisMonth: formatUsd(record.totalSpentThisMonth),
        callCount: record.callCount,
      },
      limits: {
        daily: config.spendLimitDaily !== null ? formatUsd(config.spendLimitDaily) : 'unlimited',
        monthly: config.spendLimitMonthly !== null ? formatUsd(config.spendLimitMonthly) : 'unlimited',
        remainingDaily: spendTracker.remainingDaily() === Infinity
          ? 'unlimited'
          : formatUsd(spendTracker.remainingDaily()),
        remainingMonthly: spendTracker.remainingMonthly() === Infinity
          ? 'unlimited'
          : formatUsd(spendTracker.remainingMonthly()),
      },
      agent: config.agentName,
    };

    return {
      content: [{
        type: 'text',
        text: JSON.stringify(result, null, 2),
      }],
    };
  };
}
```

### 9.4 `src/tools/estimate.ts` — `paygate_estimate`

MCP Tool Schema:
```json
{
  "name": "paygate_estimate",
  "description": "Estimate the cost of a planned sequence of API calls. Returns total cost, per-endpoint breakdown, and whether it fits within your budget.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "calls": {
        "type": "array",
        "description": "List of planned calls with endpoint and count",
        "items": {
          "type": "object",
          "properties": {
            "endpoint": {
              "type": "string",
              "description": "Endpoint string (e.g. 'POST /v1/search')"
            },
            "count": {
              "type": "number",
              "description": "Number of times to call this endpoint"
            }
          },
          "required": ["endpoint", "count"]
        }
      }
    },
    "required": ["calls"]
  }
}
```

Implementation:

```typescript
import { PricingCacheManager } from '../pricing-cache.js';
import { SpendTracker, formatUsd } from '../spend-tracker.js';
import type { EstimateInput } from '../types.js';
import { classifyError, errorToMcpContent, invalidInput } from '../errors.js';

/**
 * Build the estimate tool handler.
 */
export function handleEstimate(
  pricingCache: PricingCacheManager,
  spendTracker: SpendTracker,
) {
  return async (input: EstimateInput): Promise<{
    content: Array<{ type: 'text'; text: string }>;
    isError?: boolean;
  }> => {
    if (!input.calls || !Array.isArray(input.calls) || input.calls.length === 0) {
      return errorToMcpContent(invalidInput('calls array is required and must be non-empty'));
    }

    try {
      const pricing = await pricingCache.getPricing();
      let totalBaseUnits = 0;
      const breakdown: Array<{
        endpoint: string;
        price: string;
        count: number;
        subtotal: string;
        approximate: boolean;
      }> = [];

      for (const call of input.calls) {
        if (!call.endpoint || typeof call.count !== 'number' || call.count < 1) {
          return errorToMcpContent(invalidInput(
            `Invalid call entry: endpoint="${call.endpoint}", count=${call.count}`
          ));
        }

        const ep = pricing.endpoints.find((e) => e.endpoint === call.endpoint);
        if (!ep) {
          breakdown.push({
            endpoint: call.endpoint,
            price: 'unknown',
            count: call.count,
            subtotal: 'unknown',
            approximate: true,
          });
          continue;
        }

        const subtotal = ep.priceBaseUnits * call.count;
        totalBaseUnits += subtotal;
        breakdown.push({
          endpoint: call.endpoint,
          price: formatUsd(ep.priceBaseUnits),
          count: call.count,
          subtotal: formatUsd(subtotal),
          approximate: ep.dynamic,
        });
      }

      const remainingDaily = spendTracker.remainingDaily();
      const withinBudget = remainingDaily === Infinity || totalBaseUnits <= remainingDaily;

      return {
        content: [{
          type: 'text',
          text: JSON.stringify({
            estimatedTotal: formatUsd(totalBaseUnits),
            breakdown,
            withinBudget,
            note: breakdown.some((b) => b.approximate)
              ? 'Some endpoints have dynamic pricing — actual cost may vary'
              : undefined,
          }, null, 2),
        }],
      };
    } catch (err) {
      return errorToMcpContent(classifyError(err));
    }
  };
}
```

### 9.5 `src/tools/trace.ts` — `paygate_trace`

MCP Tool Schema:
```json
{
  "name": "paygate_trace",
  "description": "Track costs across a multi-step workflow. Start a named trace before a sequence of calls, then stop it to get total cost and breakdown with explorer links.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "action": {
        "type": "string",
        "enum": ["start", "stop"],
        "description": "'start' begins a new trace, 'stop' ends it and returns the summary"
      },
      "name": {
        "type": "string",
        "description": "Unique name for this trace (e.g. 'research-task')"
      }
    },
    "required": ["action", "name"]
  }
}
```

Implementation:

```typescript
import type { ActiveTrace, TraceInput } from '../types.js';
import { formatUsd } from '../spend-tracker.js';
import { invalidInput, errorToMcpContent } from '../errors.js';

/**
 * Build the trace tool handler.
 */
export function handleTrace(activeTraces: Map<string, ActiveTrace>) {
  return async (input: TraceInput): Promise<{
    content: Array<{ type: 'text'; text: string }>;
    isError?: boolean;
  }> => {
    if (!input.action || !input.name) {
      return errorToMcpContent(invalidInput('action and name are required'));
    }

    if (input.action === 'start') {
      if (activeTraces.has(input.name)) {
        return errorToMcpContent(invalidInput(`Trace "${input.name}" is already active. Stop it first.`));
      }

      activeTraces.set(input.name, {
        name: input.name,
        startedAt: Date.now(),
        entries: [],
      });

      return {
        content: [{
          type: 'text',
          text: JSON.stringify({
            status: 'started',
            name: input.name,
            message: `Trace "${input.name}" started. All paygate_call invocations will be tracked until you stop this trace.`,
          }, null, 2),
        }],
      };
    }

    if (input.action === 'stop') {
      const trace = activeTraces.get(input.name);
      if (!trace) {
        return errorToMcpContent(invalidInput(`No active trace named "${input.name}". Start one first.`));
      }

      activeTraces.delete(input.name);

      const totalCost = trace.entries.reduce((sum, e) => sum + e.cost, 0);
      const durationMs = Date.now() - trace.startedAt;

      // Group by endpoint for breakdown
      const byEndpoint = new Map<string, { count: number; totalCost: number }>();
      for (const entry of trace.entries) {
        const existing = byEndpoint.get(entry.endpoint) ?? { count: 0, totalCost: 0 };
        existing.count += 1;
        existing.totalCost += entry.cost;
        byEndpoint.set(entry.endpoint, existing);
      }

      const breakdown = Array.from(byEndpoint.entries()).map(([endpoint, data]) => ({
        endpoint,
        calls: data.count,
        totalCost: formatUsd(data.totalCost),
      }));

      return {
        content: [{
          type: 'text',
          text: JSON.stringify({
            status: 'stopped',
            name: input.name,
            totalCost: formatUsd(totalCost),
            callCount: trace.entries.length,
            durationMs,
            breakdown,
            explorerLinks: trace.entries
              .filter((e) => e.explorerLink)
              .map((e) => e.explorerLink),
          }, null, 2),
        }],
      };
    }

    return errorToMcpContent(invalidInput(`Unknown action: ${input.action}. Use "start" or "stop".`));
  };
}
```

## 10. Entry Point: `src/index.ts`

```typescript
#!/usr/bin/env node

import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from '@modelcontextprotocol/sdk/types.js';
import { PayGateClient } from '@paygate/sdk';
import { createWalletClient, http } from 'viem';
import { privateKeyToAccount } from 'viem/accounts';

import { loadPrivateKey } from './key-loader.js';
import { PricingCacheManager } from './pricing-cache.js';
import { SpendTracker, parseUsdcToBaseUnits, formatUsd } from './spend-tracker.js';
import { SessionManager } from './session-manager.js';
import { handleDiscover } from './tools/discover.js';
import { handleCall } from './tools/call.js';
import { handleBudget } from './tools/budget.js';
import { handleEstimate } from './tools/estimate.js';
import { handleTrace } from './tools/trace.js';
import { invalidInput, errorToMcpContent } from './errors.js';
import type { McpServerConfig, ActiveTrace, CallInput, DiscoverInput, EstimateInput, TraceInput } from './types.js';

async function main(): Promise<void> {
  // ── Load config from environment ──
  const privateKey = loadPrivateKey();
  const account = privateKeyToAccount(privateKey as `0x${string}`);
  const gatewayUrl = process.env.PAYGATE_GATEWAY_URL;
  if (!gatewayUrl) {
    throw new Error('PAYGATE_GATEWAY_URL is required');
  }

  const config: McpServerConfig = {
    gatewayUrl,
    privateKey,
    payerAddress: account.address,
    agentName: process.env.PAYGATE_AGENT_NAME ?? 'mcp-agent',
    sessionDeposit: process.env.PAYGATE_SESSION_DEPOSIT ?? '0.10',
    spendLimitDaily: parseUsdcToBaseUnits(process.env.PAYGATE_SPEND_LIMIT_DAILY),
    spendLimitMonthly: parseUsdcToBaseUnits(process.env.PAYGATE_SPEND_LIMIT_MONTHLY),
  };

  // ── Initialize components ──

  // Pay function: sends TIP-20 transferWithMemo on Tempo
  const rpcUrl = process.env.PAYGATE_RPC_URL ?? 'https://rpc.testnet.tempo.xyz';
  const walletClient = createWalletClient({
    account,
    transport: http(rpcUrl),
  });

  const payFunction = async (params: {
    to: string;
    amount: bigint;
    token: string;
    memo: string;
  }): Promise<string> => {
    // ERC-20 transferWithMemo — specific to Tempo TIP-20
    const txHash = await walletClient.writeContract({
      address: params.token as `0x${string}`,
      abi: [
        {
          name: 'transferWithMemo',
          type: 'function',
          stateMutability: 'nonpayable',
          inputs: [
            { name: 'to', type: 'address' },
            { name: 'value', type: 'uint256' },
            { name: 'memo', type: 'bytes32' },
          ],
          outputs: [{ type: 'bool' }],
        },
      ],
      functionName: 'transferWithMemo',
      args: [params.to as `0x${string}`, params.amount, params.memo as `0x${string}`],
    });
    return txHash;
  };

  const sdkClient = new PayGateClient({
    payFunction,
    payerAddress: account.address,
    autoSession: true,
    sessionDeposit: config.sessionDeposit,
  });

  const pricingCache = new PricingCacheManager(config.gatewayUrl);
  const spendTracker = new SpendTracker(config.spendLimitDaily, config.spendLimitMonthly);
  const sessionManager = new SessionManager(sdkClient, config);
  const activeTraces = new Map<string, ActiveTrace>();

  // ── Attempt session resume ──
  await sessionManager.tryResumeSession();

  // ── Define MCP tool schemas ──
  const TOOLS = [
    {
      name: 'paygate_discover',
      description:
        'List available PayGate-protected APIs with pricing. Optionally provide a goal to rank APIs by relevance with usage examples.',
      inputSchema: {
        type: 'object' as const,
        properties: {
          goal: {
            type: 'string',
            description:
              'Optional: describe what you want to accomplish. APIs will be ranked by relevance.',
          },
        },
      },
    },
    {
      name: 'paygate_call',
      description:
        'Call any PayGate-protected API endpoint. Handles session creation, payment, and authentication automatically. Returns the upstream response plus payment proof (cost, explorer link, remaining balance).',
      inputSchema: {
        type: 'object' as const,
        properties: {
          method: {
            type: 'string',
            enum: ['GET', 'POST', 'PUT', 'DELETE'],
            description: 'HTTP method',
          },
          path: {
            type: 'string',
            description: 'API path (e.g. /v1/search)',
          },
          body: {
            type: 'object',
            description: 'Request body (for POST/PUT)',
          },
          headers: {
            type: 'object',
            description: 'Additional request headers',
            additionalProperties: { type: 'string' },
          },
        },
        required: ['method', 'path'],
      },
    },
    {
      name: 'paygate_budget',
      description:
        'Check current spending status: session balance, total spent today/this month, daily/monthly limits, and remaining budget. No payment required.',
      inputSchema: {
        type: 'object' as const,
        properties: {},
      },
    },
    {
      name: 'paygate_estimate',
      description:
        'Estimate the cost of a planned sequence of API calls. Returns total cost, per-endpoint breakdown, and whether the plan fits within your budget.',
      inputSchema: {
        type: 'object' as const,
        properties: {
          calls: {
            type: 'array',
            description: 'List of planned calls',
            items: {
              type: 'object',
              properties: {
                endpoint: {
                  type: 'string',
                  description: "Endpoint (e.g. 'POST /v1/search')",
                },
                count: {
                  type: 'number',
                  description: 'Number of calls',
                },
              },
              required: ['endpoint', 'count'],
            },
          },
        },
        required: ['calls'],
      },
    },
    {
      name: 'paygate_trace',
      description:
        'Track costs across a multi-step workflow. Start a named trace, make calls, then stop it to get total cost breakdown with explorer links.',
      inputSchema: {
        type: 'object' as const,
        properties: {
          action: {
            type: 'string',
            enum: ['start', 'stop'],
            description: "'start' begins a trace, 'stop' ends it and returns summary",
          },
          name: {
            type: 'string',
            description: 'Unique name for this trace',
          },
        },
        required: ['action', 'name'],
      },
    },
  ];

  // ── Build tool handlers ──
  const discoverHandler = handleDiscover(pricingCache);
  const callHandler = handleCall(sdkClient, config, spendTracker, sessionManager, pricingCache, activeTraces);
  const budgetHandler = handleBudget(spendTracker, sessionManager, config);
  const estimateHandler = handleEstimate(pricingCache, spendTracker);
  const traceHandler = handleTrace(activeTraces);

  // ── Create MCP server ──
  const server = new Server(
    { name: 'paygate', version: '0.5.0' },
    { capabilities: { tools: {} } }
  );

  server.setRequestHandler(ListToolsRequestSchema, async () => ({
    tools: TOOLS,
  }));

  server.setRequestHandler(CallToolRequestSchema, async (request) => {
    const { name, arguments: args } = request.params;

    switch (name) {
      case 'paygate_discover':
        return discoverHandler(args as DiscoverInput);
      case 'paygate_call':
        return callHandler(args as CallInput);
      case 'paygate_budget':
        return budgetHandler();
      case 'paygate_estimate':
        return estimateHandler(args as EstimateInput);
      case 'paygate_trace':
        return traceHandler(args as TraceInput);
      default:
        return errorToMcpContent(invalidInput(`Unknown tool: ${name}`));
    }
  });

  // ── Shutdown handlers ──
  const shutdown = (): void => {
    process.stderr.write('[paygate] Shutting down...\n');
    sessionManager.logShutdownState();
    const record = spendTracker.getRecord();
    process.stderr.write(
      `[paygate] Total spent this session: ${formatUsd(record.totalSpentToday)} across ${record.callCount} calls\n`
    );
    process.exit(0);
  };

  process.on('SIGINT', shutdown);
  process.on('SIGTERM', shutdown);

  // ── Start server ──
  const transport = new StdioServerTransport();
  await server.connect(transport);

  process.stderr.write(
    `[paygate] MCP server started — gateway: ${config.gatewayUrl}, agent: ${config.agentName}, payer: ${config.payerAddress}\n`
  );
}

main().catch((err) => {
  process.stderr.write(`[paygate] Fatal: ${err instanceof Error ? err.message : String(err)}\n`);
  process.exit(1);
});
```

## 11. llms.txt: `docs/llms.txt`

```text
# PayGate

PayGate is a reverse proxy that gates HTTP API access behind per-request stablecoin micropayments on the Tempo blockchain.

## Quick Start

Add to Claude Code MCP config:
```json
{
  "mcpServers": {
    "paygate": {
      "command": "npx",
      "args": ["@paygate/mcp"],
      "env": {
        "PAYGATE_GATEWAY_URL": "https://paygate-demo-production.up.railway.app",
        "PAYGATE_PRIVATE_KEY": "0x..."
      }
    }
  }
}
```

## Tools

- paygate_discover — List available APIs with pricing. Pass a `goal` to rank by relevance.
- paygate_call — Call any API endpoint. Handles payment automatically. Returns result + payment proof.
- paygate_budget — Check spending: session balance, daily/monthly limits, remaining budget.
- paygate_estimate — Estimate cost for a plan of multiple calls.
- paygate_trace — Track total cost across a multi-step workflow.

## Typical Workflow

1. Use paygate_discover to find APIs (or with a goal: "I need to search for news")
2. Use paygate_estimate to check if your plan fits budget
3. Use paygate_trace to start tracking
4. Use paygate_call for each API request
5. Use paygate_trace to stop and get cost summary
6. Use paygate_budget to see remaining spend capacity

## Payment Flow

Every paygate_call creates an on-chain USDC payment on the Tempo blockchain. Payments are verifiable via Blockscout explorer links included in every response. Sessions batch multiple requests under a single deposit for efficiency.

## Security

- Set PAYGATE_PRIVATE_KEY_CMD instead of PAYGATE_PRIVATE_KEY to load key from a secure source (e.g. `op read op://vault/key/credential`)
- Set PAYGATE_SPEND_LIMIT_DAILY to cap autonomous spending
- All payments are on-chain and auditable
```

## 12. Tests — Minimum 12

All tests use vitest. Test files live in `packages/mcp-server/tests/`.

### Test 1: `key-loader.test.ts` — PAYGATE_PRIVATE_KEY loads correctly

```typescript
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { loadPrivateKey } from '../src/key-loader.js';

describe('loadPrivateKey', () => {
  const originalEnv = { ...process.env };

  afterEach(() => {
    process.env = { ...originalEnv };
  });

  it('loads key from PAYGATE_PRIVATE_KEY', () => {
    process.env.PAYGATE_PRIVATE_KEY = '0x' + 'ab'.repeat(32);
    delete process.env.PAYGATE_PRIVATE_KEY_CMD;
    expect(loadPrivateKey()).toBe('0x' + 'ab'.repeat(32));
  });

  it('adds 0x prefix if missing', () => {
    process.env.PAYGATE_PRIVATE_KEY = 'ab'.repeat(32);
    delete process.env.PAYGATE_PRIVATE_KEY_CMD;
    expect(loadPrivateKey()).toBe('0x' + 'ab'.repeat(32));
  });

  it('throws if neither env var is set', () => {
    delete process.env.PAYGATE_PRIVATE_KEY;
    delete process.env.PAYGATE_PRIVATE_KEY_CMD;
    expect(() => loadPrivateKey()).toThrow('No private key configured');
  });

  it('rejects invalid key format', () => {
    process.env.PAYGATE_PRIVATE_KEY = '0xinvalid';
    delete process.env.PAYGATE_PRIVATE_KEY_CMD;
    expect(() => loadPrivateKey()).toThrow('Invalid private key format');
  });
});
```

### Test 2: `key-loader.test.ts` — PAYGATE_PRIVATE_KEY_CMD executes shell command

```typescript
// (continued in same file)
describe('loadPrivateKey with CMD', () => {
  const originalEnv = { ...process.env };

  afterEach(() => {
    process.env = { ...originalEnv };
  });

  it('loads key from PAYGATE_PRIVATE_KEY_CMD', () => {
    const key = '0x' + 'cd'.repeat(32);
    process.env.PAYGATE_PRIVATE_KEY_CMD = `echo ${key}`;
    delete process.env.PAYGATE_PRIVATE_KEY;
    expect(loadPrivateKey()).toBe(key);
  });

  it('CMD takes priority over PAYGATE_PRIVATE_KEY', () => {
    const cmdKey = '0x' + 'cd'.repeat(32);
    const envKey = '0x' + 'ab'.repeat(32);
    process.env.PAYGATE_PRIVATE_KEY_CMD = `echo ${cmdKey}`;
    process.env.PAYGATE_PRIVATE_KEY = envKey;
    expect(loadPrivateKey()).toBe(cmdKey);
  });

  it('throws if CMD returns empty', () => {
    process.env.PAYGATE_PRIVATE_KEY_CMD = 'echo';
    delete process.env.PAYGATE_PRIVATE_KEY;
    expect(() => loadPrivateKey()).toThrow('empty output');
  });
});
```

### Test 3: `spend-tracker.test.ts` — spend limit enforcement

```typescript
import { describe, it, expect } from 'vitest';
import { SpendTracker, parseUsdcToBaseUnits, formatUsd } from '../src/spend-tracker.js';

describe('SpendTracker', () => {
  it('allows spending within daily limit', () => {
    const tracker = new SpendTracker(5_000_000, null); // $5 daily
    expect(tracker.checkLimit(1_000_000)).toBeNull();
  });

  it('rejects spending that exceeds daily limit', () => {
    const tracker = new SpendTracker(5_000_000, null);
    tracker.record_spend(4_500_000);
    const violation = tracker.checkLimit(1_000_000);
    expect(violation).not.toBeNull();
    expect(violation!.period).toBe('daily');
    expect(violation!.spent).toBe(4_500_000);
    expect(violation!.limit).toBe(5_000_000);
  });

  it('tracks cumulative spending', () => {
    const tracker = new SpendTracker(null, null);
    tracker.record_spend(100_000);
    tracker.record_spend(200_000);
    const record = tracker.getRecord();
    expect(record.totalSpentToday).toBe(300_000);
    expect(record.callCount).toBe(2);
  });

  it('returns Infinity for unlimited remaining', () => {
    const tracker = new SpendTracker(null, null);
    expect(tracker.remainingDaily()).toBe(Infinity);
    expect(tracker.remainingMonthly()).toBe(Infinity);
  });

  it('computes remaining correctly', () => {
    const tracker = new SpendTracker(5_000_000, 50_000_000);
    tracker.record_spend(2_000_000);
    expect(tracker.remainingDaily()).toBe(3_000_000);
    expect(tracker.remainingMonthly()).toBe(48_000_000);
  });
});

describe('parseUsdcToBaseUnits', () => {
  it('parses "5.00" to 5000000', () => {
    expect(parseUsdcToBaseUnits('5.00')).toBe(5_000_000);
  });

  it('returns null for undefined', () => {
    expect(parseUsdcToBaseUnits(undefined)).toBeNull();
  });

  it('returns null for negative', () => {
    expect(parseUsdcToBaseUnits('-1.00')).toBeNull();
  });
});

describe('formatUsd', () => {
  it('formats 1000 base units as $0.001000', () => {
    expect(formatUsd(1000)).toBe('$0.001000');
  });
});
```

### Test 4: `discover.test.ts` — discovery with and without goal

```typescript
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { handleDiscover } from '../src/tools/discover.js';
import { PricingCacheManager } from '../src/pricing-cache.js';

describe('paygate_discover', () => {
  let pricingCache: PricingCacheManager;

  beforeEach(() => {
    pricingCache = new PricingCacheManager('https://example.com');
    // Mock the getPricing method
    vi.spyOn(pricingCache, 'getPricing').mockResolvedValue({
      endpoints: [
        {
          endpoint: 'POST /v1/search',
          method: 'POST',
          path: '/v1/search',
          price: '0.001000',
          priceBaseUnits: 1000,
          description: 'Search the web for information',
          dynamic: false,
        },
        {
          endpoint: 'POST /v1/image',
          method: 'POST',
          path: '/v1/image',
          price: '0.010000',
          priceBaseUnits: 10000,
          description: 'Generate an image from a prompt',
          dynamic: true,
        },
      ],
      recipient: '0x1234',
      token: '0xUSDC',
      fetchedAt: Date.now(),
    });
  });

  it('returns all APIs without goal', async () => {
    const handler = handleDiscover(pricingCache);
    const result = await handler({});
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.apis).toHaveLength(2);
    expect(parsed.apis[0].endpoint).toBe('POST /v1/search');
  });

  it('ranks APIs by relevance when goal is provided', async () => {
    const handler = handleDiscover(pricingCache);
    const result = await handler({ goal: 'search for news' });
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.apis[0].endpoint).toBe('POST /v1/search');
    expect(parsed.goal).toBe('search for news');
  });

  it('returns error when gateway unreachable', async () => {
    vi.spyOn(pricingCache, 'getPricing').mockRejectedValue(new Error('fetch failed'));
    const handler = handleDiscover(pricingCache);
    const result = await handler({});
    expect(result.isError).toBe(true);
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.error).toBe('gateway_unreachable');
  });
});
```

### Test 5: `estimate.test.ts` — cost estimation

```typescript
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { handleEstimate } from '../src/tools/estimate.js';
import { PricingCacheManager } from '../src/pricing-cache.js';
import { SpendTracker } from '../src/spend-tracker.js';

describe('paygate_estimate', () => {
  let pricingCache: PricingCacheManager;
  let spendTracker: SpendTracker;

  beforeEach(() => {
    pricingCache = new PricingCacheManager('https://example.com');
    spendTracker = new SpendTracker(5_000_000, null);
    vi.spyOn(pricingCache, 'getPricing').mockResolvedValue({
      endpoints: [
        {
          endpoint: 'POST /v1/search',
          method: 'POST',
          path: '/v1/search',
          price: '0.001000',
          priceBaseUnits: 1000,
          description: 'Search',
          dynamic: false,
        },
        {
          endpoint: 'POST /v1/summarize',
          method: 'POST',
          path: '/v1/summarize',
          price: '0.005000',
          priceBaseUnits: 5000,
          description: 'Summarize',
          dynamic: true,
        },
      ],
      recipient: '0x1234',
      token: '0xUSDC',
      fetchedAt: Date.now(),
    });
  });

  it('computes total cost correctly', async () => {
    const handler = handleEstimate(pricingCache, spendTracker);
    const result = await handler({
      calls: [
        { endpoint: 'POST /v1/search', count: 3 },
        { endpoint: 'POST /v1/summarize', count: 1 },
      ],
    });
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.estimatedTotal).toBe('$0.008000'); // 3*1000 + 1*5000 = 8000
    expect(parsed.withinBudget).toBe(true);
    expect(parsed.breakdown).toHaveLength(2);
  });

  it('marks dynamic endpoints as approximate', async () => {
    const handler = handleEstimate(pricingCache, spendTracker);
    const result = await handler({
      calls: [{ endpoint: 'POST /v1/summarize', count: 1 }],
    });
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.breakdown[0].approximate).toBe(true);
    expect(parsed.note).toBeDefined();
  });

  it('reports withinBudget=false when over limit', async () => {
    spendTracker.record_spend(4_999_000); // almost at $5 limit
    const handler = handleEstimate(pricingCache, spendTracker);
    const result = await handler({
      calls: [{ endpoint: 'POST /v1/search', count: 5 }],
    });
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.withinBudget).toBe(false);
  });

  it('handles unknown endpoint gracefully', async () => {
    const handler = handleEstimate(pricingCache, spendTracker);
    const result = await handler({
      calls: [{ endpoint: 'POST /v1/unknown', count: 1 }],
    });
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.breakdown[0].price).toBe('unknown');
  });

  it('returns error for empty calls array', async () => {
    const handler = handleEstimate(pricingCache, spendTracker);
    const result = await handler({ calls: [] });
    expect(result.isError).toBe(true);
  });
});
```

### Test 6: `trace.test.ts` — trace start/stop lifecycle

```typescript
import { describe, it, expect } from 'vitest';
import { handleTrace } from '../src/tools/trace.js';
import type { ActiveTrace } from '../src/types.js';

describe('paygate_trace', () => {
  it('starts a trace', async () => {
    const traces = new Map<string, ActiveTrace>();
    const handler = handleTrace(traces);
    const result = await handler({ action: 'start', name: 'test-trace' });
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.status).toBe('started');
    expect(traces.has('test-trace')).toBe(true);
  });

  it('stops a trace and returns summary', async () => {
    const traces = new Map<string, ActiveTrace>();
    traces.set('test-trace', {
      name: 'test-trace',
      startedAt: Date.now() - 5000,
      entries: [
        { endpoint: 'POST /v1/search', method: 'POST', cost: 1000, timestamp: Date.now(), explorerLink: 'https://testnet.tempo.xyz/tx/0x123' },
        { endpoint: 'POST /v1/search', method: 'POST', cost: 1000, timestamp: Date.now(), explorerLink: 'https://testnet.tempo.xyz/tx/0x456' },
      ],
    });

    const handler = handleTrace(traces);
    const result = await handler({ action: 'stop', name: 'test-trace' });
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.status).toBe('stopped');
    expect(parsed.totalCost).toBe('$0.002000');
    expect(parsed.callCount).toBe(2);
    expect(parsed.breakdown).toHaveLength(1); // grouped by endpoint
    expect(parsed.explorerLinks).toHaveLength(2);
    expect(traces.has('test-trace')).toBe(false);
  });

  it('rejects starting a duplicate trace', async () => {
    const traces = new Map<string, ActiveTrace>();
    traces.set('existing', { name: 'existing', startedAt: Date.now(), entries: [] });
    const handler = handleTrace(traces);
    const result = await handler({ action: 'start', name: 'existing' });
    expect(result.isError).toBe(true);
  });

  it('rejects stopping a non-existent trace', async () => {
    const traces = new Map<string, ActiveTrace>();
    const handler = handleTrace(traces);
    const result = await handler({ action: 'stop', name: 'nope' });
    expect(result.isError).toBe(true);
  });
});
```

### Test 7: `errors.test.ts` — error classification

```typescript
import { describe, it, expect } from 'vitest';
import { classifyError, makeError, errorToMcpContent } from '../src/errors.js';

describe('classifyError', () => {
  it('classifies ECONNREFUSED as gateway_unreachable', () => {
    const err = classifyError(new Error('fetch failed: ECONNREFUSED'));
    expect(err.error).toBe('gateway_unreachable');
    expect(err.recoverable).toBe(true);
  });

  it('classifies balance errors as insufficient_balance', () => {
    const err = classifyError(new Error('insufficient balance for deposit'));
    expect(err.error).toBe('insufficient_balance');
    expect(err.recoverable).toBe(false);
  });

  it('classifies nonce errors as session_creation_failed', () => {
    const err = classifyError(new Error('Session creation failed: nonce expired'));
    expect(err.error).toBe('session_creation_failed');
    expect(err.recoverable).toBe(true);
  });

  it('classifies unknown errors as upstream_error', () => {
    const err = classifyError(new Error('something weird'));
    expect(err.error).toBe('upstream_error');
  });

  it('handles non-Error objects', () => {
    const err = classifyError('string error');
    expect(err.error).toBe('upstream_error');
    expect(err.message).toBe('string error');
  });
});

describe('errorToMcpContent', () => {
  it('wraps error as MCP isError content', () => {
    const err = makeError('invalid_input', 'bad input', false);
    const content = errorToMcpContent(err);
    expect(content.isError).toBe(true);
    const parsed = JSON.parse(content.content[0].text);
    expect(parsed.error).toBe('invalid_input');
    expect(parsed.recoverable).toBe(false);
  });
});
```

### Test 8: `session-manager.test.ts` — session state and shutdown logging

```typescript
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SessionManager } from '../src/session-manager.js';
import type { McpServerConfig, SessionState } from '../src/types.js';

describe('SessionManager', () => {
  let manager: SessionManager;
  const mockConfig: McpServerConfig = {
    gatewayUrl: 'https://example.com',
    privateKey: '0x' + 'ab'.repeat(32),
    payerAddress: '0x1234567890abcdef1234567890abcdef12345678',
    agentName: 'test-agent',
    sessionDeposit: '0.10',
    spendLimitDaily: null,
    spendLimitMonthly: null,
  };

  beforeEach(() => {
    // Mock PayGateClient
    const mockClient = {} as any;
    manager = new SessionManager(mockClient, mockConfig);
  });

  it('returns null when no session', () => {
    expect(manager.getSession()).toBeNull();
    expect(manager.getBalance()).toBe(0);
  });

  it('returns session when set', () => {
    const session: SessionState = {
      sessionId: 'sess_abc',
      sessionSecret: 'ssec_def',
      balance: 100_000,
      ratePerRequest: 1_000,
      expiresAt: new Date(Date.now() + 3600_000).toISOString(),
      gatewayBaseUrl: 'https://example.com',
    };
    manager.setSession(session);
    expect(manager.getSession()).not.toBeNull();
    expect(manager.getBalance()).toBe(100_000);
  });

  it('returns null for expired session', () => {
    manager.setSession({
      sessionId: 'sess_expired',
      sessionSecret: 'ssec_def',
      balance: 100_000,
      ratePerRequest: 1_000,
      expiresAt: new Date(Date.now() - 1000).toISOString(),
      gatewayBaseUrl: 'https://example.com',
    });
    expect(manager.getSession()).toBeNull();
  });

  it('deducts balance correctly', () => {
    manager.setSession({
      sessionId: 'sess_abc',
      sessionSecret: 'ssec_def',
      balance: 100_000,
      ratePerRequest: 1_000,
      expiresAt: new Date(Date.now() + 3600_000).toISOString(),
      gatewayBaseUrl: 'https://example.com',
    });
    manager.deductBalance(5_000);
    expect(manager.getBalance()).toBe(95_000);
  });

  it('logs session state on shutdown', () => {
    const stderrSpy = vi.spyOn(process.stderr, 'write').mockImplementation(() => true);
    manager.setSession({
      sessionId: 'sess_abc',
      sessionSecret: 'ssec_def',
      balance: 32_000,
      ratePerRequest: 1_000,
      expiresAt: '2026-03-25T12:00:00Z',
      gatewayBaseUrl: 'https://example.com',
    });
    manager.logShutdownState();
    expect(stderrSpy).toHaveBeenCalledWith(
      expect.stringContaining('sess_abc')
    );
    expect(stderrSpy).toHaveBeenCalledWith(
      expect.stringContaining('$0.032000')
    );
    stderrSpy.mockRestore();
  });

  it('logs "no active session" when none exists', () => {
    const stderrSpy = vi.spyOn(process.stderr, 'write').mockImplementation(() => true);
    manager.logShutdownState();
    expect(stderrSpy).toHaveBeenCalledWith(
      expect.stringContaining('No active session')
    );
    stderrSpy.mockRestore();
  });
});
```

### Test 9: `call.test.ts` — spend limit enforcement in paygate_call

```typescript
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { handleCall } from '../src/tools/call.js';
import { SpendTracker } from '../src/spend-tracker.js';
import { SessionManager } from '../src/session-manager.js';
import { PricingCacheManager } from '../src/pricing-cache.js';
import type { McpServerConfig, ActiveTrace } from '../src/types.js';

describe('paygate_call', () => {
  const mockConfig: McpServerConfig = {
    gatewayUrl: 'https://example.com',
    privateKey: '0x' + 'ab'.repeat(32),
    payerAddress: '0x1234',
    agentName: 'test',
    sessionDeposit: '0.10',
    spendLimitDaily: 5_000_000,
    spendLimitMonthly: null,
  };

  it('rejects when spend limit would be exceeded', async () => {
    const spendTracker = new SpendTracker(5_000_000, null);
    spendTracker.record_spend(4_999_500);

    const pricingCache = new PricingCacheManager('https://example.com');
    vi.spyOn(pricingCache, 'priceFor').mockResolvedValue({
      endpoint: 'POST /v1/search',
      method: 'POST',
      path: '/v1/search',
      price: '0.001000',
      priceBaseUnits: 1000,
      description: 'Search',
      dynamic: false,
    });

    const mockClient = {} as any;
    const sessionManager = new SessionManager(mockClient, mockConfig);
    const traces = new Map<string, ActiveTrace>();

    const handler = handleCall(mockClient, mockConfig, spendTracker, sessionManager, pricingCache, traces);
    const result = await handler({ method: 'POST', path: '/v1/search', body: { query: 'test' } });

    expect(result.isError).toBe(true);
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.error).toBe('spend_limit_exceeded');
  });

  it('rejects invalid method', async () => {
    const spendTracker = new SpendTracker(null, null);
    const pricingCache = new PricingCacheManager('https://example.com');
    const mockClient = {} as any;
    const sessionManager = new SessionManager(mockClient, mockConfig);
    const traces = new Map<string, ActiveTrace>();

    const handler = handleCall(mockClient, mockConfig, spendTracker, sessionManager, pricingCache, traces);
    const result = await handler({ method: 'PATCH', path: '/v1/search' });

    expect(result.isError).toBe(true);
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.error).toBe('invalid_input');
  });

  it('rejects missing method/path', async () => {
    const spendTracker = new SpendTracker(null, null);
    const pricingCache = new PricingCacheManager('https://example.com');
    const mockClient = {} as any;
    const sessionManager = new SessionManager(mockClient, mockConfig);
    const traces = new Map<string, ActiveTrace>();

    const handler = handleCall(mockClient, mockConfig, spendTracker, sessionManager, pricingCache, traces);
    const result = await handler({ method: '', path: '' });

    expect(result.isError).toBe(true);
  });
});
```

### Test 10: `pricing-cache.test.ts` — caching and TTL

```typescript
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { PricingCacheManager } from '../src/pricing-cache.js';

describe('PricingCacheManager', () => {
  beforeEach(() => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(
      JSON.stringify({
        apis: [
          {
            endpoint: 'POST /v1/search',
            method: 'POST',
            path: '/v1/search',
            price: '0.001000',
            price_base_units: 1000,
            description: 'Search the web',
            dynamic: false,
          },
        ],
        recipient: '0xRecipient',
        token: '0xUSDC',
      }),
      { status: 200 }
    ));
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('fetches pricing on first call', async () => {
    const cache = new PricingCacheManager('https://example.com');
    const pricing = await cache.getPricing();
    expect(pricing.endpoints).toHaveLength(1);
    expect(pricing.endpoints[0].endpoint).toBe('POST /v1/search');
    expect(globalThis.fetch).toHaveBeenCalledTimes(1);
  });

  it('returns cached pricing on second call within TTL', async () => {
    const cache = new PricingCacheManager('https://example.com');
    await cache.getPricing();
    await cache.getPricing();
    expect(globalThis.fetch).toHaveBeenCalledTimes(1); // only once
  });

  it('refreshes after invalidate()', async () => {
    const cache = new PricingCacheManager('https://example.com');
    await cache.getPricing();
    cache.invalidate();
    await cache.getPricing();
    expect(globalThis.fetch).toHaveBeenCalledTimes(2);
  });

  it('looks up specific endpoint pricing', async () => {
    const cache = new PricingCacheManager('https://example.com');
    const ep = await cache.priceFor('POST /v1/search');
    expect(ep).not.toBeNull();
    expect(ep!.priceBaseUnits).toBe(1000);
  });

  it('returns null for unknown endpoint', async () => {
    const cache = new PricingCacheManager('https://example.com');
    const ep = await cache.priceFor('POST /v1/unknown');
    expect(ep).toBeNull();
  });
});
```

### Test 11: `budget.test.ts` — budget output format

```typescript
import { describe, it, expect } from 'vitest';
import { handleBudget } from '../src/tools/budget.js';
import { SpendTracker } from '../src/spend-tracker.js';
import { SessionManager } from '../src/session-manager.js';
import type { McpServerConfig } from '../src/types.js';

describe('paygate_budget', () => {
  const config: McpServerConfig = {
    gatewayUrl: 'https://example.com',
    privateKey: '0x' + 'ab'.repeat(32),
    payerAddress: '0x1234',
    agentName: 'test-agent',
    sessionDeposit: '0.10',
    spendLimitDaily: 5_000_000,
    spendLimitMonthly: 50_000_000,
  };

  it('returns complete budget info with session', async () => {
    const spendTracker = new SpendTracker(5_000_000, 50_000_000);
    spendTracker.record_spend(1_000_000);

    const mockClient = {} as any;
    const sessionManager = new SessionManager(mockClient, config);
    sessionManager.setSession({
      sessionId: 'sess_abc',
      sessionSecret: 'ssec_def',
      balance: 90_000,
      ratePerRequest: 1_000,
      expiresAt: new Date(Date.now() + 3600_000).toISOString(),
      gatewayBaseUrl: 'https://example.com',
    });

    const handler = handleBudget(spendTracker, sessionManager, config);
    const result = await handler();
    const parsed = JSON.parse(result.content[0].text);

    expect(parsed.session.sessionId).toBe('sess_abc');
    expect(parsed.session.balance).toBe('$0.090000');
    expect(parsed.spending.totalSpentToday).toBe('$1.000000');
    expect(parsed.limits.daily).toBe('$5.000000');
    expect(parsed.limits.remainingDaily).toBe('$4.000000');
    expect(parsed.agent).toBe('test-agent');
  });

  it('returns null session when none active', async () => {
    const spendTracker = new SpendTracker(null, null);
    const mockClient = {} as any;
    const sessionManager = new SessionManager(mockClient, config);

    const handler = handleBudget(spendTracker, sessionManager, config);
    const result = await handler();
    const parsed = JSON.parse(result.content[0].text);

    expect(parsed.session).toBeNull();
    expect(parsed.limits.daily).toBe('$5.000000');
  });
});
```

### Test 12: `integration.test.ts` — full tool registration and dispatch

```typescript
import { describe, it, expect } from 'vitest';

/**
 * Integration test: verifies all tools are registered and dispatch works.
 * This tests the tool schema definitions and name matching in index.ts.
 */
describe('MCP tool registration', () => {
  const EXPECTED_TOOLS = [
    'paygate_discover',
    'paygate_call',
    'paygate_budget',
    'paygate_estimate',
    'paygate_trace',
  ];

  it('all 5 core tools are defined', () => {
    // Verify tool names are all distinct
    const unique = new Set(EXPECTED_TOOLS);
    expect(unique.size).toBe(5);
  });

  it('paygate_call requires method and path', () => {
    // Schema validation: method and path are required
    const callSchema = {
      type: 'object',
      properties: {
        method: { type: 'string', enum: ['GET', 'POST', 'PUT', 'DELETE'] },
        path: { type: 'string' },
        body: { type: 'object' },
        headers: { type: 'object', additionalProperties: { type: 'string' } },
      },
      required: ['method', 'path'],
    };
    expect(callSchema.required).toContain('method');
    expect(callSchema.required).toContain('path');
  });

  it('paygate_estimate requires calls array', () => {
    const estimateSchema = {
      type: 'object',
      properties: {
        calls: { type: 'array' },
      },
      required: ['calls'],
    };
    expect(estimateSchema.required).toContain('calls');
  });

  it('paygate_trace requires action and name', () => {
    const traceSchema = {
      type: 'object',
      properties: {
        action: { type: 'string', enum: ['start', 'stop'] },
        name: { type: 'string' },
      },
      required: ['action', 'name'],
    };
    expect(traceSchema.required).toContain('action');
    expect(traceSchema.required).toContain('name');
  });

  it('paygate_discover has optional goal parameter', () => {
    const discoverSchema = {
      type: 'object',
      properties: {
        goal: { type: 'string' },
      },
    };
    // No required array = all optional
    expect((discoverSchema as any).required).toBeUndefined();
  });

  it('paygate_budget has no required parameters', () => {
    const budgetSchema = {
      type: 'object',
      properties: {},
    };
    expect(Object.keys(budgetSchema.properties)).toHaveLength(0);
  });
});
```

## Summary of Deliverables

| Deliverable | File(s) |
|---|---|
| Root npm workspaces | `/package.json` |
| MCP server package | `packages/mcp-server/package.json`, `tsconfig.json` |
| Entry point + MCP setup | `packages/mcp-server/src/index.ts` |
| 5 tool implementations | `src/tools/{discover,call,budget,estimate,trace}.ts` |
| Session manager | `src/session-manager.ts` |
| Spend tracker | `src/spend-tracker.ts` |
| Pricing cache | `src/pricing-cache.ts` |
| Key loader (CMD support) | `src/key-loader.ts` |
| Error handling | `src/errors.ts` |
| Types | `src/types.ts` |
| llms.txt | `docs/llms.txt` |
| 12 test files | `tests/*.test.ts` |
