# Prompt: Fill Test Coverage Gaps

Read the brief at `worktree-briefs/pane-fill-tests-brief.md`.

Then read these files to understand existing patterns and module APIs:
- `crates/paygate-gateway/src/admin.rs`
- `crates/paygate-gateway/src/webhook.rs`
- `crates/paygate-gateway/src/db.rs`
- `crates/paygate-gateway/src/server.rs` (for AppState definition)
- `crates/paygate-gateway/src/main.rs` (lines 1880-2785 for test patterns)

## Task

Add ~15 tests to `admin.rs`, `webhook.rs`, and `db.rs` as described in the brief. DO NOT modify any production code — only add `#[cfg(test)] mod tests { ... }` blocks.

## Steps

1. Read the brief thoroughly
2. Read the source files listed above to understand APIs and existing test patterns
3. Read `schema.sql` to understand the DB schema for test data insertion
4. Add 7 tests to `admin.rs` (health, metrics, transactions, receipts)
5. Add 3 tests to `webhook.rs` (send, non-blocking, timeout)
6. Add 5 tests to `db.rs` (ordering, stats, limit, time filter, round-trip)
7. Run `cargo test` — fix any compile errors
8. Verify all new tests pass
9. Run `cargo test 2>&1 | grep "test result"` to confirm the count increased by ~15
10. Commit with message: "test: add 15 tests for admin, webhook, and db coverage gaps"

## Constraints

- DO NOT modify production code — only add test modules
- Clean up temp DB files in every test
- Use unique DB paths with uuid to avoid test interference
- All tests must pass independently and in parallel
- Copy the `test_state_with_upstream` pattern from main.rs for admin tests
- Copy the `insert_test_payment` + `setup_test_db` pattern for db tests
