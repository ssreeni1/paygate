# Brief: Fill P0/P1 Test Coverage Gaps (~15 tests)

## Goal

Add ~15 tests to fill coverage gaps in admin.rs, webhook.rs, and db.rs. **DO NOT modify any production code** — only add `#[cfg(test)] mod tests { ... }` blocks and test functions.

## Context

- `main.rs` already has tests for admin health, metrics, receipts, transactions, webhooks, and some db operations
- `admin.rs` (249 LOC) has **0 tests** in its own file
- `webhook.rs` (83 LOC) has **0 tests** in its own file
- `db.rs` (627 LOC) has **0 tests** in its own file
- Tests that exist in main.rs cover some of these, but the modules themselves have no local test coverage

## Pattern Reference

Look at existing test patterns in `main.rs` (lines 1880-2785) for:
- `test_state_with_upstream()` helper — constructs an `AppState` with a real upstream server
- `insert_test_payment()` helper — inserts test data directly via SQL
- `tower::ServiceExt::oneshot()` pattern for testing axum routes
- DB test pattern: create temp DB path, open connection, run schema.sql, insert data, create `DbReader`, assert, cleanup

The schema file is at `schema.sql` (referenced as `include_str!("../../../schema.sql")` from the gateway crate).

## Tests to Write

### admin.rs — Add `#[cfg(test)] mod tests` block

These tests exercise admin.rs routes directly without going through main.rs:

**1. `test_health_returns_json_structure`**
- Build AppState with mock upstream + mock RPC (both reachable)
- Call `admin_router(state).oneshot(GET /paygate/health)`
- Assert: 200, body has `status`, `version`, `db`, `tempo_rpc`, `upstream`, `uptime_seconds` fields
- Assert: `status == "healthy"`

**2. `test_health_degraded_when_rpc_down`**
- Build AppState where rpc_urls point to unreachable address (127.0.0.1:1)
- Call health endpoint
- Assert: 503, `status == "degraded"`, `tempo_rpc == "error"`

**3. `test_metrics_returns_text_plain`**
- Call `GET /paygate/metrics`
- Assert: 200, content-type contains "text/plain"
- Assert: body is valid (empty or contains `#` comment lines)

**4. `test_transactions_returns_json_array`**
- Insert 2 test payments via SQL
- Call `GET /paygate/transactions`
- Assert: 200, body has `transactions` array with 2 items, `total == 2`

**5. `test_transactions_limit_parameter`**
- Insert 5 test payments
- Call `GET /paygate/transactions?limit=2`
- Assert: `transactions` array has 2 items, `total == 5`

**6. `test_transactions_empty_db`**
- No payments inserted
- Call `GET /paygate/transactions`
- Assert: 200, `transactions` is empty array, `total == 0`

**7. `test_receipt_404_for_unknown_tx`**
- Call `GET /paygate/receipts/0xnonexistent`
- Assert: 404, body has `error == "payment not found"`

### webhook.rs — Add `#[cfg(test)] mod tests` block

**8. `test_webhook_sends_post_on_payment`**
- Start a mock HTTP server that records received requests (use `Arc<AtomicBool>`)
- Create `WebhookSender` pointing to mock server
- Call `notify_payment_verified(tx_hash, payer, amount, endpoint)`
- Sleep 200ms, assert mock received the POST

**9. `test_webhook_failure_is_nonblocking`**
- Create `WebhookSender` pointing to unreachable address (127.0.0.1:1)
- Time the call to `notify_payment_verified`
- Assert: returns in < 50ms (fire-and-forget, spawns a tokio task)

**10. `test_webhook_respects_timeout`**
- Start a mock server that sleeps 10 seconds before responding
- Create `WebhookSender` with timeout_seconds = 1
- Call `notify_payment_verified`
- Sleep 2s, assert no panic (the task timed out gracefully)

### db.rs — Add `#[cfg(test)] mod tests` block

**11. `test_recent_transactions_returns_correct_order`**
- Create temp DB, insert 3 payments with different `verified_at` timestamps
- Call `reader.recent_transactions(10, 0)`
- Assert: returned in descending `verified_at` order

**12. `test_transaction_stats_returns_correct_totals`**
- Insert 3 payments with amounts 1000, 2000, 5000
- Call `reader.transaction_stats()`
- Assert: count == 3, revenue == 8000

**13. `test_recent_transactions_respects_limit`**
- Insert 5 payments
- Call `reader.recent_transactions(2, 0)`
- Assert: exactly 2 results returned

**14. `test_revenue_summary_filters_by_time`**
- Insert payments at different timestamps (some old, some recent)
- Call `reader.revenue_summary(recent_cutoff)`
- Assert: only recent payments counted

**15. `test_insert_and_retrieve_payment`**
- Insert a payment via `db_writer.insert_payment(record)`
- Retrieve via `reader.get_payment_by_tx_hash(hash)`
- Assert: all fields match

## Implementation Notes

### For admin.rs tests

You need to construct `AppState`. Copy the pattern from main.rs `test_state_with_upstream()`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn make_test_state() -> crate::server::AppState {
        let db_path = format!("/tmp/paygate_admin_test_{}.db", uuid::Uuid::new_v4());
        let (db_reader, db_writer) = crate::db::init_db(&db_path).unwrap();
        // ... full config construction ...
        crate::server::AppState { ... }
    }

    #[tokio::test]
    async fn test_health_returns_json_structure() {
        let state = make_test_state().await;
        let app = admin_router(state);
        let req = Request::builder()
            .method("GET")
            .uri("/paygate/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // ...
    }
}
```

### For db.rs tests

Use direct SQLite connection + `DbReader`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_db() -> (String, rusqlite::Connection) {
        let path = format!("/tmp/paygate_db_test_{}.db", uuid::Uuid::new_v4());
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(include_str!("../../../schema.sql")).unwrap();
        (path, conn)
    }

    #[test]
    fn test_recent_transactions_returns_correct_order() {
        let (path, conn) = setup_test_db();
        // insert data, create DbReader, assert...
        std::fs::remove_file(&path).ok();
    }
}
```

### For webhook.rs tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_webhook_sends_post() {
        // start mock axum server, create WebhookSender, call notify, assert
    }
}
```

## Rules

1. **DO NOT** modify any production code — only add test code
2. All test temp DB files must be cleaned up (`std::fs::remove_file`)
3. Use unique DB paths with `uuid::Uuid::new_v4()` to avoid test interference
4. Tests must be `#[tokio::test]` for async, `#[test]` for sync
5. All 15 tests must pass independently and in parallel (`cargo test`)

## Verification

```bash
cargo test 2>&1 | tail -10   # all tests pass
cargo test 2>&1 | grep "test result"  # count should increase by ~15
```
