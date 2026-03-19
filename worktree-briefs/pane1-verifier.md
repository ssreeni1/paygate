# Pane 1 Brief: Payment Verification Core (feat/verifier)

## Context
You are building the payment verification core for PayGate — a reverse proxy that gates API access behind TIP-20 stablecoin micropayments on the Tempo blockchain. This is the most critical subsystem: it verifies on-chain payments and decides whether to forward requests upstream.

## Required Reading (do this first)
1. `SPEC.md` — Read in full, but focus on:
   - §3.2 Middleware Stack (the layer ordering you must implement)
   - §4.1 Price Discovery (402 response format, quote system)
   - §4.2 Direct Payment Flow (verification steps, request hash, memo)
   - §10 Security (all threat mitigations you must enforce)
   - §11 Edge Cases (every edge case must be handled)
   - §12 Observability (metrics to emit, log format)
2. `docs/designs/error-rescue-registry.md` — Every error type, its HTTP code, and rescue action
3. `docs/designs/failure-modes.md` — Every failure scenario and expected behavior
4. `crates/paygate-common/src/` — Read ALL files. These are your shared types:
   - `types.rs`: `VerificationResult`, `PaymentProof`, `PaymentRecord`, `PricingInfo`, `Quote`, `BaseUnits`
   - `hash.rs`: `request_hash()` and `payment_memo()` — you MUST use these, not reimplement
   - `mpp.rs`: All `X-Payment-*` header constants, `is_payment_header()` for sanitization
5. `crates/paygate-gateway/src/` — Read ALL existing files:
   - `config.rs`: `Config` struct with all fields, `price_for_endpoint()`, `parse_price_to_base_units()`
   - `db.rs`: `DbReader` (is_tx_consumed, get_quote, get_payment), `DbWriter` (insert_payment, insert_quote, consume_quote, log_request)
   - `server.rs`: `AppState` with `config: Arc<ArcSwap<Config>>`, `db_reader`, `db_writer`, `http_client`
   - `metrics.rs`: All metric recording functions (record_payment_verified, record_verification_duration, etc.)
6. `tests/fixtures/request_hash_vectors.json` — Test vectors for hash verification

## What to Build

### 1. `crates/paygate-gateway/src/verifier.rs`
The payment verification pipeline. This is an async function, not middleware — it's called by the middleware layer.

```rust
pub async fn verify_payment(
    state: &AppState,
    tx_hash: &str,
    payer_address: &str,
    quote_id: Option<&str>,
    endpoint: &str,
    request_hash: &B256,
) -> VerificationResult
```

Implementation steps (must be in this order):
1. **Fetch tx receipt**: Call `eth_getTransactionReceipt` on Tempo RPC via `state.http_client`
   - RPC request: POST to rpc_url with JSON-RPC body `{"jsonrpc":"2.0","method":"eth_getTransactionReceipt","params":["0x..."],"id":1}`
   - If response is null → return `TxNotFound` (caller maps to 400 + Retry-After: 1)
   - If RPC errors (timeout, 5xx, network) → return `RpcError` (caller maps to 503 + Retry-After: 2)
   - Use the FIRST rpc_url; on failure try the next (failover from config.tempo.rpc_urls)
   - Respect config.tempo.rpc_timeout_ms

2. **Decode TIP-20 Transfer event logs**: From the receipt's `logs` array
   - Transfer event signature: `Transfer(address,address,uint256)` = `keccak256("Transfer(address,address,uint256)")`
   - Topic[0] = event signature hash
   - Topic[1] = from address (zero-padded to 32 bytes)
   - Topic[2] = to address (zero-padded to 32 bytes)
   - Data = uint256 amount
   - Filter for: `to == provider_address` AND log address == `accepted_token`
   - If zero matches → `InvalidTransfer("no matching Transfer event")`
   - If multiple matches → `AmbiguousTransfer`
   - Extract the ONE matching event's from, to, amount

3. **Decode TransferWithMemo event**: Find the TransferWithMemo log
   - Event: `TransferWithMemo(address,address,uint256,bytes32)` or similar
   - Extract the memo bytes32 field

4. **Verify memo**:
   - Compute expected memo using `paygate_common::hash::payment_memo(quote_id, request_hash)`
   - Compare with on-chain memo (use constant-time comparison)
   - Mismatch → `MemoMismatch { expected, actual }`

5. **Verify amount**:
   - Get expected price: if quote_id is provided AND quote exists AND not expired, use quote price
   - Otherwise use `config.price_for_endpoint(endpoint)`
   - If on-chain amount < expected → `InsufficientAmount { expected, actual }`

6. **Verify payer binding**:
   - Compare `payer_address` (from X-Payment-Payer header) with `from` in Transfer event
   - Case-insensitive hex comparison
   - Mismatch → `PayerMismatch { expected, actual }`

7. **Check replay protection**:
   - Call `state.db_reader.is_tx_consumed(tx_hash)`
   - If true → `ReplayDetected`

8. **Check tx age**:
   - Get block timestamp from receipt (or use block_number and estimate)
   - If age > config.security.tx_expiry_seconds → `ExpiredTransaction`

9. **Record payment**: On success, insert via `state.db_writer.insert_payment()`
   - If insert fails with UNIQUE violation → `ReplayDetected` (concurrent race)

10. **Consume quote**: If quote_id was used and valid, call `state.db_writer.consume_quote()`

