# Build Brief: Sessions Protocol + no_charge_on_5xx (Pane 1)

Wave 2 core feature. Build the full session lifecycle: nonce, deposit verification, HMAC auth, atomic balance deduction, and no_charge_on_5xx refund.

## New File: `crates/paygate-gateway/src/sessions.rs`

### 1. Nonce Endpoint — `POST /paygate/sessions/nonce`

Handler: `pub async fn handle_nonce(State(state): State<AppState>, req: Request<Body>) -> Response`

- Extract `X-Payment-Payer` header (required, 402 if missing)
- Validate payer address format (0x + 40 hex chars)
- Check concurrent session limit: `count_active_sessions_for_payer(payer) < config.sessions.max_concurrent_per_payer`
  - If exceeded: 429 with `{"error": "max_concurrent_sessions", "message": "Maximum concurrent sessions exceeded", "limit": N}`
- Generate nonce: `nonce_{uuid_v4}` (use `uuid::Uuid::new_v4()`)
- Compute `expires_at = now + 300` (5 minutes)
- Store nonce via `create_session_nonce(payer, nonce, expires_at)`
- Return 200:
  ```json
  { "nonce": "nonce_abc123", "expiresAt": "2026-03-23T16:05:00Z" }
  ```

### 2. Create Session Endpoint — `POST /paygate/sessions`

Handler: `pub async fn handle_create_session(State(state): State<AppState>, req: Request<Body>) -> Response`

- Extract `X-Payment-Tx` and `X-Payment-Payer` headers (required)
- Parse request body as JSON to get `{ "nonce": "nonce_..." }` — the nonce is in the body
- Look up nonce via `get_session_by_nonce(nonce)`:
  - Not found or expired: 400 `{"error": "invalid_nonce", "message": "Nonce not found or expired"}`
  - Already used (session exists with this nonce): 409 `{"error": "nonce_consumed", "message": "Nonce already used"}`
- Verify on-chain deposit (same pattern as verifier.rs):
  - Fetch tx receipt via RPC (`eth_getTransactionReceipt`)
  - Decode TIP-20 Transfer event logs
  - Verify `to` matches `config.provider.address`
  - Verify `from` matches `X-Payment-Payer`
  - Verify amount >= `config.sessions.minimum_deposit` (parse with `parse_price_to_base_units`)
  - Verify memo: look for TransferWithMemo log, verify memo = `keccak256("paygate-session" || nonce)` as bytes32
  - Verify tx not already consumed (check payments table)
- Generate session credentials:
  - `session_id`: `sess_{64 hex chars}` — use `rand::thread_rng().gen::<[u8; 32]>()` then hex encode
  - `session_secret`: `ssec_{64 hex chars}` — same approach, separate random bytes
- Compute `rate_per_request = config.price_for_endpoint("default")` — use default price since sessions apply across endpoints. discount_percent = 0 per CEO plan.
- Compute `expires_at = now + (config.sessions.max_duration_hours * 3600)`
- Store session via `create_session(SessionRecord { id, secret, payer_address, deposit_tx, nonce, deposit_amount, balance: deposit_amount, rate_per_request, requests_made: 0, created_at: now, expires_at, status: "active" })`
- Also insert into payments table (so the tx_hash is marked consumed for replay protection)
- Return 201:
  ```json
  {
    "sessionId": "sess_<hex>",
    "sessionSecret": "ssec_<hex>",
    "balance": "0.050000",
    "ratePerRequest": "0.001000",
    "expiresAt": "2026-03-24T16:00:00Z"
  }
  ```
  Balance and ratePerRequest are decimal strings (divide base units by 1_000_000).

### 3. `verify_and_deduct()` — Called from `gateway_handler` in serve.rs

```rust
pub async fn verify_and_deduct(
    state: &AppState,
    headers: &HeaderMap,
    request_hash: &str,
    endpoint: &str,
) -> Result<SessionDeduction, SessionError>
```

Returns:
```rust
pub struct SessionDeduction {
    pub session_id: String,
    pub payer_address: String,
    pub amount_deducted: u64,
    pub remaining_balance: u64,
}

pub enum SessionError {
    MissingHeaders,
    SessionNotFound,
    SessionExpired,
    InsufficientBalance { balance: u64, rate: u64 },
    InvalidSignature,
    StaleTimestamp,
    DeductionFailed,
}
```

Steps:
1. Extract `X-Payment-Session`, `X-Payment-Session-Sig`, `X-Payment-Timestamp` from headers
2. Look up session by ID via `get_session(id)`:
   - Not found: `SessionNotFound`
   - Status != 'active': `SessionExpired`
   - `expires_at < now`: `SessionExpired`
