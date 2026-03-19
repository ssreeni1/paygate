# Brief: Fee Sponsorship Endpoint

## Objective

Implement `POST /paygate/sponsor` in the Rust gateway -- the fee payer HTTP service that implements the Tempo fee payer protocol. When enabled, the gateway pays transaction gas fees on behalf of consumers, so consumers only need testnet USDC (PathUSD) and no native Tempo gas.

## Context

Read these files before starting:
- `SPEC.md` section 4.4 (Fee Sponsorship Flow)
- `crates/paygate-gateway/src/config.rs` — existing `SponsorshipConfig` struct (fields: `enabled`, `sponsor_listen`, `budget_per_day`, `max_per_tx`)
- `crates/paygate-gateway/src/server.rs` — `AppState` struct
- `crates/paygate-gateway/src/main.rs` — router setup in `cmd_serve`
- `docs/designs/tempo-sdk-verification.md` — confirms `withFeePayer` transport exists at `viem/tempo`

The Tempo fee payer protocol (from `https://docs.tempo.xyz/guide/payments/sponsor-user-fees`):
- Consumers configure their viem client: `withFeePayer(http(), http('https://host/paygate/sponsor'))`
- The fee payer endpoint receives unsigned or partially-signed transactions
- The fee payer signs the fee portion with its own private key
- The fee payer relays the signed transaction to the Tempo RPC
- The fee payer returns the transaction hash

Public testnet sponsor service (for reference): `https://sponsor.moderato.tempo.xyz`

## New File: `crates/paygate-gateway/src/sponsor.rs`

### Tempo Fee Payer Protocol

The fee payer service implements a JSON-RPC compatible endpoint. Based on the Tempo documentation and the public sponsor service behavior:

