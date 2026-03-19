# PayGate

Wrap any API behind per-request stablecoin micropayments on Tempo.

PayGate is a reverse proxy that gates API access behind on-chain payments. No API keys, no signup, no monthly billing. Consumers pay per request with USDC on [Tempo](https://tempo.xyz), and AI agents can pay autonomously.

## How it works

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

1. Consumer calls your API through PayGate
2. PayGate returns `402 Payment Required` with pricing info and a quote ID
3. Consumer sends a TIP-20 USDC transfer on Tempo with the quote in the memo
4. Consumer retries the request with the `X-Payment-Tx` header
5. PayGate verifies the payment on-chain (< 100ms) and forwards to your API

## Quick start

```bash
cargo install paygate           # or build from source
paygate init                    # 3-question setup wizard
paygate serve                   # proxy is live, accepting payments
```

```
$ paygate serve

  PayGate v0.1.0
  Proxy: 0.0.0.0:8080 → localhost:3000
  Tempo: rpc.tempo.xyz (connected)

  Ready. Accepting payments.
```

Verify your setup on testnet:

```bash
paygate test                    # end-to-end test against Tempo testnet
```

## Configuration

PayGate is configured via `paygate.toml`. The minimal config requires just 3 fields:

```toml
[gateway]
upstream = "http://localhost:3000"

[tempo]
rpc_urls = ["https://rpc.tempo.xyz"]

[provider]
address = "0x7F3a...your-wallet-address"
```

Everything else uses sensible defaults ($0.001/request, standard rate limits).

Set per-endpoint pricing:

```toml
[pricing]
default_price = "0.001"

[pricing.endpoints]
"POST /v1/chat/completions" = "0.005"
"GET /v1/models" = "0.000"              # free
"POST /v1/embeddings" = "0.001"
```

See [`paygate.toml.example`](paygate.toml.example) for the full configuration reference.

## Client SDK

The TypeScript SDK handles the 402 negotiation automatically:

```typescript
import { PayGateClient } from '@paygate/sdk';
import { createClient, http, publicActions, walletActions } from 'viem';
import { Account, tempoActions } from 'viem/tempo';
import { tempoModerato } from 'viem/chains'; // testnet; use `tempo` for mainnet

const account = Account.fromSecp256k1(process.env.TEMPO_PRIVATE_KEY!);

const tempoClient = createClient({
  account,
  chain: tempoModerato,
  transport: http(),
})
  .extend(publicActions)
  .extend(walletActions)
  .extend(tempoActions());

const client = new PayGateClient({ tempoClient });

// Use like a normal fetch — payment happens automatically
const response = await client.fetch(
  'https://api.example.com/v1/chat/completions',
  {
    method: 'POST',
    body: JSON.stringify({ model: 'gpt-4', messages: [...] }),
  }
);
```

For AI agents, wrap it as a tool:

```typescript
const paidApiTool = {
  name: 'call_paid_api',
  description: 'Call a micropayment-gated API. Payment is handled automatically.',
  parameters: { url: 'string', method: 'string', body: 'object' },
  execute: async ({ url, method, body }) => {
    return await client.fetch(url, { method, body: JSON.stringify(body) });
  },
};
```

## CLI reference

```
paygate init              Interactive setup wizard → generates paygate.toml
paygate serve             Start the gateway proxy
paygate status            Show gateway status and RPC connectivity
paygate pricing           Display current pricing table
paygate pricing --html    Generate static HTML pricing page
paygate sessions          List active payment sessions
paygate revenue           Revenue summary (24h, 7d, 30d)
paygate wallet            Show provider on-chain balance + 24h income
paygate demo              Spin up demo echo server + run full payment cycle
paygate test              End-to-end test against Tempo testnet
```

## Architecture

| Component | Tech | Description |
|-----------|------|-------------|
| **Gateway** | Rust (axum + tower) | Reverse proxy with payment verification middleware |
| **Client SDK** | TypeScript + Rust | Payment-aware HTTP client that auto-pays |
| **CLI** | Rust (clap) | `paygate init`, `paygate serve`, `paygate revenue` |
| **Local DB** | SQLite (WAL mode) | Replay protection, session state, request logs |

The gateway is a tower middleware stack:

```
Request
  → Rate Limiter        reject if over limit (429)
  → MPP Negotiator      no payment headers → 402 with pricing + quote
  |                     price == 0 → skip to proxy (free endpoint)
  → Payment Verifier    verify on-chain payment via Tempo RPC
  → Payer Binder        verify X-Payment-Payer matches on-chain sender
  → Header Sanitizer    strip X-Payment-* headers before forwarding
  → Reverse Proxy       forward to upstream
  → Response Logger     log to SQLite, export Prometheus metrics
  → Receipt Injector    add X-Payment-Receipt + X-Payment-Cost headers
Response
```

## Security

PayGate is designed for adversarial environments where consumers may attempt to bypass payment:

- **Replay protection** — Each `tx_hash` can only be used once (SQLite UNIQUE constraint)
- **Payer binding** — `X-Payment-Payer` must match the on-chain `from` address
- **Request binding** — Payment memo includes `keccak256(quoteId + requestHash)`, tying the payment to the specific API request
- **Stale tx rejection** — Transactions older than 5 minutes are rejected
- **Ambiguity rejection** — Transactions with multiple matching Transfer events are rejected
- **Upstream isolation** — All `X-Payment-*` headers are stripped before forwarding; upstream sees a normal HTTP request
- **Rate limiting** — Per-payer and global rate limits prevent abuse
- **SSRF protection** — Webhook URLs are validated against private IP ranges

Tempo's single-slot finality means a transaction in a receipt is irreversible. No reorg risk, no confirmation waiting.

## Observability

- **Health check**: `GET /paygate/health` — JSON status of gateway, RPC, upstream, DB
- **Prometheus metrics**: `GET /paygate/metrics` — payment counts, verification latency, upstream latency, revenue, rate limit rejections, RPC errors
- **Structured logging**: JSON to stdout with `tx_hash`, `payer_address`, `endpoint`, `amount`, `verification_result`, `latency_ms`
- **Receipt verification**: `GET /paygate/receipts/{tx_hash}` — look up a specific payment

## Roadmap

| Wave | Version | Focus |
|------|---------|-------|
| **1** | **v0.1** | **MVP** — Gateway, payment verification, CLI, TypeScript SDK, testnet e2e |
| 2 | v0.2 | Sessions (pay-as-you-go), fee sponsorship, dynamic pricing, escrow |
| 3 | v0.3 | React dashboard, on-chain service registry, multi-instance (PostgreSQL) |
| 4 | v1.0 | Security audit, multi-chain, SLA enforcement, API marketplace |

## Project structure

```
paygate/
  Cargo.toml                      # workspace root
  crates/
    paygate-gateway/              # main binary (axum + tower middleware)
    paygate-common/               # shared types, hash functions, MPP headers
    paygate-client/               # Rust client SDK
  sdk/                            # TypeScript client SDK (@paygate/sdk)
  contracts/                      # Solidity (Foundry) — optional registry + escrow
  tests/
    fixtures/
      request_hash_vectors.json   # shared hash test vectors (Rust + TS)
```

## License

MIT
