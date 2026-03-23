# Prompt: Build Transaction Explorer Backend

You are building the backend for the PayGate transaction explorer feature.

## Instructions

1. Read the build brief:
   - `/Users/saneel/projects/paygate/worktree-briefs/pane-tx-backend-brief.md`

2. Read these source files for context:
   - `/Users/saneel/projects/paygate/schema.sql` — existing schema, you will add an index
   - `/Users/saneel/projects/paygate/crates/paygate-gateway/src/db.rs` — existing DbReader methods, you will add two new ones
   - `/Users/saneel/projects/paygate/crates/paygate-gateway/src/admin.rs` — existing route patterns, you will add the transactions endpoint
   - `/Users/saneel/projects/paygate/crates/paygate-gateway/src/server.rs` — find where routes are mounted to wire in the new route
   - `/Users/saneel/projects/paygate/crates/paygate-common/src/types.rs` — PaymentRecord and BaseUnits type definitions

3. Implement all changes described in the brief:
   - Add the `idx_payments_verified` index to `schema.sql`
   - Add `recent_transactions()` and `transaction_stats()` methods to `DbReader` in `db.rs`
   - Add `transactions_route()` and `transactions_handler()` to `admin.rs`
   - Wire the new route into the main gateway router in `server.rs`
   - Add all 6 tests (T1-T6)

4. Build and test:
   - Run `cargo build` to verify compilation
   - Run `cargo test` to verify all tests pass
   - Fix any issues until both pass cleanly

5. Commit with message:
   ```
   feat: add GET /paygate/transactions endpoint with recent payments and stats
   ```

## Important notes

- Follow existing code patterns exactly (error handling, row mapping, route registration)
- The transactions endpoint goes on the PUBLIC router (like `receipt_route()`), not the admin router
- USDC uses 6 decimal places: amount 1000 = 0.001000 USDC
- The `amount_formatted` field should have 6 decimal places, `total_revenue_formatted` should have 2
- Use `chrono` for ISO timestamp formatting (already a dependency)
- For query params, use `#[derive(Deserialize)]` struct with `axum::extract::Query`