**Request format** (from the consumer's viem client via `withFeePayer` transport):
The endpoint receives standard Ethereum JSON-RPC calls, primarily `eth_sendTransaction` or a Tempo-specific method. The fee payer transport in viem forwards the transaction request to the sponsor URL instead of directly to the RPC.

The implementation should:
1. Accept JSON-RPC requests at `POST /paygate/sponsor`
2. For `eth_sendTransaction` or `eth_sendRawTransaction`:
   - Parse the transaction from the request
   - Sign the fee payer portion using the gateway's private key (from `PAYGATE_PRIVATE_KEY` env var)
   - Submit the fully-signed transaction to the Tempo RPC
   - Return the transaction hash in JSON-RPC response format

**Important**: Study the actual protocol by making test requests to `https://sponsor.moderato.tempo.xyz` and reading the viem/tempo `withFeePayer` transport source code. The exact request/response format must match what viem expects. If the protocol involves RPC method proxying (the sponsor acts as an RPC proxy that intercepts and co-signs transactions), implement it as such.

### Core Logic

```rust
pub struct SponsorService {
    config: Arc<ArcSwap<Config>>,
    http_client: reqwest::Client,
    signer: /* alloy PrivateKeySigner or equivalent */,
    budget: Arc<SponsorBudget>,
}

pub struct SponsorBudget {
    daily_spent: AtomicU64,       // base units spent today
    daily_limit: u64,             // from config.sponsorship.budget_per_day
    per_tx_limit: u64,            // from config.sponsorship.max_per_tx
    last_reset: AtomicI64,        // UTC timestamp of last midnight reset
}
```

### Budget Tracking

Track daily spend **in memory** (no DB needed -- acceptable to reset on restart):

1. `daily_spent: AtomicU64` -- incremented on each sponsored tx
2. Reset at midnight UTC: check `last_reset` timestamp before each operation; if current day differs, reset `daily_spent` to 0
3. Before signing any transaction:
   - Estimate the fee (or use a fixed upper bound like `max_per_tx`)
   - Check: `daily_spent + estimated_fee <= daily_limit`
   - Check: `estimated_fee <= per_tx_limit`
   - If either check fails: return JSON-RPC error or HTTP 503 with `"Fee sponsorship temporarily unavailable"`
4. After successful relay: increment `daily_spent` by actual fee (or estimated fee if actual is hard to determine synchronously)

### Budget Configuration Parsing

The config already has these fields in `SponsorshipConfig`:
```rust
pub budget_per_day: String,  // e.g., "10.00" (USDC)
pub max_per_tx: String,      // e.g., "0.01" (USDC)
```

Parse these to base units (6 decimals) at startup. Use the existing `parse_price_to_base_units` function from `config.rs`. However, note that gas fees are in the native currency (USD on Tempo, also 6 decimals), not in USDC -- adjust if needed.

### Metric

Add a Prometheus gauge:
```
paygate_sponsor_budget_remaining  // daily_limit - daily_spent, in base units
```

Update this metric:
- After each sponsored transaction
- Every 60 seconds via a background task that also checks the gateway wallet's on-chain balance

### Balance Check Task

Spawn a background task that runs every 60 seconds:
1. Query the gateway wallet's native balance via `eth_getBalance` RPC call
2. Update a `paygate_sponsor_wallet_balance` gauge metric
3. If balance < `max_per_tx`, log a warning: `"sponsor wallet balance critically low"`

### Signer Setup

Read the private key from the env var specified in `config.tempo.private_key_env` (default: `PAYGATE_PRIVATE_KEY`).

Use `alloy` crate for signing:
```rust
use alloy::signers::local::PrivateKeySigner;
let signer: PrivateKeySigner = std::env::var(&config.tempo.private_key_env)
    .expect("PAYGATE_PRIVATE_KEY not set")
    .parse()
    .expect("invalid private key");
```

If `sponsorship.enabled = true` but the private key env var is missing, fail at startup with:
```
error: sponsorship enabled but PAYGATE_PRIVATE_KEY not set
  hint: export PAYGATE_PRIVATE_KEY=<your-tempo-private-key> or set sponsorship.enabled = false
```

## Wiring into the Gateway

In `main.rs` `cmd_serve`, after building the main gateway router:

```rust
if config.sponsorship.enabled {
    let sponsor_service = sponsor::SponsorService::new(
        state.config.clone(),
        state.http_client.clone(),
    )?;
    // Add sponsor route to the gateway router
    // The path comes from config.sponsorship.sponsor_listen (default: "/paygate/sponsor")
    gateway_app = gateway_app.route(
        &config.sponsorship.sponsor_listen,
        post(sponsor::handle_sponsor).with_state(sponsor_service),
    );
    // Spawn balance check background task
    sponsor_service.spawn_balance_checker();
}
```

## AppState Changes

The `SponsorService` can either:
- Be a separate state type injected into the sponsor route only (preferred -- keeps AppState unchanged)
- Or be added as `Option<SponsorService>` to AppState

Prefer option 1: the sponsor handler has its own state via `axum::extract::State<SponsorService>`.

## Error Responses

| Condition | Response |
|-----------|----------|
| Budget exhausted (daily) | 503 `{"error": "fee_sponsorship_unavailable", "message": "Fee sponsorship temporarily unavailable — daily budget exhausted"}` |
| Budget exhausted (per-tx) | 400 `{"error": "fee_too_high", "message": "Transaction fee exceeds per-transaction sponsorship limit"}` |
| Invalid transaction format | 400 `{"error": "invalid_transaction", "message": "..."}` |
| Relay to RPC failed | 502 `{"error": "relay_failed", "message": "Failed to relay transaction to Tempo RPC"}` |
| Private key not configured | Startup failure (not a runtime error) |

## Tests

Write tests in `crates/paygate-gateway/src/sponsor.rs` (inline `#[cfg(test)]` module) or a separate test file:

### Test 1: Budget tracking — daily limit

1. Create a `SponsorBudget` with `daily_limit = 10_000` (0.01 USDC)
2. Spend 9_000
3. Assert: 1_000 remaining
4. Try to spend 2_000 -- should fail (exceeds remaining)
5. Try to spend 1_000 -- should succeed
6. Try to spend 1 -- should fail (exhausted)

### Test 2: Budget tracking — per-tx limit

1. Create a `SponsorBudget` with `per_tx_limit = 5_000`, `daily_limit = 100_000`
2. Try to spend 6_000 -- should fail immediately
3. Try to spend 5_000 -- should succeed

### Test 3: Budget reset at midnight

1. Create a `SponsorBudget`, spend to near-limit
2. Manually set `last_reset` to yesterday
3. Call `check_and_spend` -- should reset and succeed

### Test 4: Invalid transaction format

1. POST garbage JSON to the sponsor endpoint
2. Expect 400 with descriptive error

### Test 5: Sponsorship disabled

1. Config has `sponsorship.enabled = false`
2. The `/paygate/sponsor` route should not be registered
3. Requests to that path should get the normal gateway fallback (402 or 404)

## Dependencies

May need to add to `Cargo.toml`:
- `alloy` (for signing) -- check if already in workspace deps
- `alloy-signer-local` if needed for `PrivateKeySigner`

Check existing `Cargo.toml` for what alloy crates are already used.

## Key Constraints

- The private key is NEVER logged, NEVER in config files, NEVER in error messages
- Budget tracking must be atomic (use `AtomicU64` for concurrent access)
- The sponsor endpoint must be fast -- it should not add significant latency to the consumer's transaction
- If the Tempo fee payer protocol turns out to be different from what's described here (after studying the actual viem source), adapt accordingly -- the protocol correctness is more important than matching this brief exactly
