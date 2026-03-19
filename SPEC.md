# PayGate: Micropayment-Gated API Gateway on Tempo

> Wrap any API or AI agent behind per-request stablecoin micropayments using the Tempo blockchain and Machine Payments Protocol (MPP).

## 1. Problem

APIs today are monetized via API keys + monthly billing. This creates friction (signup, billing, invoicing) and excludes a massive emerging class of consumers: **autonomous AI agents** that need to pay-per-call in real-time without human intervention.

Tempo's mainnet (launched March 18, 2026) provides sub-second finality, TIP-20 stablecoins with built-in memos, fee sponsorship, and the Machine Payments Protocol (MPP) co-authored with Stripe. PayGate exploits all of this to make any API instantly monetizable with zero-friction micropayments.

## 2. Solution Overview

PayGate is a **reverse proxy** that sits in front of any HTTP API. When a request arrives without payment, it returns `HTTP 402 Payment Required` with pricing headers describing the cost. The caller pays on-chain via a TIP-20 stablecoin transfer, then retries with the transaction hash. PayGate verifies the payment on-chain (< 100ms thanks to sub-second finality), then forwards the request to the upstream API.

```
Consumer            PayGate Proxy           Tempo Chain         Upstream API
   |                     |                       |                    |
   |-- GET /api -------->|                       |                    |
   |<-- 402 + pricing ---|                       |                    |
   |                     |                       |                    |
   |-- TIP-20 transfer --+-----> confirmed       |                    |
   |                     |                       |                    |
   |-- GET /api -------->|                       |                    |
   |   + X-Payment-Tx    |-- verify tx --------->|                    |
   |                     |<-- confirmed ---------|                    |
   |                     |                       |                    |
   |                     |-- forward ----------->|                    |
   |                     |<-- response ----------|                    |
   |<-- response --------|                       |                    |
```

## 3. Architecture

### 3.1 Components

| Component | Tech | Description |
|-----------|------|-------------|
| **Gateway** | Rust (axum + tower) | Reverse proxy with payment verification middleware |
| **Fee Payer Service** | Rust (built into gateway) | HTTP endpoint implementing Tempo's fee payer protocol for sponsoring caller gas |
| **Smart Contracts** | Solidity (Foundry) | Optional on-chain pricing registry and escrow |
| **Client SDK** | TypeScript + Rust | Payment-aware HTTP client that auto-pays |
| **Dashboard** | React + Wagmi/Tempo | Real-time revenue analytics (v0.3) |
| **CLI** | Rust (clap) | `paygate init`, `paygate serve`, `paygate revenue` |
| **Local DB** | SQLite (WAL mode) | Replay protection, session state, request logs |

### 3.2 Middleware Stack

Requests flow through these tower layers in order:

```
Request
  -> [Rate Limiter]       reject if over limit (429)
  -> [MPP Negotiator]     if no payment headers -> return 402 with pricing + quote ID
  |                        if price == 0 -> skip to [Header Sanitizer] (free endpoint)
  -> [Payment Verifier]   verify on-chain payment (decode TIP-20 Transfer logs) or session token
  -> [Payer Binder]       verify X-Payment-Payer matches on-chain sender
  -> [Header Sanitizer]   strip X-Payment-* headers before forwarding
  -> [Reverse Proxy]      forward to upstream
  -> [Response Logger]    log to SQLite, collect metrics, export Prometheus
  -> [Receipt Injector]   add X-Payment-Receipt + X-Payment-Cost headers
Response
```

## 4. Request Lifecycle

### 4.1 Price Discovery (402 Response)

When a request arrives without payment proof:

```http
HTTP/1.1 402 Payment Required

X-Payment-Required: true
X-Payment-Amount: 1000
X-Payment-Decimals: 6
X-Payment-Token: 0x...USDC
X-Payment-Recipient: 0x...Provider
X-Payment-Network: tempo-mainnet
X-Payment-Chain-Id: <TEMPO_CHAIN_ID>
X-Payment-Quote-Id: qt_a1b2c3d4
X-Payment-Quote-Expires: 1710799200
X-Payment-Methods: direct,session

{
  "error": "payment_required",
  "message": "Send 0.001000 USDC to 0x...Provider on Tempo, then retry with X-Payment-Tx header.",
  "help_url": "https://docs.paygate.dev/quickstart#paying",
  "pricing": {
    "amount": "0.001000",
    "amount_base_units": 1000,
    "decimals": 6,
    "token": "0x...USDC",
    "recipient": "0x...Provider",
    "quote_id": "qt_a1b2c3d4",
    "quote_expires_at": "2026-03-18T12:00:00Z",
    "methods": ["direct", "session"]
  }
}
```

**Canonical format**: The JSON body is the authoritative pricing source. Headers provide a machine-parseable summary. Amounts in headers are always base units (integer); amounts in the body include both decimal strings and base unit integers.

> **Note on intermediaries**: PayGate MUST receive the identical request body bytes that the client used to compute `requestHash`. Do not place body-modifying proxies (e.g., JSON reformatters, whitespace normalizers) between client and PayGate.

**Quote validity**: Each 402 response includes a `quote_id` and `quote_expires_at` (default TTL: 5 minutes). The gateway MUST honor the quoted price for any payment whose memo references this quote ID, even if the provider updates pricing before the quote expires. Quotes are stored in SQLite with a TTL-based cleanup.

> **Open question — MPP header format**: The `X-Payment-*` header names are PayGate-specific. The formal MPP wire protocol spec has not been published as of March 2026. If/when Tempo publishes the MPP spec, these headers should be updated to match. Track at: https://github.com/tempoxyz/tempo/issues (MPP spec). Until then, `tempo curl` compatibility is aspirational, not guaranteed.

