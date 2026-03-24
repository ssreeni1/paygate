# Build Brief: SDK Enhancements — Price Estimation + failureMode + Agent Identity (Wave 3, Stream 3)

Add three features to `@paygate/sdk`: cost estimation with pricing cache, fail-open/fail-closed gateway bypass, and agent identity header propagation. All changes are in the TypeScript SDK (`sdk/`). No Rust changes.

## Source Files You Are Modifying

| File | What changes |
|------|-------------|
| `sdk/src/types.ts` | Add `EstimateCostResult`, `EstimateCostEntry`, `FailureMode` type, extend `PayGateClientOptions` |
| `sdk/src/client.ts` | Add `estimateCost()` method, pricing cache, `failureMode` logic in `fetch()`, `agentName` header injection |
| `sdk/src/discovery.ts` | Add `fetchPricingEndpoint()` that calls `GET /v1/pricing` and returns structured pricing map |
| `sdk/tests/client.test.ts` | Add 10+ new tests |
| `sdk/src/index.ts` | Export new types |

---

## 1. New Types — `sdk/src/types.ts`

### 1.1 `FailureMode` Type

```typescript
export type FailureMode = 'open' | 'closed';
```

### 1.2 Extended `PayGateClientOptions`

Add three new optional fields to the existing interface:

```typescript
export interface PayGateClientOptions {
  /** Function that executes on-chain payment. Returns tx hash. */
  payFunction: (params: PaymentParams) => Promise<string>;
  /** Payer's wallet address */
  payerAddress: string;
  /** Max retries after payment (default: 1) */
  maxRetries?: number;
  /** Enable automatic session management */
  autoSession?: boolean;
  /** Deposit amount per session in USDC (default: "0.10") */
  sessionDeposit?: string;

  // --- NEW FIELDS (Wave 3) ---

  /**
   * Behavior when the gateway is unreachable due to network errors.
   * - 'closed' (default): throw the network error. Safe default.
   * - 'open': bypass payment and forward the request directly to upstreamUrl.
   * Only triggers on network-level failures (ECONNREFUSED, ENOTFOUND, fetch TypeError,
   * AbortError/timeout). Does NOT trigger on HTTP error responses (4xx, 5xx) from
   * the gateway — those are always propagated.
   */
  failureMode?: FailureMode;

  /**
   * Required when failureMode is 'open'. The upstream origin URL to forward requests
   * to when the gateway is unreachable. Example: 'https://api.upstream.com'
   *
   * The SDK cannot auto-discover this (it is behind the gateway config), so the
   * caller must provide it explicitly.
   *
   * The path from the original request URL is preserved; only the origin is replaced.
   */
  upstreamUrl?: string;

  /**
   * Agent identity string. If set, the SDK includes an `X-Payment-Agent: <agentName>`
   * header on EVERY outgoing request, including session nonce requests and session
   * creation requests.
   */
  agentName?: string;

  /**
   * Optional spend limit in USDC (decimal string, e.g. "5.00").
   * Used by estimateCost() to compute the `withinBudget` flag.
   * Not enforced in fetch() — enforcement is server-side (gateway governance).
   */
  spendLimit?: string;
}
```

### 1.3 `EstimateCostEntry`

```typescript
export interface EstimateCostEntry {
  /** Endpoint path, e.g. "POST /v1/search" */
  endpoint: string;
  /** Price per request as decimal string, e.g. "0.005000" */
  price: string;
  /** Number of calls planned */
  count: number;
  /** price * count as decimal string */
  subtotal: string;
  /** True if this endpoint uses dynamic pricing (price is approximate) */
  dynamic: boolean;
}
```

### 1.4 `EstimateCostResult`

```typescript
export interface EstimateCostResult {
  /** Total estimated cost as decimal string, e.g. "0.025000" */
  total: string;
  /** Per-endpoint breakdown */
  breakdown: EstimateCostEntry[];
  /**
   * True if total <= spendLimit (from client options).
   * Always true if no spendLimit is configured.
   */
  withinBudget: boolean;
}
```

