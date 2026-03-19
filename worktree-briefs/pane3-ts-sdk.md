# Pane 3 Brief: TypeScript SDK (feat/ts-sdk)

## Context
You are building the TypeScript client SDK for PayGate — a library that lets JavaScript/TypeScript apps call PayGate-protected APIs with automatic payment. The SDK handles: price discovery (parsing 402 responses), payment execution, and transparent retry.

## Required Reading (do this first)
1. `SPEC.md` — Focus on:
   - §4.1 Price Discovery (402 response format — headers AND JSON body)
   - §4.2 Direct Payment Flow (request hash computation, memo format, payment headers)
   - §7.1 TypeScript SDK example (the target API surface)
   - §7.3 Agent Tool Pattern
2. `crates/paygate-common/src/hash.rs` — The Rust implementation of request_hash and payment_memo. Your TypeScript MUST produce IDENTICAL output byte-for-byte.
3. `crates/paygate-common/src/mpp.rs` — Header constant names
4. `tests/fixtures/request_hash_vectors.json` — Shared test vectors. Your tests MUST validate against these.

## What to Build

All files go in `sdk/` directory.

### 1. `sdk/package.json`
```json
{
  "name": "@paygate/sdk",
  "version": "0.1.0",
  "type": "module",
  "main": "dist/index.js",
  "types": "dist/index.d.ts",
  "scripts": {
    "build": "tsc",
    "test": "vitest run",
    "test:watch": "vitest"
  },
  "dependencies": {
    "viem": "^2.0.0"
  },
  "devDependencies": {
    "typescript": "^5.0.0",
    "vitest": "^3.0.0"
  }
}
```

### 2. `sdk/tsconfig.json`
Standard TypeScript config targeting ES2022, module NodeNext, strict mode, outDir dist/.

### 3. `sdk/src/types.ts`
```typescript
export interface PricingInfo {
  amount: string;              // "0.005000"
  amount_base_units: number;   // 5000
  decimals: number;            // 6
  token: string;               // "0x...USDC"
  recipient: string;           // "0x...Provider"
  quote_id: string;            // "qt_a1b2c3d4"
  quote_expires_at: string;    // ISO 8601
  methods: string[];           // ["direct", "session"]
}

export interface PaymentRequiredResponse {
  error: string;               // "payment_required"
  message: string;             // actionable text
  help_url: string;            // docs URL
  pricing: PricingInfo;
}

export interface PaymentHeaders {
  'X-Payment-Tx': string;
  'X-Payment-Payer': string;
  'X-Payment-Quote-Id'?: string;
}

export interface ReceiptInfo {
  tx_hash: string;
  payer_address: string;
  amount: number;
  verified_at: number;
  status: string;
}

export interface PayGateClientOptions {
  /** Function that executes on-chain payment. Returns tx hash. */
  payFunction: (params: PaymentParams) => Promise<string>;
  /** Payer's wallet address */
  payerAddress: string;
  /** Max retries after payment (default: 1) */
  maxRetries?: number;
}

export interface PaymentParams {
  to: string;           // recipient address
  amount: bigint;       // amount in base units
  token: string;        // token contract address
  memo: string;         // bytes32 hex string
}
```

### 4. `sdk/src/hash.ts` — CRITICAL: Must match Rust exactly

```typescript
import { keccak256, toHex, toBytes } from 'viem';

/**
 * Compute requestHash = keccak256(method + " " + path + "\n" + body).
 *
 * This MUST produce identical output to paygate-common/src/hash.rs::request_hash().
 * Both use UTF-8 encoding of the concatenated string, then keccak256.
 */
export function requestHash(method: string, path: string, body: string | Uint8Array): `0x${string}` {
  // Build the input exactly as Rust does:
  // method bytes + " " + path bytes + "\n" + body bytes
  const methodBytes = new TextEncoder().encode(method);
  const spaceBytes = new Uint8Array([0x20]); // " "
  const pathBytes = new TextEncoder().encode(path);
  const newlineBytes = new Uint8Array([0x0a]); // "\n"
  const bodyBytes = typeof body === 'string' ? new TextEncoder().encode(body) : body;

  const input = new Uint8Array(
    methodBytes.length + 1 + pathBytes.length + 1 + bodyBytes.length
  );
  let offset = 0;
  input.set(methodBytes, offset); offset += methodBytes.length;
  input.set(spaceBytes, offset); offset += 1;
  input.set(pathBytes, offset); offset += pathBytes.length;
  input.set(newlineBytes, offset); offset += 1;
  input.set(bodyBytes, offset);

  return keccak256(input);
}

/**
 * Compute payment memo = keccak256("paygate" + quoteId + requestHash).
 *
 * requestHash is the raw 32-byte hash (not hex-encoded).
 */
export function paymentMemo(quoteId: string, reqHash: `0x${string}`): `0x${string}` {
  const prefixBytes = new TextEncoder().encode('paygate');
  const quoteBytes = new TextEncoder().encode(quoteId);
  const hashBytes = toBytes(reqHash); // 32 bytes

  const input = new Uint8Array(prefixBytes.length + quoteBytes.length + hashBytes.length);
  let offset = 0;
  input.set(prefixBytes, offset); offset += prefixBytes.length;
  input.set(quoteBytes, offset); offset += quoteBytes.length;
  input.set(hashBytes, offset);

  return keccak256(input);
}
```