3. Verify timestamp freshness: `|now - timestamp| < 60` seconds. If stale: `StaleTimestamp`
4. Compute expected HMAC: `HMAC-SHA256(session.secret, request_hash || timestamp_str)`
   - Use `hmac` and `sha2` crates
   - **Constant-time comparison**: use `hmac::Mac::verify_slice()` which does constant-time compare
5. If HMAC invalid: `InvalidSignature`
6. Determine rate: use `config.price_for_endpoint(endpoint)` for the specific endpoint price (not default)
7. Atomically deduct: `deduct_session_balance(session.id, rate)` which runs:
   ```sql
   UPDATE sessions SET balance = balance - ?, requests_made = requests_made + 1
   WHERE id = ? AND balance >= ? AND status = 'active' AND expires_at > ?
   ```
   If zero rows affected: `InsufficientBalance`
8. Return `SessionDeduction { session_id, payer_address, amount_deducted: rate, remaining_balance }`

### 4. `no_charge_on_5xx` — Post-Proxy Refund

After proxying the request in `gateway_handler`, if the upstream response status is 5xx:
- Check if endpoint has `no_charge_on_5xx = true` in config
- If yes: call `refund_session_balance(session_id, amount_deducted)`
  ```sql
  UPDATE sessions SET balance = balance + ?, requests_made = requests_made - 1
  WHERE id = ?
  ```
- Add `X-Payment-Refunded: true` header to the response

Config: Add `no_charge_on_5xx` as a HashMap<String, bool> in PricingConfig or as a per-endpoint toggle. Simplest approach: add a new config section or extend `pricing.endpoints` to support structured values. **Recommended**: add `[pricing.no_charge_on_5xx]` section as a list of endpoint patterns:
```toml
[pricing]
no_charge_on_5xx = ["POST /v1/summarize", "POST /v1/search"]
```
Add to `PricingConfig`:
```rust
#[serde(default)]
pub no_charge_on_5xx: Vec<String>,
```
Add helper to `Config`:
```rust
pub fn is_no_charge_on_5xx(&self, endpoint: &str) -> bool {
    self.pricing.no_charge_on_5xx.contains(&endpoint.to_string())
}
```

### 5. Update `serve.rs` — `gateway_handler`

Modify the payment check logic in `gateway_handler`. After the free-endpoint check and before the existing direct-payment path, add a session check:

```rust
// Free endpoint: skip payment
if price == 0 { /* existing code */ }

// Session auth: HMAC-based
if parts.headers.contains_key("x-payment-session") {
    let request_hash = paygate_common::hash::request_hash(&method, &path, &body_bytes);
    match sessions::verify_and_deduct(&state, &parts.headers, &request_hash, &endpoint).await {
        Ok(deduction) => {
            let req = Request::from_parts(parts, Body::from(body_bytes));
            match crate::proxy::forward_request(&state, req, "", deduction.amount_deducted, &endpoint).await {
                Ok(mut resp) => {
                    // no_charge_on_5xx refund
                    if resp.status().is_server_error() && config.is_no_charge_on_5xx(&endpoint) {
                        let _ = state.db_writer.refund_session_balance(&deduction.session_id, deduction.amount_deducted).await;
                        resp.headers_mut().insert("X-Payment-Refunded", HeaderValue::from_static("true"));
                    }
                    let _ = state.db_writer.log_request(
                        None, Some(deduction.session_id), endpoint, deduction.payer_address,
                        deduction.amount_deducted, Some(resp.status().as_u16() as i32), None,
                    ).await;
                    resp
                }
                Err(e) => /* proxy error handling, same pattern as existing */
            }
        }
        Err(SessionError::InsufficientBalance { balance, rate }) => {
            // 402 with session balance info
            (StatusCode::PAYMENT_REQUIRED, Json(json!({
                "error": "insufficient_session_balance",
                "message": "Session balance too low",
                "balance": balance,
                "rate_per_request": rate,
            }))).into_response()
        }
        Err(SessionError::InvalidSignature) | Err(SessionError::StaleTimestamp) => {
            (StatusCode::FORBIDDEN, Json(json!({"error": "invalid_session_auth"}))).into_response()
        }
        Err(SessionError::SessionNotFound) | Err(SessionError::SessionExpired) => {
            (StatusCode::PAYMENT_REQUIRED, Json(json!({"error": "session_expired_or_not_found"}))).into_response()
        }
        Err(_) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "session_error"}))).into_response()
        }
    }
}

// Direct payment: existing X-Payment-Tx path
else if mpp::has_payment_headers(&parts.headers) { /* existing code */ }

// No payment: 402
else { /* existing 402 code */ }
```

### 6. Wire Session Routes in `serve.rs`

In `cmd_serve`, add session routes to the gateway router before the fallback:

```rust
let mut gateway_app = Router::new()
    .route("/paygate/sessions/nonce", axum::routing::post(sessions::handle_nonce))
    .route("/paygate/sessions", axum::routing::post(sessions::handle_create_session))
    .merge(admin::receipt_route())
    .merge(admin::transactions_route())
    .fallback(gateway_handler)
    // ... rest same
```