### 1.5 `EndpointPricing` (internal, for pricing cache)

```typescript
export interface EndpointPricing {
  /** Decimal price string, e.g. "0.005000" */
  price: string;
  /** Price in base units (integer) */
  priceBaseUnits: number;
  /** Number of decimals (typically 6 for USDC) */
  decimals: number;
  /** Whether this endpoint uses dynamic pricing */
  dynamic: boolean;
}
```

---

## 2. Pricing Discovery Enhancement — `sdk/src/discovery.ts`

### 2.1 New Function: `fetchEndpointPricing()`

The existing `getPricing()` function triggers a 402 on the base URL. For `estimateCost()`, we need the full pricing map from `GET /v1/pricing` (a free endpoint on the gateway that returns all endpoint prices).

Add this function:

```typescript
/**
 * Fetch the full pricing map from the gateway's /v1/pricing endpoint.
 * Returns a map of "METHOD /path" -> EndpointPricing.
 *
 * Expected response shape:
 * {
 *   "apis": [
 *     { "endpoint": "POST /v1/search", "price": "0.005000", "price_base_units": 5000, "decimals": 6, "dynamic": false },
 *     { "endpoint": "POST /v1/summarize", "price": "0.010000", "price_base_units": 10000, "decimals": 6, "dynamic": true },
 *     ...
 *   ]
 * }
 */
export async function fetchEndpointPricing(
  baseUrl: string,
): Promise<Map<string, EndpointPricing>> {
  const url = `${baseUrl.replace(/\/$/, '')}/v1/pricing`;
  const response = await fetch(url, { method: 'GET' });

  if (!response.ok) {
    throw new Error(`Failed to fetch pricing from ${url}: ${response.status}`);
  }

  const body = await response.json() as {
    apis: Array<{
      endpoint: string;
      price: string;
      price_base_units: number;
      decimals: number;
      dynamic: boolean;
    }>;
  };

  const map = new Map<string, EndpointPricing>();
  for (const api of body.apis) {
    map.set(api.endpoint, {
      price: api.price,
      priceBaseUnits: api.price_base_units,
      decimals: api.decimals,
      dynamic: api.dynamic ?? false,
    });
  }
  return map;
}
```

Export `fetchEndpointPricing` from `sdk/src/index.ts`.

---

## 3. Client Changes — `sdk/src/client.ts`

### 3.1 New Private Fields

Add these fields to the `PayGateClient` class:

```typescript
private failureMode: FailureMode;
private upstreamUrl: string | null;
private agentName: string | null;
private spendLimit: number | null; // base units, null = no limit

// Pricing cache for estimateCost()
private pricingCache: Map<string, EndpointPricing> | null = null;
private pricingCacheExpiry: number = 0; // Unix ms timestamp
private static readonly PRICING_CACHE_TTL_MS = 60_000; // 60 seconds
```

### 3.2 Constructor Updates

```typescript
constructor(options: PayGateClientOptions) {
  this.payFunction = options.payFunction;
  this.payerAddress = options.payerAddress;
  this.maxRetries = options.maxRetries ?? 1;
  this.autoSession = options.autoSession ?? false;
  this.sessionDeposit = options.sessionDeposit ?? '0.10';

  // Wave 3 additions
  this.failureMode = options.failureMode ?? 'closed';
  this.upstreamUrl = options.upstreamUrl ?? null;
  this.agentName = options.agentName ?? null;
  this.spendLimit = options.spendLimit
    ? Math.round(parseFloat(options.spendLimit) * 1_000_000)
    : null;

  // Validate: failureMode 'open' requires upstreamUrl
  if (this.failureMode === 'open' && !this.upstreamUrl) {
    throw new Error("failureMode 'open' requires upstreamUrl to be set");
  }
}
```

