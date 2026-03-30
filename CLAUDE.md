# CLAUDE.md — Instructions for AI assistants

## What is PayGate?

PayGate is a reverse proxy that gates HTTP API access behind per-request stablecoin micropayments on the Tempo blockchain. It returns `HTTP 402 Payment Required` with pricing info, verifies on-chain TIP-20 payments, and forwards paid requests to the upstream API.

## Build & test

```bash
# Rust workspace (gateway, common types, client SDK)
cargo build
cargo test

# TypeScript SDK
cd sdk && npm install && npm test

# Solidity contracts (optional)
cd contracts && forge test
```

## Project structure

```
paygate/
  crates/
    paygate-gateway/        # Main binary. axum server + tower middleware stack.
      src/
        main.rs             # CLI (clap) — all subcommands
        config.rs           # TOML config parsing + validation
        server.rs           # AppState (ArcSwap config, DB handles, HTTP client, Prometheus)
        verifier.rs         # On-chain payment verification (TIP-20 event log decoding)
        mpp.rs              # 402 response generation + quote management
        proxy.rs            # Reverse proxy to upstream
        rate_limit.rs       # Rate limiting (global + per-payer + 402 flood)
        db.rs               # SQLite (WAL mode, single writer task, batch writes)
        admin.rs            # Admin API: /paygate/health, /metrics, /receipts/{tx_hash}
        metrics.rs          # Prometheus metric recording functions
        webhook.rs          # Fire-and-forget payment notification webhooks
    paygate-common/         # Shared types used by gateway + client
      src/
        types.rs            # BaseUnits, PaymentProof, VerificationResult, format_amount/format_usd
        hash.rs             # requestHash + payment memo computation (keccak256)
        mpp.rs              # X-Payment-* header constants + is_payment_header()
    paygate-client/         # Rust client SDK
      src/
        client.rs           # PayGateClient — auto-pay HTTP client
        discovery.rs        # 402 parsing + pricing discovery
  sdk/                      # TypeScript client SDK (@paygate/sdk)
    src/
      client.ts             # PayGateClient wrapping viem/tempo
      hash.ts               # requestHash computation (must match Rust)
  contracts/                # Solidity (Foundry) — optional registry + escrow
  tests/
    fixtures/
      request_hash_vectors.json   # Shared test vectors for cross-language hash parity
  schema.sql                # SQLite schema (payments, quotes, sessions, request_log)
```

## Key architectural decisions

### Tower middleware stack
Requests flow through layered tower middleware: Rate Limiter → MPP Negotiator → Payment Verifier → Payer Binder → Header Sanitizer → Reverse Proxy → Response Logger → Receipt Injector. Free endpoints (price == 0) skip from MPP Negotiator directly to Header Sanitizer.

### SQLite WAL with single writer task
All DB writes go through a dedicated tokio task via a bounded mpsc channel. Writes are batched in transactions (flush every 10ms or 50 writes). This avoids `SQLITE_BUSY` under concurrent load. When the channel is full, the gateway returns 503 (backpressure).

### ArcSwap config
Config is wrapped in `Arc<ArcSwap<Config>>` so it can be reloaded at runtime via SIGHUP without restarting the server.

### RPC failover
`rpc_urls` is an array. The gateway tries each URL in order with a configurable timeout, falling back to the next on failure.

## Testing

```bash
cargo test                  # all Rust tests
cd sdk && npm test          # TypeScript SDK tests
```

### Shared hash test vectors
`tests/fixtures/request_hash_vectors.json` contains input/output pairs for `requestHash` and `payment_memo`. Both the Rust (`paygate-common/src/hash.rs`) and TypeScript (`sdk/src/hash.ts`) implementations must produce identical output for every vector. **If you modify the hash algorithm, update the vectors and verify both implementations.**

### Test categories
- **Unit tests**: In each module's `#[cfg(test)]` block. Cover config parsing, hash computation, rate limiting, event log decoding.
- **Integration tests**: Admin endpoints (health, metrics, receipts), payment verification flow.
- **E2E**: `paygate test` runs against Tempo testnet (requires `PAYGATE_TEST_KEY` env var).

## Important rules

### Cross-language hash parity
The `requestHash` algorithm (`keccak256(method || " " || path || "\n" || body)`) is implemented in both Rust and TypeScript. Any change to one MUST be mirrored in the other and validated against the shared test vectors.

### Error → HTTP status mapping
Every payment verification error maps to a specific HTTP status code. The mapping is defined in `docs/designs/error-rescue-registry.md`. When adding new error paths:
1. Add the error variant to `VerificationResult` in `paygate-common/src/types.rs`
2. Add the HTTP mapping in `verifier.rs`
3. Add the entry to `error-rescue-registry.md`
4. Write a test for the new error path

### CLI output format
All CLI output follows strict conventions (see SPEC.md §9.1):
- 2-space indent for all output blocks
- Section headers use `───` (U+2500) underlines
- Errors: `error: <message>` + indented `hint: <fix>`
- No emoji (except `✓`/`✗` in test output), no color by default
- Monetary: `$X.XX` format. Addresses: truncated `0x7F3a...Prov`

### Security
- Payment headers (`X-Payment-*`) are stripped before forwarding to upstream
- Webhook URLs are validated against private IP ranges (SSRF protection)
- Private keys are read from env vars, never stored in config files
- SQLite `tx_hash` UNIQUE constraint is the replay protection mechanism — do not remove

## Design System
Always read DESIGN.md before making any visual or UI decisions.
All font choices, colors, spacing, and aesthetic direction are defined there.
Do not deviate without explicit user approval.
In QA mode, flag any code that doesn't match DESIGN.md.