> **Open question — Chain ID and RPC**: Tempo mainnet chain ID and RPC URL must be verified from [Tempo's official chain config](https://docs.tempo.xyz/) before implementation. The testnet chain is available as `tempoTestnet` from `viem/chains`. Config values shown are placeholders.

### 4.2 Direct Payment Flow

#### Request Hash Computation

The `requestHash` binds a payment to a specific API request. Both the gateway verifier and client SDK must compute it identically:

```
requestHash = keccak256(method || " " || path || "\n" || body)
```

- `method`: uppercase HTTP method (e.g., `POST`)
- `path`: request path including query string (e.g., `/v1/chat/completions?stream=true`)
- `body`: raw request body bytes (empty string for GET/DELETE)
- Encoding: UTF-8, no header canonicalization

Example: `keccak256("POST /v1/chat/completions\n{\"model\":\"gpt-4\"}")` -> `0xabc...`

The memo sent on-chain is: `keccak256("paygate" || quoteId || requestHash)` as `bytes32`. Inputs are UTF-8 encoded and concatenated as raw bytes before hashing. keccak256 output is already bytes32 (no truncation needed).

#### Flow

1. Consumer sends TIP-20 `transferWithMemo(to, amount, memo)` on Tempo
   - `to` = provider's address
   - `amount` >= quoted price
   - `memo` = `keccak256("paygate" || quoteId || requestHash)` as `bytes32`
2. Consumer retries original request with headers:
   ```
   X-Payment-Tx: 0xabc123...
   X-Payment-Payer: 0x9E2b...Consumer
   X-Payment-Quote-Id: qt_a1b2c3d4
   ```
3. Gateway verifies (< 100ms):
   - Fetch tx receipt from Tempo RPC using `eth_getTransactionReceipt`
   - If receipt is null (tx not yet indexed): return **400** with `Retry-After: 1` header and body `{"error": "Transaction not yet indexed, retry shortly"}`
   - If RPC returns an error (timeout, 5xx, network): return **503** with `Retry-After: 2` header
   - **Decode TIP-20 Transfer event logs** from the receipt (NOT the top-level tx `to` field, which is the token contract address). Match exactly one `Transfer(from, to, amount)` event where `to == provider_address` and the token contract matches `accepted_token`
   - Verify `TransferWithMemo` log contains expected memo bytes32
   - Verify amount >= price (using quoted price if `quote_id` is provided and quote has not expired)
   - Verify `X-Payment-Payer` matches the `from` address in the Transfer event (prevents front-running: only the actual payer can redeem)
   - Verify tx not already consumed (SQLite UNIQUE check on `tx_hash`)
   - Verify tx is **finalized** (Tempo uses Simplex Consensus with single-slot finality; a tx in a receipt is final)
   - Verify tx age < `tx_expiry_seconds` (default 300s)
   - Reject transactions with multiple matching Transfer events (ambiguous)
4. Gateway forwards request to upstream, returns response with `X-Payment-Receipt` header

### 4.3 Session Flow (Pay-as-you-go)

For high-frequency callers (AI agents making hundreds of calls):

1. Consumer requests a session nonce:
   ```
   POST /paygate/sessions/nonce
   X-Payment-Payer: 0x9E2b...Consumer
   ```
   Gateway returns: `{ "nonce": "nonce_abc123", "expiresAt": "..." }`

2. Consumer deposits with memo containing the nonce:
   ```
   TIP-20.transferWithMemo(provider, amount, keccak256("paygate-session" || nonce))
   ```

3. Consumer creates session:
   ```
   POST /paygate/sessions
   X-Payment-Tx: 0xdef456...
   X-Payment-Payer: 0x9E2b...Consumer
   ```
   Gateway verifies: deposit tx memo contains the server-issued nonce, and `from` address matches `X-Payment-Payer`. This prevents front-running of deposit claims.

4. Gateway returns session:
   ```json
   {
     "sessionId": "sess_<256-bit-hex>",
     "sessionSecret": "ssec_<256-bit-hex>",
     "balance": "0.050000",
     "ratePerRequest": "0.000500",
     "expiresAt": "2026-03-18T12:00:00Z"
   }
   ```
   The `sessionSecret` is a high-entropy bearer token returned only once. The `sessionId` is public.

5. Subsequent requests use HMAC authentication:
   ```
   GET /api/v1/chat/completions
   X-Payment-Session: sess_<id>
   X-Payment-Session-Sig: HMAC-SHA256(sessionSecret, requestHash || timestamp)
   X-Payment-Timestamp: 1710799200
   ```

6. Gateway verifies HMAC using **constant-time comparison** (to prevent timing attacks), checks timestamp freshness (< 60s), then **atomically** deducts from session balance:
   ```sql
   UPDATE sessions SET balance = balance - rate_per_request, requests_made = requests_made + 1
   WHERE id = ? AND balance >= rate_per_request AND status = 'active' AND expires_at > ?;
   ```
   If zero rows updated: return 402 with balance info (insufficient funds or expired).

7. Refund requires payer signature:
   ```
   POST /paygate/sessions/{id}/refund
   X-Payment-Payer: 0x9E2b...
   X-Payment-Sig: <EIP-191 signature of "refund:{sessionId}">
   ```

### 4.4 Fee Sponsorship Flow

When the API provider opts to pay caller gas fees (frictionless UX):

1. Provider sets `sponsorship.enabled = true` in config
2. PayGate runs a **fee payer HTTP service** at `/paygate/sponsor` that implements the Tempo fee payer protocol
3. Consumer configures their viem client with PayGate as the fee payer:
   ```typescript
   import { withFeePayer } from 'viem/tempo'
   const client = createWalletClient({
     transport: withFeePayer(
       http(),                                    // default transport
       http('https://paygate-host/paygate/sponsor') // fee payer transport
     ),
   })
   ```
4. When the consumer sends a TIP-20 transfer with `feePayer: true`, the transaction fee is routed to PayGate's sponsor endpoint, which signs and relays it using the gateway's Tempo account
5. Provider's sponsorship budget is tracked and enforced per-day and per-transaction

## 5. Configuration

### 5.1 Full Config (`paygate.toml`)

```toml
[gateway]
listen = "0.0.0.0:8080"
admin_listen = "127.0.0.1:8081"
upstream = "http://localhost:3000"
upstream_timeout_seconds = 30            # timeout for upstream forwarding
max_response_body_bytes = 10485760       # 10MB — response body size limit (prevents OOM from upstream)

[tempo]
network = "testnet"                      # "testnet" or "mainnet" — controls chain defaults
rpc_urls = ["https://rpc.tempo.xyz", "https://rpc2.tempo.xyz"]  # failover list
failover_timeout_ms = 2000               # switch to next RPC URL after this timeout
rpc_pool_max_idle = 10                   # HTTP connection pool size for RPC
rpc_timeout_ms = 5000                    # per-request timeout for RPC calls
chain_id = 0                             # PLACEHOLDER — verify from viem/chains
private_key_env = "PAYGATE_PRIVATE_KEY"  # env var, never in config
# MVP: exactly one accepted token. Multi-token with FX rules deferred to v0.2.
accepted_token = "0x...USDC"             # TIP-20 token address (mainnet TBD)

[provider]
address = "0x7F3a...Provider"
name = "My AI API"
description = "GPT-4 wrapper with custom tools"

[sponsorship]
enabled = false
sponsor_listen = "/paygate/sponsor"      # fee payer endpoint path
budget_per_day = "10.00"
max_per_tx = "0.01"

[sessions]
enabled = true
discount_percent = 50
minimum_deposit = "0.05"
max_duration_hours = 24
auto_refund = true
max_concurrent_per_payer = 5

[pricing]
default_price = "0.001"          # USDC per request
quote_ttl_seconds = 300          # how long a 402 quote is honored

[pricing.endpoints]
"POST /v1/chat/completions" = "0.005"
"GET /v1/models" = "0.000"       # free
"POST /v1/embeddings" = "0.001"

[pricing.dynamic]
# NOTE: Dynamic pricing requires sessions or escrow, because the final
# price is only known after the upstream responds. It is NOT compatible
# with the direct prepay flow. Deferred to v0.2.
enabled = false
token_price = "0.00001"          # per output token (for LLM APIs)
compute_price = "0.001"          # per second of compute
header_source = "X-Token-Count"  # upstream header with token count

[pricing.tiers]
"100" = "0.004"                  # after 100 req/day: $0.004 each
"1000" = "0.003"
"10000" = "0.002"

[rate_limiting]
requests_per_second = 100
per_payer_per_second = 10
min_payment_interval_ms = 100

[security]
require_payment_before_forward = true
max_request_body_bytes = 10485760
tx_expiry_seconds = 300
replay_protection = true

[webhooks]
payment_verified_url = ""                # POST on every verified payment (fire-and-forget)
timeout_seconds = 5                      # webhook delivery timeout
# URL validation: reject private IPs, localhost, link-local, non-HTTPS at config load

[storage]
request_log_retention_days = 30          # TTL for request_log entries
```

### 5.2 Minimal Config (3 fields)

```toml
[gateway]
upstream = "http://localhost:3000"

[tempo]
rpc_urls = ["https://rpc.tempo.xyz"]

[provider]
address = "0x7F3a...Provider"
```

Everything else uses sensible defaults (price $0.001, single accepted token auto-detected, standard rate limits, `network = "testnet"`).

### 5.3 Config Reload

Config is held in `Arc<ArcSwap<Config>>` (or equivalent atomic swap). On `SIGHUP` or config file change, the new config is validated before swapping. Invalid new config is rejected (log error, keep old config). Config changes are logged with a diff summary.

### 5.4 Config Validation at Startup

All config fields are validated at startup with clear error messages naming the specific field and issue:
- **Addresses**: valid 40-char hex with `0x` prefix
- **Prices**: non-negative decimal strings
- **URLs**: valid format; webhook URLs must not be private/localhost/link-local
- **Required fields**: `upstream`, `rpc_urls` (at least one), `provider.address`

## 6. Smart Contracts

### 6.1 PayGateRegistry.sol (Optional — for on-chain service discovery)

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ITIP20} from "./interfaces/ITIP20.sol";