### 3.3 Agent Name Header Injection

Create a private helper that injects `X-Payment-Agent` into any `RequestInit`. This header must be present on ALL outgoing requests: initial fetch, session nonce, session creation, retries.

```typescript
private injectAgentHeader(headers: Record<string, string>): Record<string, string> {
  if (this.agentName) {
    return { ...headers, 'X-Payment-Agent': this.agentName };
  }
  return headers;
}
```

**Integration points** (every place that calls `fetch()`):

1. In the auto-session HMAC path (line ~45): add `X-Payment-Agent` to `sessionHeaders` before calling `this.mergeHeaders()`
2. In the auto-session creation path (line ~66): add to the initial `fetch(url, init)` call
3. In `createSession()` — both the nonce request (line ~144) and the session creation request (line ~165): add `X-Payment-Agent` to the headers objects
4. In the direct payment retry loop (line ~118): add to `paymentHeaders`

**Implementation approach**: Modify `mergeHeaders()` to always inject the agent header:

```typescript
private mergeHeaders(init: RequestInit | undefined, extra: Record<string, string>): RequestInit {
  const existingHeaders: Record<string, string> = {};
  if (init?.headers) {
    if (init.headers instanceof Headers) {
      init.headers.forEach((v, k) => { existingHeaders[k] = v; });
    } else if (Array.isArray(init.headers)) {
      for (const [k, v] of init.headers) { existingHeaders[k] = v; }
    } else {
      Object.assign(existingHeaders, init.headers);
    }
  }
  const merged = { ...existingHeaders, ...extra };
  // Always inject agent name if configured
  if (this.agentName) {
    merged['X-Payment-Agent'] = this.agentName;
  }
  return {
    ...init,
    headers: merged,
  };
}
```

Also update the standalone `fetch()` calls that do NOT go through `mergeHeaders()` (the initial probing requests at lines 46, 66, 92). Wrap these:

```typescript
// Before:
const response = await fetch(url, init);

// After:
const response = await fetch(url, this.mergeHeaders(init, {}));
```

This ensures `X-Payment-Agent` is on every single outgoing request.

Also update `createSession()` — the nonce request and session creation request headers must include agent name:

```typescript
// Nonce request
const nonceHeaders: Record<string, string> = {
  'X-Payment-Payer': this.payerAddress,
};
if (this.agentName) {
  nonceHeaders['X-Payment-Agent'] = this.agentName;
}

// Session creation request
const sessionHeaders: Record<string, string> = {
  'X-Payment-Tx': txHash,
  'X-Payment-Payer': this.payerAddress,
  'Content-Type': 'application/json',
};
if (this.agentName) {
  sessionHeaders['X-Payment-Agent'] = this.agentName;
}
```

### 3.4 `failureMode` Implementation in `fetch()`

The fail-open/fail-closed logic wraps the entire payment flow. It only triggers on **network-level errors** — not on HTTP responses from the gateway (even 5xx).

**What counts as a network error:**
- `TypeError` thrown by `fetch()` (standard for network failures: DNS resolution failure, connection refused, etc.)
- `AbortError` or `DOMException` with name `AbortError` (request timeout)
- Any error where `error instanceof TypeError` or `error.name === 'AbortError'`

**What does NOT trigger failureMode:**
- HTTP 4xx responses from the gateway (including 402, 403, etc.)
- HTTP 5xx responses from the gateway
- Payment function failures
- Session creation failures due to bad nonce, invalid deposit, etc.

**Implementation**: Add a private helper to detect network errors:

```typescript
private isNetworkError(error: unknown): boolean {
  if (error instanceof TypeError) return true; // fetch() network failure
  if (error instanceof DOMException && error.name === 'AbortError') return true; // timeout
  if (error instanceof Error) {
    const msg = error.message.toLowerCase();
    // Node.js fetch errors
    if (msg.includes('econnrefused') || msg.includes('enotfound') || msg.includes('etimedout')) {
      return true;
    }
  }
  return false;
}
```