**CRITICAL**: The Rust code in hash.rs does:
- `request_hash`: concatenates `method.as_bytes() + b" " + path.as_bytes() + b"\n" + body`
- `payment_memo`: concatenates `b"paygate" + quote_id.as_bytes() + request_hash.as_slice()`

Note that for payment_memo, the request_hash is the RAW 32 bytes, NOT the hex-encoded string. Make sure the TypeScript matches this.

### 5. `sdk/src/discovery.ts`
```typescript
import type { PricingInfo, PaymentRequiredResponse } from './types.js';

/**
 * Parse a 402 response into structured pricing info.
 */
export function parse402Response(response: Response): Promise<PaymentRequiredResponse>

/**
 * Check if a response is a 402 Payment Required.
 */
export function isPaymentRequired(response: Response): boolean

/**
 * Discover pricing for all endpoints by calling the API root.
 */
export async function getPricing(baseUrl: string): Promise<Record<string, PricingInfo>>
```

Parse both headers (X-Payment-*) and JSON body. The JSON body is authoritative per spec.

### 6. `sdk/src/client.ts`
```typescript
import { requestHash, paymentMemo } from './hash.js';
import { parse402Response, isPaymentRequired } from './discovery.js';
import type { PayGateClientOptions, PaymentParams } from './types.js';

export class PayGateClient {
  private payFunction: (params: PaymentParams) => Promise<string>;
  private payerAddress: string;
  private maxRetries: number;

  constructor(options: PayGateClientOptions) { ... }

  /**
   * Fetch a PayGate-protected URL. Handles 402 → pay → retry automatically.
   */
  async fetch(url: string, init?: RequestInit): Promise<Response> {
    // 1. Make the initial request
    // 2. If 402, parse pricing from response
    // 3. Compute requestHash from method + URL path + body
    // 4. Compute memo from quoteId + requestHash
    // 5. Call payFunction with {to, amount, token, memo}
    // 6. Retry with X-Payment-Tx, X-Payment-Payer, X-Payment-Quote-Id headers
    // 7. If still 402 after retry, throw error
    // 8. If not 402, return response directly (free endpoint)
  }

  /**
   * Discover pricing without paying.
   */
  async getPricing(baseUrl: string): Promise<Record<string, PricingInfo>> { ... }
}
```

### 7. `sdk/src/index.ts`
Export everything public:
```typescript
export { PayGateClient } from './client.js';
export { requestHash, paymentMemo } from './hash.js';
export { parse402Response, isPaymentRequired, getPricing } from './discovery.js';
export type * from './types.js';
```

### 8. `sdk/tests/hash.test.ts` — THE MOST IMPORTANT TEST
```typescript
import { describe, it, expect } from 'vitest';
import { requestHash, paymentMemo } from '../src/hash.js';
import vectors from '../../tests/fixtures/request_hash_vectors.json';

describe('requestHash cross-language parity', () => {
  for (const vector of vectors.request_hash_vectors) {
    it(`should match for: ${vector.description}`, () => {
      const hash = requestHash(vector.method, vector.path, vector.body);
      // Verify the input bytes match expected hex
      // The hash itself should be deterministic
      expect(hash).toMatch(/^0x[a-f0-9]{64}$/);

      // Verify input encoding matches the expected hex
      const encoder = new TextEncoder();
      const input = new Uint8Array([
        ...encoder.encode(vector.method),
        0x20, // space
        ...encoder.encode(vector.path),
        0x0a, // newline
        ...encoder.encode(vector.body),
      ]);
      const inputHex = Buffer.from(input).toString('hex');
      expect(inputHex).toBe(vector.input_hex);
    });
  }
});

describe('paymentMemo', () => {
  it('should produce deterministic output', () => {
    const rh = requestHash('POST', '/v1/chat/completions', '{"model":"gpt-4"}');
    const memo1 = paymentMemo('qt_abc123', rh);
    const memo2 = paymentMemo('qt_abc123', rh);
    expect(memo1).toBe(memo2);
  });

  it('should differ for different quotes', () => {
    const rh = requestHash('POST', '/v1/chat', '{}');
    const memo1 = paymentMemo('qt_abc', rh);
    const memo2 = paymentMemo('qt_def', rh);
    expect(memo1).not.toBe(memo2);
  });
});
```

### 9. `sdk/tests/client.test.ts`
Test the PayGateClient flow with mocked fetch:
1. Free endpoint (200 response) → passes through
2. Paid endpoint (402 → pay → 200) → auto-pay flow works
3. Payment failure → throws meaningful error
4. Retry exhaustion → throws after maxRetries

## Running Tests
```bash
cd sdk && npm install && npm test
```
Make sure ALL tests pass before committing.

## Commit
Commit with descriptive message on feat/ts-sdk branch.