contract PayGateRegistry {
    struct Service {
        address provider;
        uint256 pricePerRequest;    // stablecoin base units (6 decimals)
        address acceptedToken;
        bool active;
        string metadataUri;         // pricing manifest URL
    }

    mapping(bytes32 => Service) public services;

    event ServiceRegistered(bytes32 indexed serviceId, address indexed provider, uint256 price);

    function registerService(
        string calldata name,
        uint256 pricePerRequest,
        address acceptedToken,
        string calldata metadataUri
    ) external returns (bytes32 serviceId) {
        serviceId = keccak256(abi.encodePacked(msg.sender, name));
        services[serviceId] = Service({
            provider: msg.sender,
            pricePerRequest: pricePerRequest,
            acceptedToken: acceptedToken,
            active: true,
            metadataUri: metadataUri
        });
        emit ServiceRegistered(serviceId, msg.sender, pricePerRequest);
    }

    function updatePrice(bytes32 serviceId, uint256 newPrice) external {
        require(services[serviceId].provider == msg.sender, "Not provider");
        services[serviceId].pricePerRequest = newPrice;
    }

    function deactivate(bytes32 serviceId) external {
        require(services[serviceId].provider == msg.sender, "Not provider");
        services[serviceId].active = false;
    }
}
```

### 6.2 PayGateEscrow.sol (Optional — for refund-eligible payments, v0.2)

Deferred to Wave 2. Design requirements:
- Gateway is the authorized releaser (signs release after successful upstream response)
- Provider can release manually as fallback
- Payer can claim refund unilaterally after escrow expiry
- Access control: `release()` callable only by gateway or provider; `refund()` callable only by payer after `expiresAt`

### 6.3 MVP Approach: No Contract Required

For MVP, PayGate verifies payments by reading Tempo RPC directly:
- Consumer sends a standard TIP-20 `transferWithMemo` to the provider's address
- Gateway decodes the `Transfer` and `TransferWithMemo` event logs from the transaction receipt
- No custom smart contract needed

The registry and escrow contracts are optional add-ons for discoverability and refund safety.

## 7. Client SDK

### 7.1 TypeScript

Uses the actual Tempo SDK (`viem/tempo`) for wallet and transaction management:

```typescript
import { PayGateClient } from '@paygate/sdk';
import { createClient, http, publicActions, walletActions } from 'viem';
import { Account, tempoActions } from 'viem/tempo';
import { tempoTestnet } from 'viem/chains'; // or tempoMainnet when available