Metrics to emit at each step: `record_verification_duration`, `record_payment_verified`

### 2. `crates/paygate-gateway/src/mpp.rs`
402 response generation and quote management.

```rust
/// Generate a 402 Payment Required response for an endpoint.
pub async fn payment_required_response(
    state: &AppState,
    endpoint: &str,
) -> axum::response::Response

/// Check if a request has payment headers (X-Payment-Tx).
pub fn has_payment_headers(headers: &HeaderMap) -> bool

/// Extract payment headers from a request.
pub fn extract_payment_headers(headers: &HeaderMap) -> Option<PaymentHeaders>

pub struct PaymentHeaders {
    pub tx_hash: String,
    pub payer_address: String,
    pub quote_id: Option<String>,
}
```

The 402 response MUST include:
- All X-Payment-* response headers (from mpp constants)
- JSON body with: `error: "payment_required"`, `message` (actionable, includes amount and provider address), `help_url: "https://ssreeni1.github.io/paygate/quickstart#paying"`, `pricing` object
- A new quote stored in DB with TTL from config.pricing.quote_ttl_seconds

### 3. `crates/paygate-gateway/src/rate_limit.rs`
```rust
/// Rate limiter state, created at startup.
pub struct RateLimiter { ... }

/// Tower middleware layer.
pub async fn rate_limit_middleware(state, request, next) -> Response
```

Use `governor` crate. Three limits:
- Global: config.rate_limiting.requests_per_second
- Per-payer: config.rate_limiting.per_payer_per_second (keyed by X-Payment-Payer header or IP)
- 402 flood: 1000/min per IP on 402 responses

Return 429 with JSON body on limit exceeded. Call `metrics::record_rate_limit_rejected()`.

### 4. `crates/paygate-gateway/src/proxy.rs`
```rust
/// Forward a request to the upstream API.
pub async fn forward_request(
    state: &AppState,
    mut request: Request,
) -> Result<Response, ProxyError>
```

Steps:
1. Strip ALL headers where `mpp::is_payment_header(name)` returns true
2. Forward to `config.gateway.upstream` + original path + query string
3. Timeout: `config.gateway.upstream_timeout_seconds`
4. If response body > `config.gateway.max_response_body_bytes` → 502
5. Add `X-Payment-Receipt: {tx_hash}` header to response
6. Add `X-Payment-Cost: {formatted_amount}` header (use `paygate_common::types::format_amount`)
7. Call `metrics::record_upstream_duration()`
8. On timeout → 504, on connection error → 502

### 5. `crates/paygate-gateway/src/webhook.rs`
```rust
/// Webhook sender, created at startup. Fire-and-forget.
pub struct WebhookSender { ... }

/// Send payment notification (non-blocking).
pub fn notify_payment_verified(&self, tx_hash, payer, amount, endpoint)
```

- Uses `tokio::spawn` internally — never awaited by caller
- `tokio::sync::Semaphore` with 50 permits to cap concurrent deliveries
- Timeout from config.webhooks.timeout_seconds
- Log success/failure, call `metrics::record_webhook_delivery("success"/"failure"/"timeout")`
- Payload: `{"event":"payment.verified","tx_hash":"0x...","payer_address":"0x...","amount":1000,"endpoint":"POST /v1/chat","timestamp":"2026-03-18T12:00:00Z"}`

### 6. Update `main.rs`
Wire the tower middleware stack in this order (from SPEC §3.2):
```
Request
  -> rate_limit_middleware
  -> mpp_middleware (check for payment headers; if none and price>0 → 402; if price==0 → skip to proxy)
  -> payment_verify_middleware (call verifier::verify_payment, handle result)
  -> payer_bind_middleware (already done in verifier, this is a no-op or combined)
  -> header_sanitize (strip X-Payment-* before forwarding — done in proxy.rs)
  -> proxy::forward_request
  -> response_logger (log to DB via db_writer)
  -> receipt_injector (add receipt headers — done in proxy.rs)
Response
```

You can combine some layers. The key constraint: no request is EVER forwarded upstream without verified payment (unless price == 0).

## Integration Points
- Use `AppState` from server.rs for all shared state
- Use `paygate_common::hash::request_hash()` to compute request hash from incoming request
- Use `paygate_common::hash::payment_memo()` to compute expected memo
- Use `paygate_common::mpp::*` for all header name constants
- Use `paygate_common::types::*` for all shared types
- Use `crate::metrics::*` for all metric recording
- Use `crate::db::DbReader` for reads, `crate::db::DbWriter` for writes
- Config is accessed via `state.current_config()` (returns `Arc<Config>`)

## Tests to Write
Put in `crates/paygate-gateway/src/verifier.rs` (as `#[cfg(test)] mod tests`) or in `tests/`:
1. Valid payment verification with mock RPC response
2. Replay rejection (same tx_hash twice)
3. Payer mismatch detection
4. Insufficient amount detection
5. Expired transaction detection
6. Memo mismatch detection
7. Ambiguous transfer (multiple matching events) detection
8. Null receipt → TxNotFound
9. RPC error handling
10. Quote honored within TTL
11. Quote expired → fallback to current price
12. 402 response format (headers + JSON body match spec)
13. Free endpoint (price=0) bypasses payment
14. Header sanitization (X-Payment-* stripped)
15. Rate limiter returns 429

## Commit Message Format
When done, commit with a descriptive message. Make sure `cargo check` and `cargo test` pass first.
