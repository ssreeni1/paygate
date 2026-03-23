# Build Brief: Transaction Explorer — Backend

## Overview

Add a `GET /paygate/transactions` API endpoint that returns recent verified payments with stats, enabling the frontend transaction feed. This is the data layer for the embedded transaction explorer feature.

## Changes

### 1. schema.sql — Add index for verified_at ordering

Add after the existing `idx_payments_payer` index:

```sql
CREATE INDEX IF NOT EXISTS idx_payments_verified ON payments(verified_at);
```

This index supports the `ORDER BY verified_at DESC` query used by the new endpoint.

### 2. db.rs — Add two new DbReader methods

Add to the `impl DbReader` block:

#### `recent_transactions(limit: u32, offset: u32) -> Result<Vec<PaymentRecord>, DbError>`

- Query: `SELECT id, tx_hash, payer_address, amount, token_address, endpoint, request_hash, quote_id, block_number, verified_at, status FROM payments ORDER BY verified_at DESC LIMIT ? OFFSET ?`
- Map rows to `PaymentRecord` using the same pattern as `get_payment()`
- Use `query_map` + collect pattern (same as `list_active_sessions()` and `revenue_by_endpoint()`)

#### `transaction_stats() -> Result<(u64, BaseUnits), DbError>`

- Query: `SELECT COUNT(*), COALESCE(SUM(amount), 0) FROM payments`
- Returns `(total_count, total_revenue)` as `(u64, BaseUnits)`
- Use `query_row` pattern (same as `revenue_summary()`)

### 3. admin.rs — Add `GET /paygate/transactions` handler

#### Route registration

Add a new public function (like the existing `receipt_route()`) that exposes the transactions endpoint on the main gateway router (NOT admin-only), so the GitHub Pages frontend can call it via CORS:

```rust
pub fn transactions_route() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/paygate/transactions", get(transactions_handler))
}
```

Wire this into the main gateway router wherever `receipt_route()` is merged.

#### Handler: `transactions_handler`

- Extract query params: `limit` (default 20, max 100), `offset` (default 0)
- Use `axum::extract::Query` with a small deserializable struct
- Call `state.db_reader.recent_transactions(limit, offset)`
- Call `state.db_reader.transaction_stats()`
- Build JSON response:

```json
{
  "transactions": [
    {
      "tx_hash": "0x...",
      "payer_address": "0x...",
      "amount": 1000,
      "amount_formatted": "0.001000",
      "endpoint": "POST /v1/search",
      "verified_at": 1234567890,
      "verified_at_iso": "2026-03-23T12:00:00Z",
      "status": "verified",
      "explorer_url": "https://explore.moderato.tempo.xyz/tx/0x..."
    }
  ],
  "total": 42,
  "total_revenue": 50000,
  "total_revenue_formatted": "$0.05"
}
```

Formatting notes:
- `amount_formatted`: divide by 1_000_000 (USDC has 6 decimals), format to 6 decimal places
- `total_revenue_formatted`: divide by 1_000_000, format as `$X.XX` (2 decimal places)
- `verified_at_iso`: use `chrono::DateTime::from_timestamp(verified_at, 0)` and `.to_rfc3339()`
- `explorer_url`: concatenate `https://explore.moderato.tempo.xyz/tx/` + tx_hash

#### Rate limiting

Reuse the existing rate limiting pattern from the codebase. Target: 60 requests/min per IP. If no existing rate limiter middleware is available, add a simple in-memory token bucket or use `tower_governor` / similar.

#### CORS

Ensure the `/paygate/transactions` route has CORS headers allowing requests from `https://ssreeni1.github.io`. If the main router already has CORS middleware, this is free. If not, add the appropriate `Access-Control-Allow-Origin` header.

### 4. Tests

Add to the existing test module(s) in `db.rs` and/or `admin.rs`:

| ID | Test | Description |
|----|------|-------------|
| T1 | `test_recent_transactions_ordered` | Insert 3 payments with different `verified_at` timestamps. Call `recent_transactions(10, 0)`. Assert the results are ordered by `verified_at` DESC. |
| T2 | `test_recent_transactions_empty_db` | Call `recent_transactions(10, 0)` on a fresh DB. Assert returns empty vec, no error. |
| T3 | `test_transaction_stats_correct` | Insert 3 payments with known amounts. Call `transaction_stats()`. Assert count == 3, revenue == sum of amounts. |
| T4 | `test_transactions_endpoint_json` | Use `axum::test` / tower::ServiceExt to send `GET /paygate/transactions`. Assert 200 status, valid JSON with `transactions` array and `total` field. |
| T5 | `test_transactions_limit_param` | Send `GET /paygate/transactions?limit=2`. Assert response contains at most 2 transactions. |
| T6 | `test_transactions_cors_headers` | Send `GET /paygate/transactions`. Assert response has `Access-Control-Allow-Origin` header. |

## Commit message

```
feat: add GET /paygate/transactions endpoint with recent payments and stats
```

## Key patterns to follow

- **DbReader methods**: Follow the `get_payment()` pattern for row mapping, and `revenue_by_endpoint()` for multi-row queries
- **Route registration**: Follow the `receipt_route()` pattern — a standalone function returning `axum::Router<AppState>`
- **Handler structure**: Follow `receipt_handler()` — extract state, query DB, return `(StatusCode, Json(...))`
- **Error handling**: Match on `Ok`/`Err` from DB calls, log errors with `tracing::error!`, return 500 with `{"error": "internal error"}`