// Create a Tempo wallet using viem/tempo
const account = Account.fromSecp256k1(process.env.TEMPO_PRIVATE_KEY!);
// Or for passkey-based access keys:
// const keyPair = await WebCryptoP256.createKeyPair();
// const account = Account.fromWebCryptoP256(keyPair, { access: parentAccount });

const tempoClient = createClient({
  account,
  chain: tempoTestnet,
  transport: http(),
})
  .extend(publicActions)
  .extend(walletActions)
  .extend(tempoActions());

// Wrap in PayGateClient for auto-pay behavior
const client = new PayGateClient({
  tempoClient,
  autoSession: true,        // auto-create sessions for high-frequency use
  sessionDeposit: '0.10',   // $0.10 USDC per session
});

// Use like a normal fetch — payment happens automatically
const response = await client.fetch(
  'https://api.example.com/v1/chat/completions',
  {
    method: 'POST',
    body: JSON.stringify({ model: 'gpt-4', messages: [...] }),
  }
);

// Discover pricing without paying
const pricing = await client.getPricing('https://api.example.com');
// => { "POST /v1/chat/completions": { price: "0.005", token: "USDC" } }
```

### 7.2 Rust

```rust
use paygate_client::PayGateClient;
use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;

let signer = std::env::var("TEMPO_PRIVATE_KEY")?.parse::<PrivateKeySigner>()?;
let client = PayGateClient::new(signer, "https://rpc.tempo.xyz");

let response = client
    .post("https://api.example.com/v1/chat/completions")
    .json(&body)
    .send()
    .await?;
```

### 7.3 Agent Tool Pattern

```typescript
// Define as a tool for any AI agent framework
const paidApiTool = {
  name: 'call_paid_api',
  description: 'Call a micropayment-gated API. Payment is handled automatically.',
  parameters: { url: 'string', method: 'string', body: 'object' },
  execute: async ({ url, method, body }) => {
    return await payGateClient.fetch(url, { method, body: JSON.stringify(body) });
  },
};
```

### 7.4 Tempo CLI Compatibility

Tempo's `tempo curl` is documented as handling MPP 402 negotiation automatically. **Note**: Until the formal MPP wire spec is published and PayGate's headers are confirmed compatible, this is aspirational:

```bash
tempo curl https://api.example.com/v1/chat/completions \
  --data '{"model":"gpt-4","messages":[...]}'
# Expected: auto-discovers price, pays, retries
```

## 8. Data Model (SQLite)

SQLite is configured with `PRAGMA journal_mode=WAL` for concurrent readers with a single writer. All writes go through a dedicated writer task via a **bounded** tokio mpsc channel (default capacity: 10,000) to avoid `SQLITE_BUSY` under load. When the channel is full, new write requests return 503 to the caller — this prevents unbounded memory growth under extreme load.

The writer task batches INSERTs in a transaction, flushing every 10ms or 50 writes, whichever comes first. SQLite write throughput: ~50,000/sec with batching. End-to-end verification throughput is limited by RPC latency (~1,000/sec with 50 concurrent connections). Multi-instance deployments (v0.3) will migrate to PostgreSQL.

A periodic cleanup task runs every 5 minutes to remove expired quotes: `DELETE FROM quotes WHERE expires_at < now() - 3600`. Request log entries are cleaned up every hour based on the configured retention period (default 30 days): `DELETE FROM request_log WHERE created_at < strftime('%s', 'now', '-N days')`.

```sql
PRAGMA journal_mode = WAL;

CREATE TABLE payments (
    id              TEXT PRIMARY KEY,
    tx_hash         TEXT UNIQUE NOT NULL,
    payer_address   TEXT NOT NULL,
    amount          INTEGER NOT NULL,          -- base units
    token_address   TEXT NOT NULL,
    endpoint        TEXT NOT NULL,
    request_hash    TEXT,
    quote_id        TEXT,                      -- FK to quotes.id
    block_number    INTEGER NOT NULL,
    verified_at     INTEGER NOT NULL,
    status          TEXT NOT NULL DEFAULT 'verified'
);
CREATE INDEX idx_payments_payer ON payments(payer_address);

CREATE TABLE quotes (
    id              TEXT PRIMARY KEY,          -- qt_<random>
    endpoint        TEXT NOT NULL,
    price           INTEGER NOT NULL,          -- base units
    token_address   TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    expires_at      INTEGER NOT NULL
);
CREATE INDEX idx_quotes_expires ON quotes(expires_at);

CREATE TABLE sessions (
    id              TEXT PRIMARY KEY,          -- sess_<256-bit-hex>
    secret          TEXT NOT NULL,             -- server-issued, used for HMAC verification
    payer_address   TEXT NOT NULL,
    deposit_tx      TEXT NOT NULL,
    nonce           TEXT NOT NULL,             -- server-issued nonce from /sessions/nonce
    deposit_amount  INTEGER NOT NULL,
    balance         INTEGER NOT NULL,
    rate_per_request INTEGER NOT NULL,
    requests_made   INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL,
    expires_at      INTEGER NOT NULL,
    status          TEXT NOT NULL DEFAULT 'active'
);
CREATE INDEX idx_sessions_payer ON sessions(payer_address);