**Wrap the `fetch()` method body in a try-catch:**

```typescript
async fetch(url: string, init?: RequestInit): Promise<Response> {
  try {
    return await this._fetchInner(url, init);
  } catch (error) {
    if (this.failureMode === 'open' && this.isNetworkError(error)) {
      // Bypass: replace gateway origin with upstreamUrl origin, forward directly
      return this.bypassToUpstream(url, init);
    }
    throw error; // 'closed' mode or non-network error: re-throw
  }
}
```

Rename the existing `fetch()` body to `_fetchInner()` (private). The public `fetch()` becomes the try-catch wrapper.

**Bypass helper:**

```typescript
private async bypassToUpstream(url: string, init?: RequestInit): Promise<Response> {
  const original = new URL(url);
  const upstream = new URL(this.upstreamUrl!);
  const bypassUrl = `${upstream.origin}${original.pathname}${original.search}`;

  // Strip all X-Payment-* headers — upstream does not need them
  const cleanInit = { ...init };
  if (cleanInit.headers) {
    const cleaned: Record<string, string> = {};
    const extractHeaders = (headers: HeadersInit) => {
      if (headers instanceof Headers) {
        headers.forEach((v, k) => { if (!k.toLowerCase().startsWith('x-payment-')) cleaned[k] = v; });
      } else if (Array.isArray(headers)) {
        for (const [k, v] of headers) { if (!k.toLowerCase().startsWith('x-payment-')) cleaned[k] = v; }
      } else {
        for (const [k, v] of Object.entries(headers)) {
          if (!k.toLowerCase().startsWith('x-payment-')) cleaned[k] = v;
        }
      }
    };
    extractHeaders(cleanInit.headers);
    cleanInit.headers = cleaned;
  }

  return fetch(bypassUrl, cleanInit);
}
```

### 3.5 `estimateCost()` Method

```typescript
/**
 * Estimate the cost of a planned set of API calls.
 *
 * Fetches pricing from the gateway's GET /v1/pricing endpoint (cached for 60s).
 * For dynamic-priced endpoints, the price is marked as approximate.
 *
 * @param gatewayBaseUrl - The gateway origin, e.g. "https://paygate.example.com"
 * @param calls - Array of { endpoint, count } where endpoint is "METHOD /path"
 * @returns EstimateCostResult with total, breakdown, and withinBudget flag
 */
async estimateCost(
  gatewayBaseUrl: string,
  calls: { endpoint: string; count: number }[],
): Promise<EstimateCostResult> {
  // Fetch or use cached pricing
  const pricing = await this.getOrFetchPricing(gatewayBaseUrl);

  let totalBaseUnits = 0;
  const breakdown: EstimateCostEntry[] = [];

  for (const call of calls) {
    const endpointPricing = pricing.get(call.endpoint);
    if (!endpointPricing) {
      throw new Error(
        `Unknown endpoint: "${call.endpoint}". ` +
        `Available endpoints: ${[...pricing.keys()].join(', ')}`
      );
    }

    const subtotalBaseUnits = endpointPricing.priceBaseUnits * call.count;
    totalBaseUnits += subtotalBaseUnits;

    breakdown.push({
      endpoint: call.endpoint,
      price: endpointPricing.price,
      count: call.count,
      subtotal: formatUsdc(subtotalBaseUnits, endpointPricing.decimals),
      dynamic: endpointPricing.dynamic,
    });
  }

  const decimals = breakdown.length > 0
    ? (pricing.get(calls[0].endpoint)?.decimals ?? 6)
    : 6;
  const total = formatUsdc(totalBaseUnits, decimals);

  const withinBudget = this.spendLimit === null
    ? true
    : totalBaseUnits <= this.spendLimit;

  return { total, breakdown, withinBudget };
}
```

