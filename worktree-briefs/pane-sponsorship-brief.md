# Build Brief: Fee Sponsorship E2E + SDK Auto-Session (Pane 2)

Two independent parts: verify existing sponsor.rs works E2E, then add auto-session to the TypeScript SDK.

## Part A: Fee Sponsorship E2E Test

### Goal

Prove that the existing `/paygate/sponsor` endpoint works end-to-end with Tempo's `withFeePayer` transport. The code in `sponsor.rs` already handles JSON-RPC proxying with budget tracking — this part writes an integration test.

### New File: `sdk/sponsor-e2e.mjs`

A standalone Node.js script (not part of the SDK build) that runs against the deployed demo gateway or a local instance.

```javascript
// Usage: TEMPO_PRIVATE_KEY=0x... GATEWAY_URL=http://localhost:8080 node sdk/sponsor-e2e.mjs

import { createClient, http, publicActions, walletActions } from 'viem';
import { Account, tempoActions, withFeePayer } from 'viem/tempo';
import { tempoModerato } from 'viem/chains';
```

Steps:
1. Read `TEMPO_PRIVATE_KEY` and `GATEWAY_URL` from env
2. Create a viem client with `withFeePayer` transport:
   ```javascript
   const account = Account.fromSecp256k1(process.env.TEMPO_PRIVATE_KEY);
   const client = createClient({
     account,
     chain: tempoModerato,
     transport: withFeePayer(
       http(),
       http(`${GATEWAY_URL}/paygate/sponsor`)
     ),
   }).extend(publicActions).extend(walletActions).extend(tempoActions());
   ```
3. Get initial native balance of the consumer
4. Send a TIP-20 `transferWithMemo`:
   ```javascript
   const PATHUSD = '0x20c0000000000000000000000000000000000000';
   const PROVIDER = '0x002925FAFE98cfeB9fdBb7d6045ce318E4BD4b88';
   const tx = await client.sendTransaction({
     to: PATHUSD,
     data: encodeFunctionData({
       abi: TIP20_ABI,
       functionName: 'transferWithMemo',
       args: [PROVIDER, 1000n, '0x' + '00'.repeat(32)],
     }),
     feePayer: true,
   });
   ```
5. Wait for receipt: `await client.waitForTransactionReceipt({ hash: tx })`
6. Get final native balance of the consumer
7. Assert:
   - Transaction confirmed (receipt.status === 'success')
   - Consumer's native balance did NOT decrease (gas was sponsored)
   - Print: "Fee sponsorship E2E: PASS"

Include a minimal TIP-20 ABI (just `transferWithMemo(address,uint256,bytes32)`) inline.

### Config Change: `demo/paygate.toml`

Add sponsorship section:
```toml
[sponsorship]
enabled = true
budget_per_day = "1.00"
max_per_tx = "0.01"
```

Note: The gateway wallet must have native Tempo tokens for gas. The wallet at `0x002925FAFE98cfeB9fdBb7d6045ce318E4BD4b88` should already have testnet tokens from previous testing. If not, use the Tempo faucet: `tempo_fundAddress` RPC method.

## Part B: SDK Auto-Session

### Goal

When `autoSession: true`, the PayGateClient automatically manages session lifecycle: creates sessions on first 402, uses HMAC auth for subsequent requests, and auto-renews when balance is exhausted.

### Modify: `sdk/src/client.ts`

Add session state to the class:

```typescript
export class PayGateClient {
  private payFunction: (params: PaymentParams) => Promise<string>;
  private payerAddress: string;
  private maxRetries: number;
  private autoSession: boolean;
  private sessionDeposit: string;

  // Session state
  private sessionId: string | null = null;
  private sessionSecret: string | null = null;
  private sessionBalance: number = 0;
  private sessionRatePerRequest: number = 0;
  private gatewayBaseUrl: string | null = null;
```

Update constructor:
```typescript
constructor(options: PayGateClientOptions) {
  this.payFunction = options.payFunction;
  this.payerAddress = options.payerAddress;
  this.maxRetries = options.maxRetries ?? 1;
  this.autoSession = options.autoSession ?? false;
  this.sessionDeposit = options.sessionDeposit ?? '0.10';
}
```

#### Updated `fetch()` flow:

```
1. If autoSession && has active session:
   → add HMAC headers, send request
   → if 402 with "insufficient_session_balance" or "session_expired_or_not_found":
     → invalidate session, fall through to create new session
   → else: return response

2. If autoSession && no active session:
   → send initial request (no payment)
   → if 402: create session, retry with HMAC

3. If !autoSession:
   → existing direct-payment logic (unchanged)
```

#### New private methods:

```typescript
private async createSession(gatewayUrl: string): Promise<void> {
  const baseUrl = new URL(gatewayUrl).origin;

  // Step 1: Get nonce
  const nonceResp = await fetch(`${baseUrl}/paygate/sessions/nonce`, {
    method: 'POST',
    headers: { 'X-Payment-Payer': this.payerAddress },
  });
  const { nonce } = await nonceResp.json();

  // Step 2: Send deposit
  // Compute memo: keccak256("paygate-session" || nonce)
  const memo = sessionMemo(nonce);
  const amount = parseUnits(this.sessionDeposit, 6); // USDC 6 decimals

  // Use the 402 response to get recipient + token
  // (cached from the initial 402 that triggered session creation)
  const txHash = await this.payFunction({
    to: this.cachedRecipient!,
    amount: BigInt(amount),
    token: this.cachedToken!,
    memo,
  });

  // Step 3: Create session
  const sessionResp = await fetch(`${baseUrl}/paygate/sessions`, {
    method: 'POST',
    headers: {
      'X-Payment-Tx': txHash,
      'X-Payment-Payer': this.payerAddress,
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ nonce }),
  });

  if (!sessionResp.ok) {
    throw new Error(`Session creation failed: ${sessionResp.status}`);
  }

  const session = await sessionResp.json();
  this.sessionId = session.sessionId;
  this.sessionSecret = session.sessionSecret;
  this.sessionBalance = parseFloat(session.balance) * 1_000_000;
  this.sessionRatePerRequest = parseFloat(session.ratePerRequest) * 1_000_000;
  this.gatewayBaseUrl = baseUrl;
}

private computeSessionHeaders(method: string, path: string, body: string): Record<string, string> {
  const timestamp = Math.floor(Date.now() / 1000).toString();
  const reqHash = requestHash(method, path, body);
  const sigPayload = reqHash + timestamp;
  const sig = hmacSha256(this.sessionSecret!, sigPayload);

  return {
    'X-Payment-Session': this.sessionId!,
    'X-Payment-Session-Sig': sig,
    'X-Payment-Timestamp': timestamp,
  };
}

private hasActiveSession(): boolean {
  return this.sessionId !== null && this.sessionBalance > this.sessionRatePerRequest;
}

private invalidateSession(): void {
  this.sessionId = null;
  this.sessionSecret = null;
  this.sessionBalance = 0;
}
```

#### New helper: `sessionMemo(nonce: string): string`

Add to `sdk/src/hash.ts`:
```typescript
export function sessionMemo(nonce: string): string {
  const input = new TextEncoder().encode('paygate-session' + nonce);
  return keccak256(input);
}
```

#### New helper: `hmacSha256(secret: string, message: string): string`

Add to `sdk/src/hash.ts`:
```typescript
import { createHmac } from 'crypto';

export function hmacSha256(secret: string, message: string): string {
  // Strip the ssec_ prefix for the raw secret
  const rawSecret = secret.startsWith('ssec_') ? secret.slice(5) : secret;
  return createHmac('sha256', Buffer.from(rawSecret, 'hex'))
    .update(message)
    .digest('hex');
}
```

### Modify: `sdk/src/types.ts`

Add session-related types:

```typescript
export interface PayGateClientOptions {
  payFunction: (params: PaymentParams) => Promise<string>;
  payerAddress: string;
  maxRetries?: number;
  /** Enable automatic session management */
  autoSession?: boolean;
  /** Deposit amount per session in USDC (default: "0.10") */
  sessionDeposit?: string;
}

export interface SessionNonceResponse {
  nonce: string;
  expiresAt: string;
}

export interface SessionCreateResponse {
  sessionId: string;
  sessionSecret: string;
  balance: string;
  ratePerRequest: string;
  expiresAt: string;
}

export interface SessionInfo {
  sessionId: string;
  balance: string;
  ratePerRequest: string;
  requestsMade: number;
  expiresAt: string;
  status: string;
}
```

### Tests: `sdk/tests/client.test.ts`

Add tests (using vitest or the existing test framework):

1. **Auto-session first call creates session**: Mock server returns 402 on first call. Client should call /sessions/nonce, payFunction, /sessions, then retry with HMAC headers. Assert HMAC headers present on retry.

2. **Auto-session subsequent call uses HMAC**: After session is created, second fetch() should NOT call payFunction again. Should send X-Payment-Session + X-Payment-Session-Sig + X-Payment-Timestamp headers directly.

3. **Session exhausted triggers auto-renew**: Mock server returns 402 with `insufficient_session_balance` error on a session-auth request. Client should create a new session and retry.

4. **Non-auto-session mode unchanged**: With `autoSession: false`, the existing direct-payment flow works exactly as before (no session headers).

5. **sessionMemo produces correct keccak256**: Verify `sessionMemo("nonce_abc123")` matches expected hash.

6. **hmacSha256 produces correct signature**: Verify against a known test vector.

## Source Files to Read Before Building

- `sdk/src/client.ts` — current PayGateClient (you are modifying this)
- `sdk/src/types.ts` — current types (you are extending this)
- `sdk/src/hash.ts` — keccak256 + paymentMemo (you are adding sessionMemo + hmacSha256)
- `sdk/src/discovery.ts` — parse402Response (you need the pricing info for session creation)
- `crates/paygate-gateway/src/sponsor.rs` — existing sponsor handler (Part A context)
- `crates/paygate-gateway/src/sessions.rs` — session endpoints your SDK calls (built by Pane 1)
- `demo/paygate.toml` — config you are modifying for sponsorship
- `SPEC.md` section 4.3 — session protocol
- `SPEC.md` section 4.4 — fee sponsorship flow