CREATE TABLE request_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    payment_id      TEXT,
    session_id      TEXT,
    endpoint        TEXT NOT NULL,
    payer_address   TEXT NOT NULL,
    amount_charged  INTEGER NOT NULL,
    upstream_status INTEGER,
    upstream_latency_ms INTEGER,
    created_at      INTEGER NOT NULL
);
CREATE INDEX idx_request_log_created ON request_log(created_at);
CREATE INDEX idx_request_log_payer ON request_log(payer_address);
```

Note: FK constraints are intentionally omitted for write performance (standard SQLite pattern for high-throughput systems).

## 9. CLI

```
paygate init              Interactive setup wizard -> generates paygate.toml
paygate serve             Start the gateway proxy
paygate status            Show gateway status and RPC connectivity
paygate pricing           Display current pricing table
paygate pricing --html    Generate static HTML pricing page
paygate sessions          List active payment sessions
paygate revenue           Revenue summary (24h, 7d, 30d)
paygate wallet            Show provider on-chain balance + 24h income summary
paygate demo              Spin up demo echo server + testnet wallet, run full payment cycle
paygate test              End-to-end test against Tempo testnet (see below)
paygate contract deploy   Deploy PayGateRegistry to Tempo
paygate contract register Register service in on-chain registry
```

### 9.1 CLI Output Conventions

All CLI output follows the nginx/caddy philosophy — clean, quiet, let the logs speak.

- **Indent**: 2-space indent for all output blocks
- **Section headers**: text + `───` underline
- **Errors**: `error: <message>` + indented `hint: <fix>` (Rust compiler style)
- **Timestamps**: ISO 8601 in request logs
- **Status markers**: `✓` (pass), `✗` (fail)
- **Color**: No emoji. No color by default. Respect `NO_COLOR` env var.
- **Monetary amounts**: `$X.XX` format (USD equivalent)
- **Wallet addresses**: truncated `0x7F3a...Provider`
- **Terminal width**: degrade gracefully in narrow terminals

### 9.2 CLI Output Specifications

#### `paygate serve`

```
$ paygate serve

  PayGate v0.1.0
  Proxy: 0.0.0.0:8080 → localhost:3000
  Tempo: rpc.tempo.xyz (connected)

  Ready. Accepting payments.

2026-03-18T12:00:01Z  POST /v1/chat  402  →  0.005 USDC
2026-03-18T12:00:03Z  POST /v1/chat  200  ←  0.005 USDC  tx:0xab..cd  47ms
```

**Error states:**

```
$ paygate serve

  PayGate v0.1.0

  error: Tempo RPC unreachable
    rpc_url = "https://rpc.tempo.xyz"
    hint: check your network or verify the URL in paygate.toml

$ paygate serve

  error: port 8080 already in use
    hint: set gateway.listen in paygate.toml or kill the existing process

$ paygate serve

  error: config not found
    hint: run `paygate init` to create paygate.toml
```

#### `paygate init`

3-question minimal wizard. Ask only what can't be inferred.

```
$ paygate init

  PayGate Setup
  ──────────────

  Upstream API URL: http://localhost:3000
  Provider wallet address: 0x7F3a...
  Private key env var [PAYGATE_PRIVATE_KEY]:

  Created paygate.toml
  Default price: $0.001/request (edit paygate.toml to customize)

  Next steps:
    export PAYGATE_PRIVATE_KEY=<your-tempo-private-key>
    paygate serve
    paygate test    # verify on testnet