### 3.6 Pricing Cache Helper

```typescript
private async getOrFetchPricing(gatewayBaseUrl: string): Promise<Map<string, EndpointPricing>> {
  const now = Date.now();
  if (this.pricingCache && now < this.pricingCacheExpiry) {
    return this.pricingCache;
  }

  this.pricingCache = await fetchEndpointPricing(gatewayBaseUrl);
  this.pricingCacheExpiry = now + PayGateClient.PRICING_CACHE_TTL_MS;
  return this.pricingCache;
}
```

### 3.7 USDC Formatting Helper

Add as a module-level function in `client.ts` (or as a static method):

```typescript
function formatUsdc(baseUnits: number, decimals: number): string {
  const divisor = 10 ** decimals;
  return (baseUnits / divisor).toFixed(decimals);
}
```

---

## 4. Exports — `sdk/src/index.ts`

Update to export new types:

```typescript
export { PayGateClient } from './client.js';
export { requestHash, paymentMemo, sessionMemo, hmacSha256 } from './hash.js';
export { parse402Response, isPaymentRequired, getPricing, fetchEndpointPricing } from './discovery.js';
export type * from './types.js';
```

---

## 5. Tests — `sdk/tests/client.test.ts`

Add a new `describe` block for each feature. All tests use the existing vitest + `vi.spyOn(globalThis, 'fetch')` mock pattern already established in the file.

### 5.1 Mock Helpers to Add

```typescript
const MOCK_PRICING_RESPONSE = {
  apis: [
    { endpoint: 'POST /v1/search', price: '0.005000', price_base_units: 5000, decimals: 6, dynamic: false },
    { endpoint: 'POST /v1/summarize', price: '0.010000', price_base_units: 10000, decimals: 6, dynamic: true },
    { endpoint: 'POST /v1/image', price: '0.050000', price_base_units: 50000, decimals: 6, dynamic: false },
  ],
};

function makePricingResponse(): Response {
  return new Response(JSON.stringify(MOCK_PRICING_RESPONSE), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  });
}
```

### 5.2 Test: estimateCost happy path

```
describe('estimateCost', () => {
  it('computes total and breakdown for multiple endpoints', async () => {
    // Mock GET /v1/pricing
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(makePricingResponse());

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
    });

    const result = await client.estimateCost('https://gateway.example.com', [
      { endpoint: 'POST /v1/search', count: 3 },
      { endpoint: 'POST /v1/summarize', count: 2 },
    ]);

    expect(result.total).toBe('0.035000');  // 3*0.005 + 2*0.01
    expect(result.breakdown).toHaveLength(2);
    expect(result.breakdown[0]).toEqual({
      endpoint: 'POST /v1/search',
      price: '0.005000',
      count: 3,
      subtotal: '0.015000',
      dynamic: false,
    });
    expect(result.breakdown[1]).toEqual({
      endpoint: 'POST /v1/summarize',
      price: '0.010000',
      count: 2,
      subtotal: '0.020000',
      dynamic: true,
    });
    expect(result.withinBudget).toBe(true); // no spendLimit set
  });
```

### 5.3 Test: estimateCost with budget — within

```
  it('withinBudget is true when total <= spendLimit', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(makePricingResponse());

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      spendLimit: '0.05',
    });

    const result = await client.estimateCost('https://gateway.example.com', [
      { endpoint: 'POST /v1/search', count: 5 },
    ]);

    expect(result.total).toBe('0.025000');
    expect(result.withinBudget).toBe(true);
  });
```

### 5.4 Test: estimateCost with budget — exceeded

```
  it('withinBudget is false when total > spendLimit', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(makePricingResponse());

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      spendLimit: '0.01',
    });

    const result = await client.estimateCost('https://gateway.example.com', [
      { endpoint: 'POST /v1/search', count: 5 },
    ]);

    expect(result.total).toBe('0.025000');
    expect(result.withinBudget).toBe(false);
  });
```

