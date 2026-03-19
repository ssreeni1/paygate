You are writing the remaining test cases for PayGate to increase coverage from 36% to ~70%+ of the 69 specified test cases.

Read the QA report at .gstack/qa-reports/qa-report-paygate-2026-03-19.md — section 3 lists exactly which of the 69 test cases are NOT covered. Focus on those.

Read the existing tests first to match conventions:
- crates/paygate-common/src/hash.rs (test module at bottom)
- crates/paygate-gateway/src/verifier.rs (test module)
- crates/paygate-gateway/src/mpp.rs (test module)
- crates/paygate-gateway/src/proxy.rs (test module)
- crates/paygate-gateway/src/config.rs (test module)
- crates/paygate-gateway/src/rate_limit.rs (test module)
- crates/paygate-gateway/src/main.rs (test module at bottom)
- sdk/tests/hash.test.ts
- sdk/tests/client.test.ts

Also read the source files you're testing to understand what to assert.

Priority order (implement as many as you can):

**P0 — Must have:**
- T15: Upstream 5xx returns 502 + receipt (proxy.rs test)
- T18: Health endpoint returns correct JSON for healthy + degraded states (integration test)
- T19: Metrics endpoint returns Prometheus format (integration test)
- T20: Graceful shutdown drains in-flight requests
- T26: Invalid/empty RPC receipt returns 400 with "tx not yet indexed" (already partially covered but verify)

**P1 — Should have:**
- T13: Wrong recipient (dedicated test, not just side effect of decode_transfer_events)
- T21: RPC failover — primary timeout, secondary succeeds
- Webhook delivery test — payment triggers webhook POST
- Webhook failure test — bad webhook URL doesn't block response
- Receipt endpoint found — GET /paygate/receipts/{known_tx_hash} returns 200 + payment data
- Receipt endpoint not found — GET /paygate/receipts/{unknown} returns 404
- X-Payment-Cost header test — verify response includes cost header with correct amount

**P2 — Nice to have:**
- CLI tests: paygate init creates valid TOML
- CLI tests: paygate revenue returns correct format
- Config reload test: SIGHUP triggers config swap
- Backpressure test: bounded channel full returns 503
- 402 flood rate limiter test: excessive 402 requests get 429

**Test conventions to follow:**
- Rust tests: `#[cfg(test)] mod tests { }` at bottom of each source file
- Use `#[tokio::test]` for async tests
- Create AppState with all fields (config, db_reader, db_writer, http_client, rate_limiter, webhook_sender, prometheus_handle, started_at)
- Use real echo servers (bind to 127.0.0.1:0 for random port) for integration tests
- For DB tests, use temp file paths: `/tmp/paygate_test_{uuid}.db`
- TS tests: vitest with describe/it/expect

After writing tests, run `cargo test` and `cd sdk && npm test` to verify all pass. Fix any failures.

Only modify test modules and test files. Do NOT modify production source code.

Commit when done with message: "test: add P0/P1 test cases from QA coverage gap analysis"