```

**Error states:**
- Invalid URL → `error: invalid URL` + `hint: include the scheme (http:// or https://)`
- Invalid address → `error: invalid Ethereum address` + `hint: must start with 0x and be 42 characters`
- Config already exists → show diff, prompt for confirmation; use `--force` to overwrite non-interactively

#### `paygate revenue`

```
$ paygate revenue

  Revenue Summary
  ───────────────
  24h     $12.45   2,490 requests
   7d     $67.30  13,460 requests
  30d    $245.10  49,020 requests

  Top endpoints (24h):
    POST /v1/chat/completions   $10.20  (2,040 req)
    POST /v1/embeddings          $2.15    (430 req)
    GET  /v1/models              $0.00     (20 req)  free
```

**Empty state:**
```
$ paygate revenue

  Revenue Summary
  ───────────────
  No payments recorded yet.

  hint: run `paygate test` to verify your setup, or send a request to your gateway
```

#### `paygate test`

```
$ paygate test

  PayGate end-to-end test (tempo-testnet)
  ─────────────────────────────────────
  Starting echo server on :9999
  Starting gateway on :8080 → :9999

  [1/6] Request without payment     402 ✓
  [2/6] Fund test wallet            0.01 USDC ✓
  [3/6] Pay and retry               200 ✓  (47ms verify)
  [4/6] Replay same tx              402 ✓
  [5/6] Wrong payer address         402 ✓
  [6/6] Insufficient amount         402 ✓

  All tests passed. Verification latency: 47ms p50, 62ms p99
```

**Failure state:**
```
  [1/6] Request without payment     402 ✓
  [2/6] Fund test wallet            0.01 USDC ✓
  [3/6] Pay and retry               FAIL ✗
    expected: 200, got: 402
    tx_hash: 0xabc...def
    hint: check that the gateway can reach Tempo RPC

  1 of 6 tests failed.
```

#### `paygate demo`

Runs a self-contained demo of the full payment cycle:
1. Starts a built-in echo server as the upstream
2. Requires `PAYGATE_TEST_KEY` env var (Tempo testnet private key) or prompts interactively
3. Requests testnet tokens from the Tempo faucet (`tempo_fundAddress`)
4. Sends a test TIP-20 `transferWithMemo` on testnet
5. Verifies payment through the gateway
6. Prints results with timing statistics

#### `paygate status`

```
$ paygate status

  PayGate Status
  ──────────────
  Gateway    running  0.0.0.0:8080
  Upstream   healthy  localhost:3000
  Tempo RPC  connected  rpc.tempo.xyz
  DB         ok  paygate.db (1.2 MB)
  Uptime     4h 23m
  Requests   2,490 (24h)
  Revenue    $12.45 (24h)
```

**Degraded states:**
```
  Upstream   unreachable  localhost:3000
  Tempo RPC  error  rpc.tempo.xyz (timeout)
```

#### `paygate sessions`

```
$ paygate sessions

  Active Sessions
  ───────────────
  ID            Payer          Balance    Requests  Expires
  sess_a1b2..   0x9E2b...Con   $0.032     36        2h 15m
  sess_c3d4..   0x4F1a...Bot   $0.008     84        45m

  2 active sessions, $0.040 total balance
```

**Empty state:**
```
$ paygate sessions

  No active sessions.
```

#### `paygate pricing`

```
$ paygate pricing

  Pricing Table
  ─────────────
  Endpoint                       Price
  POST /v1/chat/completions      $0.005
  POST /v1/embeddings            $0.001
  GET  /v1/models                free
  *  (default)                   $0.001
```

#### `paygate wallet`

Queries Tempo RPC for the provider's token balance and queries SQLite for recent revenue. Displays on-chain balance and 24h income summary.

#### `paygate pricing --html`

Generates a static HTML file with: endpoint pricing table, accepted token, provider address, and example `curl` commands.

### 9.3 Interaction State Coverage

| Command | Loading | Empty | Error | Success | Partial |
|---------|---------|-------|-------|---------|---------|
| `paygate serve` startup | N/A | N/A | `error:` + `hint:` | startup block | N/A |
| `paygate revenue` | N/A | "No payments recorded" + hint | DB error + hint | summary table | N/A |
| `paygate test` | step-by-step progress | N/A | per-step FAIL with details | all passed + latency | partial pass count |
| `paygate init` | N/A | N/A | per-field validation + hint | created + next steps | N/A |
| `paygate status` | N/A | N/A | per-component degraded | all healthy | mixed healthy/degraded |
| `paygate sessions` | N/A | "No active sessions" | DB error | session table | N/A |
| `paygate wallet` | N/A | "Balance: $0.00" | RPC error + hint | balance + income | N/A |
| 402 response | N/A | N/A | N/A | message + help_url + pricing | N/A |
| Health endpoint | N/A | N/A | per-component status | all healthy JSON | degraded JSON |

### 9.4 Developer Experience: Three Commands

```bash
cargo install paygate           # or: brew install paygate
paygate init                    # interactive wizard (if paygate.toml exists, shows diff and prompts; use --force to overwrite)
paygate serve                   # proxy is live
```

## 10. Security

### 10.1 Payment Security

| Threat | Mitigation |
|--------|-----------|
| **Replay attack** | `tx_hash` has UNIQUE constraint in SQLite. A tx pays for exactly one request. |
| **Front-running** | `X-Payment-Payer` must match the `from` address in the on-chain Transfer event. Only the actual payer can redeem their payment. |
| **Stale transaction** | Tx must be < `tx_expiry_seconds` old (default 300s) |
| **Wrong amount** | Gateway verifies on-chain amount >= endpoint price (or quoted price if quote_id provided) |
| **Wrong recipient** | Gateway decodes Transfer event logs and verifies `to` matches provider address (NOT the top-level tx `to`, which is the token contract) |
| **Endpoint mismatch** | Memo = `keccak256("paygate" \|\| quoteId \|\| requestHash)` binding payment to specific request |
| **Key exposure** | Private key read from env var, never stored in config |
| **Ambiguous transactions** | Reject any transaction with multiple matching Transfer events |
| **Finality** | Tempo uses Simplex Consensus with single-slot finality. A transaction in a receipt is irreversible. No reorg risk. |
| **Webhook SSRF** | `webhook_url` validated at config load: reject private IPs (10.x, 172.16-31.x, 192.168.x), localhost, link-local (169.254.x), and non-HTTPS schemes |

### 10.2 Quote Security

| Threat | Mitigation |
|--------|-----------|
| **Price change after payment** | Quotes have a TTL (default 5 min). Gateway honors the quoted price within the TTL window. |
| **Quote replay** | Quote IDs are single-use — consumed only after successful payment verification (not on reference, to avoid wasting quotes on failed verifications) |
| **Expired quote** | Payment with expired quote_id is verified against current price instead |

### 10.3 DDoS / Abuse

| Threat | Mitigation |
|--------|-----------|
| **Payment flood** | Per-payer rate limit (10 req/s), min interval (100ms) |
| **402 bombardment** | IP-based rate limit on 402 responses (1000/min) |
| **Session abuse** | Max 5 concurrent sessions per payer |
| **Large payloads** | Request body size limit (10MB default) |
| **Economic DDoS** | Attacker pays real money per request — self-limiting |

### 10.4 Upstream Protection

- Requests are **never** forwarded without verified payment (unless endpoint is free)
- All `X-Payment-*` headers are stripped before forwarding
- Upstream sees a normal HTTP request

### 10.5 Refund Policy

| Mode | Failed upstream (5xx) | Behavior |
|------|----------------------|----------|
| Direct | No auto-refund | Consumer can dispute off-chain. Payment was for the attempt, not the outcome. |
| Escrow | Escrow not released | Auto-refund after expiry |
| Session | **Balance IS deducted** | The request was forwarded and may have caused side effects. For idempotent endpoints, provider can configure `no_charge_on_5xx = true` per endpoint to waive the charge. Default: charge. |

## 11. Edge Cases

| Scenario | Handling |
|----------|---------|
| **Tempo RPC unreachable** | Return 503 with `Retry-After: 2` header. Active sessions continue working. Health endpoint reports status. |
| **RPC timeout during verification** | Return 503 with `Retry-After: 2` header. Consumer may retry same request with same `X-Payment-Tx`. Verification is idempotent — `tx_hash` not consumed until verification succeeds. |
| **Null/empty receipt from RPC** | Return 400 with `Retry-After: 1` header and body `{"error": "Transaction not yet indexed, retry shortly"}`. This is a timing issue, not a payment issue. |
| **Partial payment** | Return 402 with `X-Payment-Shortfall`. No partial top-ups in MVP. |
| **Payment verified but upstream fails** | Direct: return 502 + receipt (no refund). Session: deduct by default (see 10.5). Escrow: hold funds. |
| **Gateway restart mid-session** | Sessions persisted in SQLite (secret stored in DB), reloaded on startup. |
| **Price change during quoted 402** | If consumer provides valid `quote_id`, gateway honors the quoted price. If quote expired, re-verify against current price. |
| **Double-spend across instances** | MVP is single-instance. Multi-instance requires shared dedup store (v0.3). |
| **Graceful shutdown** | On SIGTERM: stop accepting new connections, drain in-flight requests (30s timeout), persist session state, exit. |
| **Malformed event logs** | Decode failure returns 402 with descriptive error (never panic). |
| **SQLite write failure** | Disk full or I/O error returns 503 to pending verifications. |
| **Upstream response too large** | Response body exceeding `max_response_body_bytes` is truncated and returns 502. |

## 12. Observability

### 12.1 Health Check

`GET /paygate/health` returns:
```json
{
  "status": "healthy",
  "tempo_rpc": "connected",
  "upstream": "reachable",
  "active_sessions": 12,
  "db": "ok"
}
```

### 12.2 Prometheus Metrics

`GET /paygate/metrics` exports:
- `paygate_payments_verified_total` (counter, labels: endpoint, status)
- `paygate_payment_verification_duration_seconds` (histogram)
- `paygate_upstream_request_duration_seconds` (histogram, labels: endpoint, status_code)
- `paygate_revenue_total_base_units` (counter, labels: token)
- `paygate_active_sessions` (gauge)
- `paygate_rate_limit_rejected_total` (counter)
- `paygate_rpc_errors_total` (counter)
- `paygate_db_errors_total` (counter) — SQLite operation failures
- `paygate_db_writer_queue_depth` (gauge) — current mpsc channel depth
- `paygate_webhook_delivery_total` (counter, labels: status=success/failure/timeout)
- `paygate_quotes_active` (gauge) — unexpired quotes in DB
- `paygate_config_reloads_total` (counter, labels: status=success/failure)

### 12.3 Structured Logging

JSON-formatted logs to stdout. All payment events are logged with: `tx_hash`, `payer_address`, `endpoint`, `amount`, `verification_result`, `verification_step`, `latency_ms`. **No secrets or private keys are ever logged.**

The `verification_step` field indicates which step of payment verification the log pertains to (e.g., `receipt_fetch`, `event_decode`, `memo_check`, `amount_check`, `replay_check`, `payer_binding`, `tx_age_check`). Example:
```json
{
  "event": "payment_verification",
  "tx_hash": "0x...",
  "payer": "0x...",
  "endpoint": "POST /v1/chat/completions",
  "verification_step": "amount_check",
  "result": "insufficient",
  "expected": 5000,
  "actual": 1000,
  "latency_ms": 42
}
```

### 12.4 Receipt Verification Endpoint

```
GET /paygate/receipts/{tx_hash}
```

Returns: `{ tx_hash, payer_address, amount, endpoint, verified_at, status }` or 404 if not found.

Public endpoint on main port. Rate-limited (100 req/min per IP).

## 13. Project Structure

```
paygate/
  Cargo.toml                      # workspace root
  paygate.toml.example            # example config
  SPEC.md                         # this file
  crates/
    paygate-gateway/              # main binary
      src/
        main.rs                   # CLI (clap)
        config.rs                 # TOML parsing
        server.rs                 # axum setup
        proxy.rs                  # reverse proxy
        mpp.rs                    # 402 response generation + quote management
        verifier.rs               # on-chain payment verification (TIP-20 event log decoding)
        sessions.rs               # session management (nonce, HMAC, atomic debit)
        db.rs                     # SQLite operations (WAL mode, writer task)
        rate_limit.rs             # rate limiting
        admin.rs                  # admin API + health + metrics
        sponsor.rs                # fee payer HTTP service (Tempo withFeePayer protocol)
        metrics.rs                # Prometheus metrics
    paygate-common/               # shared types
      src/
        lib.rs
        types.rs                  # Amount, PaymentProof, VerificationResult, RequestHash
        mpp.rs                    # header constants and parsing
        hash.rs                   # requestHash computation (keccak256)
    paygate-client/               # Rust client SDK
      src/
        lib.rs
        client.rs
        discovery.rs
        session.rs
  contracts/                      # Foundry project
    src/
      PayGateRegistry.sol
      interfaces/ITIP20.sol
    test/
      PayGateRegistry.t.sol
    foundry.toml
  sdk/                            # TypeScript client SDK
    src/
      index.ts
      client.ts                   # PayGateClient wrapping viem/tempo
      types.ts
      discovery.ts
      hash.ts                     # requestHash computation (must match gateway)
    package.json
  dashboard/                      # React dashboard (v0.3)
    src/
      App.tsx
    package.json
```

## 14. Implementation Waves

### Wave 1: MVP (v0.1) — "It Works"

**In scope:**
- Single Rust binary (`paygate serve`)
- TOML config with static per-endpoint pricing, single accepted token
- Config validation at startup, config reload via SIGHUP
- 402 responses with quote IDs and TTL (includes `message` and `help_url` fields)
- On-chain payment verification via Tempo RPC (decode TIP-20 Transfer event logs)
- RPC failover (`rpc_urls` array) with connection pooling
- Payer binding (`X-Payment-Payer` must match on-chain `from`)
- Request hash computation (`keccak256(method || path || body)`)
- Replay protection (SQLite, WAL mode, bounded writer task with backpressure)
- Basic rate limiting
- `paygate init` wizard
- `paygate demo` — self-contained demo with echo server + testnet payment cycle
- `paygate pricing --html` — static HTML pricing page generator
- `paygate wallet` — provider on-chain balance + 24h income summary
- TypeScript client SDK with auto-pay (using `viem/tempo`)
- `paygate revenue` CLI
- `paygate test` end-to-end testnet verification
- Free-endpoint passthrough
- Request logging with configurable retention
- Receipt verification endpoint (`GET /paygate/receipts/{tx_hash}`)
- `X-Payment-Cost` response header (amount charged for this request)
- Webhook on payment verified (fire-and-forget, SSRF-safe)
- Health check endpoint
- Prometheus metrics endpoint (including DB, webhook, and config reload metrics)
- Structured JSON logging with `verification_step` field
- Graceful shutdown (SIGTERM drain)
- Defensive error handling for null receipts, malformed logs, disk full, and upstream OOM

**Out of scope for MVP:**
- Smart contracts, escrow, sessions, dynamic pricing, tiers, fee sponsorship, dashboard, multi-instance, on-chain registry, SSE streaming, multi-token support

### Wave 2: Sessions + Sponsorship (v0.2)

- Pay-as-you-go sessions with nonce-bound deposits, HMAC auth, atomic balance deduction
- Fee sponsorship via Tempo `withFeePayer` protocol (gateway runs fee payer HTTP service)
- Dynamic pricing via sessions/escrow (post-settlement, not prepay)
- Volume tier discounts
- Escrow contract for refund-eligible payments
- Multi-token support with per-token pricing
- Configurable `no_charge_on_5xx` per endpoint

### Wave 3: Dashboard + Discovery (v0.3)

- React revenue dashboard
- On-chain PayGateRegistry for service discovery
- Multi-instance support (PostgreSQL shared state)
- SSE streamed payments
- Additional webhook events (session.created, session.exhausted, session.refunded) — `payment.verified` webhook is in Wave 1

### Wave 4: Production (v1.0)

- Security audit (smart contracts + gateway)
- Multi-chain support
- SLA enforcement + auto-refunds
- Subscription plans
- API marketplace / directory
- Kubernetes deployment templates

## 15. Open Questions

| # | Question | Impact | Resolution Path |
|---|----------|--------|----------------|
| 1 | Tempo mainnet chain ID and RPC URL | Config defaults, client SDK chain config | Verify from `viem/chains` or docs.tempo.xyz when mainnet chain object is published |
| 2 | Mainnet USDC token contract address | `accepted_token` default in `paygate init` | Check Tempo mainnet token registry after launch |
| 3 | Formal MPP wire protocol spec | Header format, `tempo curl` compatibility | Track Tempo GitHub for MPP spec publication |
| 4 | Tempo fee payer service protocol | `/paygate/sponsor` implementation details | Reference `viem/tempo` `withFeePayer` transport source code |

## 16. Verification Plan

### Manual Testing (MVP)
1. Start a mock upstream API (simple echo server)
2. Run `paygate serve` with config pointing to it
3. `curl` the gateway — expect 402 with pricing headers and `quote_id`
4. Send a TIP-20 `transferWithMemo` on Tempo testnet with correct memo (containing `quote_id` + `requestHash`)
5. `curl` again with `X-Payment-Tx` and `X-Payment-Payer` headers — expect proxied response
6. Retry same tx hash — expect 402 (replay protection)
7. Send payment from address A, try to redeem with `X-Payment-Payer: addressB` — expect 402 (payer binding)
8. Send insufficient amount — expect 402 with shortfall
9. Wait for quote to expire, retry — expect re-verification against current price
10. Check `paygate revenue` shows the transaction
11. Verify `/paygate/health` and `/paygate/metrics` endpoints

### Client SDK Testing
1. Use TypeScript SDK (with `viem/tempo`) to call the gateway
2. Verify auto-discovery (402 -> pay -> retry) works transparently
3. Verify `requestHash` computed by SDK matches gateway's computation

### Shared Test Vectors

A shared fixture file `tests/fixtures/request_hash_vectors.json` contains test vectors for:
- `requestHash` computation (method, path, body → expected hash)
- Memo computation (`"paygate" || quoteId || requestHash` → expected memo bytes32)

Both the Rust and TypeScript test suites MUST validate against this fixture to ensure cross-language consistency.

### Test Matrix (29 cases)

| # | Codepath | Test description | Type | Lang |
|---|----------|-----------------|------|------|
| T1 | Rate limiter | Rejects at threshold (429) | Unit | Rust |
| T2 | Free endpoint | price=0 skips payment, returns 200 | Integration | Rust |
| T3 | 402 generation | Correct headers, JSON body, quote stored | Unit | Rust |
| T4 | Quote honored | Quoted price accepted within TTL after price change | Integration | Rust |
| T5 | Quote expired | Expired quote falls back to current price | Integration | Rust |
| T6 | Receipt fetch | Mock RPC, decode TIP-20 Transfer event logs | Unit | Rust |
| T7 | Memo verify | keccak256("paygate" \|\| quoteId \|\| requestHash) matches | Unit | Rust+vectors |
| T8 | Replay protection | Same tx_hash rejected on second use | Integration | Rust |
| T9 | Payer binding | X-Payment-Payer mismatch → rejected | Unit | Rust |
| T10 | TX age check | Stale tx (> tx_expiry_seconds) rejected | Unit | Rust |
| T11 | Multiple events | TX with 2 matching Transfer events → rejected | Unit | Rust |
| T12 | Wrong amount | amount < price → 402 with shortfall | Unit | Rust |
| T13 | Wrong recipient | to != provider → rejected | Unit | Rust |
| T14 | Header sanitization | X-Payment-* stripped before upstream | Integration | Rust |
| T15 | Upstream 5xx | Returns 502 + receipt | Integration | Rust |
| T16 | Request hash | Matches shared test vectors | Unit | Rust+TS |
| T17 | Config parsing | Minimal, full, and invalid TOML configs | Unit | Rust |
| T18 | Health endpoint | Healthy + degraded (RPC down) states | Integration | Rust |
| T19 | Metrics endpoint | Prometheus counters increment correctly | Integration | Rust |
| T20 | Graceful shutdown | SIGTERM drains in-flight requests | Integration | Rust |
| T21 | RPC failover | Primary timeout → secondary succeeds | Unit | Rust |
| T22 | TS SDK auto-pay | 402 → pay → retry flow works transparently | Integration | TS |
| T23 | TS SDK requestHash | Matches shared test vectors | Unit | TS |
| T24 | `paygate test` e2e | Full testnet: faucet → pay → verify → response | E2E | Rust |
| T25 | SQLite concurrency | 100 concurrent inserts, no SQLITE_BUSY | Unit | Rust |
| T26 | Invalid RPC receipt | None/empty receipt → 400 "tx not yet indexed" | Unit | Rust |
| T27 | Malformed event logs | Decode failure → 402 (not panic) | Unit | Rust |
| T28 | SQLite write failure | Simulated disk full → 503 to pending verifications | Unit | Rust |
| T29 | Upstream response OOM | Response body > size limit → 502 | Integration | Rust |

### Load Testing
1. Use `k6` or `wrk` to send concurrent paid requests
2. Verify no double-acceptance of transactions
3. Verify rate limiting kicks in correctly
4. Measure verification latency (target: < 100ms p99)
5. Verify SQLite writer throughput under concurrent session deductions (v0.2)