### 5.5 Test: estimateCost uses pricing cache (does not re-fetch within 60s)

```
  it('uses cached pricing on second call within TTL', async () => {
    const mockFetch = vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(makePricingResponse());

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
    });

    await client.estimateCost('https://gateway.example.com', [
      { endpoint: 'POST /v1/search', count: 1 },
    ]);

    // Second call — should NOT trigger another fetch
    const result = await client.estimateCost('https://gateway.example.com', [
      { endpoint: 'POST /v1/summarize', count: 1 },
    ]);

    expect(mockFetch).toHaveBeenCalledTimes(1); // only one pricing fetch
    expect(result.total).toBe('0.010000');
  });
```

### 5.6 Test: estimateCost with unknown endpoint throws

```
  it('throws on unknown endpoint', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(makePricingResponse());

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
    });

    await expect(
      client.estimateCost('https://gateway.example.com', [
        { endpoint: 'POST /v1/nonexistent', count: 1 },
      ])
    ).rejects.toThrow('Unknown endpoint');
  });
```

### 5.7 Test: estimateCost with empty calls array

```
  it('returns zero total for empty calls array', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(makePricingResponse());

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      spendLimit: '1.00',
    });

    const result = await client.estimateCost('https://gateway.example.com', []);

    expect(result.total).toBe('0.000000');
    expect(result.breakdown).toHaveLength(0);
    expect(result.withinBudget).toBe(true);
  });
});
```

### 5.8 Test: failureMode 'closed' throws on network error (default behavior)

```
describe('failureMode', () => {
  it('closed (default) throws on network error', async () => {
    vi.spyOn(globalThis, 'fetch').mockRejectedValueOnce(new TypeError('fetch failed'));

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      // failureMode defaults to 'closed'
    });

    await expect(
      client.fetch('https://gateway.example.com/v1/search', {
        method: 'POST',
        body: '{"q":"test"}',
      })
    ).rejects.toThrow('fetch failed');
  });
```

### 5.9 Test: failureMode 'open' bypasses to upstream on network error

```
  it('open bypasses to upstream on network error', async () => {
    const mockFetch = vi.spyOn(globalThis, 'fetch')
      .mockRejectedValueOnce(new TypeError('fetch failed'))  // gateway unreachable
      .mockResolvedValueOnce(make200Response('{"fallback":true}')); // upstream direct

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      failureMode: 'open',
      upstreamUrl: 'https://upstream.example.com',
    });

    const response = await client.fetch('https://gateway.example.com/v1/search?q=test', {
      method: 'POST',
      body: '{"q":"test"}',
    });

    expect(response.status).toBe(200);
    const body = await response.json();
    expect(body).toEqual({ fallback: true });

    // Verify the bypass call went to upstream with correct path
    expect(mockFetch).toHaveBeenCalledTimes(2);
    const bypassCall = mockFetch.mock.calls[1];
    expect(bypassCall[0]).toBe('https://upstream.example.com/v1/search?q=test');
  });
```

### 5.10 Test: failureMode 'open' does NOT trigger on HTTP 5xx from gateway

```
  it('open does NOT bypass on gateway 5xx (only network errors)', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(
      new Response('Internal Server Error', { status: 500 })
    );

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      failureMode: 'open',
      upstreamUrl: 'https://upstream.example.com',
    });

    const response = await client.fetch('https://gateway.example.com/v1/search', {
      method: 'POST',
      body: '{}',
    });

    // 500 is returned as-is, no bypass
    expect(response.status).toBe(500);
  });
```

### 5.11 Test: failureMode 'open' without upstreamUrl throws at construction

```
  it('open without upstreamUrl throws at construction', () => {
    expect(() => new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      failureMode: 'open',
      // no upstreamUrl
    })).toThrow("failureMode 'open' requires upstreamUrl");
  });
});
```

### 5.12 Test: agentName propagated on every request