### 7. DB Operations — Add to `db.rs`

Add new `WriteCommand` variants:
```rust
CreateSessionNonce { payer: String, nonce: String, expires_at: i64 },
CreateSession { record: FullSessionRecord },
DeductSessionBalance { id: String, amount: u64, reply: oneshot::Sender<Result<bool, DbError>> },
RefundSessionBalance { id: String, amount: u64 },
```

Add a `session_nonces` table to `schema.sql`:
```sql
CREATE TABLE IF NOT EXISTS session_nonces (
    nonce       TEXT PRIMARY KEY,
    payer_address TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL,
    consumed    INTEGER NOT NULL DEFAULT 0
);
```

Add to `DbReader`:
```rust
pub fn get_session_nonce(&self, nonce: &str) -> Result<Option<NonceRecord>, DbError>
pub fn get_session(&self, id: &str) -> Result<Option<FullSessionRecord>, DbError>
pub fn count_active_sessions_for_payer(&self, payer: &str) -> Result<u64, DbError>
```

Add to `DbWriter`:
```rust
pub async fn create_session_nonce(&self, payer: String, nonce: String, expires_at: i64) -> Result<(), DbError>
pub async fn create_session(&self, record: FullSessionRecord) -> Result<(), DbError>
pub async fn deduct_session_balance(&self, id: &str, amount: u64) -> Result<bool, DbError>
pub async fn refund_session_balance(&self, id: &str, amount: u64) -> Result<(), DbError>
```

The `deduct_session_balance` MUST return a bool via oneshot channel — `true` if rows were updated, `false` if insufficient balance/expired. This is the atomic operation.

Define `FullSessionRecord`:
```rust
pub struct FullSessionRecord {
    pub id: String,
    pub secret: String,
    pub payer_address: String,
    pub deposit_tx: String,
    pub nonce: String,
    pub deposit_amount: u64,
    pub balance: u64,
    pub rate_per_request: u64,
    pub requests_made: u64,
    pub created_at: i64,
    pub expires_at: i64,
    pub status: String,
}
```

### 8. Config Changes

In `config.rs`:
- Change `discount_percent` default from 50 to 0 (CEO plan: no discount for sessions)
- Add `no_charge_on_5xx: Vec<String>` to `PricingConfig` with `#[serde(default)]`
- Add `Config::is_no_charge_on_5xx(&self, endpoint: &str) -> bool`

### 9. Module Registration

In `crates/paygate-gateway/src/main.rs` (or lib.rs), add:
```rust
mod sessions;
```

### 10. Dependencies

Add to `crates/paygate-gateway/Cargo.toml`:
```toml
hmac = "0.12"
sha2 = "0.10"
rand = "0.8"
```
Check if these are already present; they may be.

### 11. Tests — At Least 10

Write tests in `sessions.rs` `#[cfg(test)] mod tests`:

1. **Nonce generation happy path**: POST /paygate/sessions/nonce with valid payer returns 200 with nonce
2. **Max concurrent sessions exceeded**: Insert 5 active sessions for a payer, then request nonce -> 429
3. **Create session with valid deposit**: Mock RPC returns valid receipt with correct memo, amount, payer -> 201 with sessionId + sessionSecret
4. **Create session with invalid nonce**: POST /paygate/sessions with unknown nonce -> 400
5. **HMAC verification happy path**: Set up session in DB, send request with valid HMAC -> deduct succeeds, proxy returns 200
6. **Invalid HMAC**: Wrong signature -> 403
7. **Stale timestamp**: Timestamp older than 60s -> 403
8. **Insufficient balance**: Session with balance < rate -> 402 with balance info
9. **Session expired**: Session with expires_at in the past -> 402
10. **no_charge_on_5xx refund**: Upstream returns 500 on endpoint with no_charge_on_5xx -> balance refunded, X-Payment-Refunded header present

For tests that need a mock RPC, use the same pattern as `serve.rs` tests — start a local axum server that returns canned JSON-RPC responses.

For tests that only need DB operations, use `init_db` with a temp path.

## Source Files to Read Before Building

- `crates/paygate-gateway/src/serve.rs` — gateway_handler you are modifying
- `crates/paygate-gateway/src/db.rs` — writer pattern you must follow
- `crates/paygate-gateway/src/config.rs` — config structs and parsing
- `crates/paygate-gateway/src/verifier.rs` — on-chain verification pattern to reuse
- `crates/paygate-gateway/src/server.rs` — AppState struct
- `crates/paygate-common/src/mpp.rs` — header constants (X-Payment-Session etc. already defined)
- `crates/paygate-common/src/hash.rs` — keccak256 helper
- `schema.sql` — sessions table schema
- `SPEC.md` section 4.3 — session protocol spec