```
describe('agentName', () => {
  it('includes X-Payment-Agent header on every outgoing request', async () => {
    const mockFetch = vi.spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(make402Response())
      .mockResolvedValueOnce(make200Response());

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      agentName: 'my-research-bot',
    });

    await client.fetch('https://gateway.example.com/v1/search', {
      method: 'POST',
      body: '{"q":"test"}',
    });

    // Both the initial 402 probe and the payment retry should have the agent header
    for (const call of mockFetch.mock.calls) {
      const init = call[1] as RequestInit;
      const headers = init.headers as Record<string, string>;
      expect(headers['X-Payment-Agent']).toBe('my-research-bot');
    }
  });
```

### 5.13 Test: agentName propagated on session creation requests

```
  it('includes X-Payment-Agent in session nonce and creation requests', async () => {
    const sessionClient = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      autoSession: true,
      sessionDeposit: '0.05',
      agentName: 'session-bot',
    });

    const mockFetch = vi.spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(make402Response())       // 1. initial -> 402
      .mockResolvedValueOnce(                         // 2. nonce
        new Response(JSON.stringify({ nonce: 'nonce_abc' }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        })
      )
      .mockResolvedValueOnce(                         // 3. session create
        new Response(JSON.stringify({
          sessionId: 'sess_1',
          sessionSecret: 'ssec_aabbccdd',
          balance: '0.050000',
          ratePerRequest: '0.000500',
        }), { status: 200, headers: { 'Content-Type': 'application/json' } })
      )
      .mockResolvedValueOnce(make200Response());       // 4. retry

    await sessionClient.fetch('https://gateway.example.com/v1/chat', {
      method: 'POST',
      body: '{}',
    });

    // Check nonce request (call index 1) has agent header
    const nonceInit = mockFetch.mock.calls[1][1] as RequestInit;
    const nonceHeaders = nonceInit.headers as Record<string, string>;
    expect(nonceHeaders['X-Payment-Agent']).toBe('session-bot');

    // Check session creation request (call index 2) has agent header
    const createInit = mockFetch.mock.calls[2][1] as RequestInit;
    const createHeaders = createInit.headers as Record<string, string>;
    expect(createHeaders['X-Payment-Agent']).toBe('session-bot');
  });
});
```

### 5.14 Test: estimateCost marks dynamic endpoints

```
describe('estimateCost dynamic pricing', () => {
  it('marks dynamic-priced endpoints in breakdown', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(makePricingResponse());

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
    });

    const result = await client.estimateCost('https://gateway.example.com', [
      { endpoint: 'POST /v1/search', count: 1 },
      { endpoint: 'POST /v1/summarize', count: 1 },
    ]);

    const searchEntry = result.breakdown.find(b => b.endpoint === 'POST /v1/search')!;
    const summarizeEntry = result.breakdown.find(b => b.endpoint === 'POST /v1/summarize')!;

    expect(searchEntry.dynamic).toBe(false);
    expect(summarizeEntry.dynamic).toBe(true);
  });
});
```

---

## 6. Validation Checklist

After implementing all changes, verify:

1. `cd sdk && npm test` — all existing tests still pass
2. All 14 new tests pass
3. `cd sdk && npm run build` — TypeScript compiles with no errors
4. No `any` types introduced
5. `X-Payment-Agent` header is present on EVERY outgoing `fetch()` call when `agentName` is set — including session nonce, session creation, initial probes, and retries
6. `failureMode: 'open'` only triggers on `TypeError` / `AbortError` / ECONNREFUSED — NOT on 4xx/5xx HTTP responses
7. `bypassToUpstream` strips all `X-Payment-*` headers before forwarding
8. Pricing cache expires after 60 seconds (not infinite)
9. `estimateCost` with empty `calls` array returns zero total and `withinBudget: true`
10. Constructor throws immediately if `failureMode: 'open'` without `upstreamUrl`
