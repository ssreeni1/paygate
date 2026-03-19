# PayGate QA Validation Report

**Date:** 2026-03-19
**Scope:** Full implementation validation against SPEC.md and review documents
**Status:** 14 bugs found, significant test coverage gaps

---

## 1. Spec Compliance Check

### 3.2 Middleware Stack Order

**Status: PARTIAL COMPLIANCE**

The spec defines 7 middleware layers in order:
1. Rate Limiter -- IMPLEMENTED (rate_limit.rs, middleware layer)
2. MPP Negotiator -- IMPLEMENTED (mpp.rs + gateway_handler in main.rs)
3. Payment Verifier -- IMPLEMENTED (verifier.rs)
4. Payer Binder -- IMPLEMENTED (inside verifier.rs, step 6)
5. Header Sanitizer -- IMPLEMENTED (proxy.rs, strips X-Payment-* before forwarding)
6. Reverse Proxy -- IMPLEMENTED (proxy.rs)
7. Response Logger -- IMPLEMENTED (in gateway_handler after proxy response)
8. Receipt Injector -- IMPLEMENTED (proxy.rs, adds X-Payment-Receipt + X-Payment-Cost)

**Issue:** The middleware stack is not implemented as discrete tower layers. Instead, it is a monolithic `gateway_handler` function that sequentially performs all steps. This is functionally equivalent but architecturally divergent from the spec's tower layer design. The rate limiter IS a proper axum middleware layer, but everything else is in one handler function. This is acceptable for MVP but makes it harder to reorder or inject new layers.

### 4.1 402 Response Format

**Status: COMPLIANT**

Verified in `crates/paygate-gateway/src/mpp.rs`:
- All specified headers present: X-Payment-Required, X-Payment-Amount, X-Payment-Decimals, X-Payment-Token, X-Payment-Recipient, X-Payment-Network, X-Payment-Chain-Id, X-Payment-Quote-Id, X-Payment-Quote-Expires, X-Payment-Methods
- JSON body includes: error, message, help_url, pricing object with all required fields
- `message` field includes human-readable instruction
- `help_url` points to ssreeni1.github.io/paygate
- Quote IDs generated and stored in DB
- Unit test confirms format (mpp.rs test_402_response_format)

### 4.2 Payment Verification Steps (10 Steps)

**Status: COMPLIANT (all 10 steps implemented)**

Verified in `crates/paygate-gateway/src/verifier.rs::verify_payment()`:
1. Fetch tx receipt via eth_getTransactionReceipt -- line 247
2. Decode TIP-20 Transfer event logs -- line 287 (decode_transfer_events)
3. Verify TransferWithMemo log / extract memo -- line 296 (decode_memo_from_logs)
4. Verify memo with constant-time comparison -- line 307 (constant_time_eq)
5. Verify amount >= price (with quote honor logic) -- line 314-329
6. Verify X-Payment-Payer matches on-chain from -- line 331-343
7. Check replay protection (SQLite UNIQUE) -- line 346-353
8. Verify tx age < tx_expiry_seconds -- line 355-362
9. Record payment in DB -- line 364-386
10. Consume quote after verification -- line 388-391

**Issue:** Spec step says "Reject transactions with multiple matching Transfer events" -- IMPLEMENTED in decode_transfer_events (returns AmbiguousTransfer when matches.len() > 1).

### 5.1 Config Fields

**Status: COMPLIANT**

All config sections from the spec are parsed and validated:
- `[gateway]` - listen, admin_listen, upstream, upstream_timeout_seconds, max_response_body_bytes
- `[tempo]` - network, rpc_urls, failover_timeout_ms, rpc_pool_max_idle, rpc_timeout_ms, chain_id, private_key_env, accepted_token
- `[provider]` - address, name, description
- `[sponsorship]` - enabled, sponsor_listen, budget_per_day, max_per_tx
- `[sessions]` - enabled, discount_percent, minimum_deposit, max_duration_hours, auto_refund, max_concurrent_per_payer
- `[pricing]` - default_price, quote_ttl_seconds, endpoints, dynamic, tiers
- `[rate_limiting]` - requests_per_second, per_payer_per_second, min_payment_interval_ms
- `[security]` - require_payment_before_forward, max_request_body_bytes, tx_expiry_seconds, replay_protection
- `[webhooks]` - payment_verified_url, timeout_seconds
- `[storage]` - request_log_retention_days

Validation at startup: addresses validated (0x + 42 chars + hex), prices non-negative, rpc_urls non-empty, upstream required, webhook SSRF protection.

**Missing from validation:** `min_payment_interval_ms` is parsed but never enforced in the rate limiter. See Bug #6.

### 8. Data Model (Schema)

**Status: COMPLIANT**

Schema in `/Users/saneel/projects/paygate/schema.sql` matches SPEC.md section 8 exactly:
- payments table with all columns and UNIQUE on tx_hash
- quotes table with expiry index
- sessions table with secret (plaintext, per ENG-REVIEW Issue 1 resolution)
- request_log table with created_at and payer indices
- WAL mode set via PRAGMA
- `IF NOT EXISTS` used for idempotent creation

### 9. CLI Commands

**Status: MOSTLY COMPLIANT**

| Command | Implemented | Notes |
|---------|:-----------:|-------|
| `paygate init` | YES | Interactive wizard with validation |
| `paygate serve` | YES | Full server startup |
| `paygate status` | YES | Checks gateway, upstream, RPC, DB |
| `paygate pricing` | YES | Text table output |
| `paygate pricing --html` | YES | Static HTML generation |
| `paygate sessions` | YES | Lists active sessions from DB |
| `paygate revenue` | YES | 24h/7d/30d summary + top endpoints |
| `paygate wallet` | YES | On-chain balance + 24h income |
| `paygate demo` | YES | Alias for `paygate test --demo` pattern |
| `paygate test` | PARTIAL | **Stub implementation** -- see Bug #1 |
| `paygate contract deploy` | NOT IMPLEMENTED | See note below |
| `paygate contract register` | NOT IMPLEMENTED | See note below |

**Note:** `paygate contract deploy` and `paygate contract register` are listed in spec section 9, but contracts are listed as "Optional" in section 6 and Wave 3 in section 14. These are NOT in Wave 1 scope, so their absence is acceptable.

### 10. Security Mitigations

| Mitigation | Implemented | Location |
|-----------|:-----------:|----------|
| Replay protection (UNIQUE tx_hash) | YES | verifier.rs line 346, schema.sql |
| Front-running prevention (payer binding) | YES | verifier.rs line 331-343 |
| Stale tx rejection | YES | verifier.rs line 355-362 |
| Amount verification | YES | verifier.rs line 314-329 |
| Wrong recipient check | YES | verifier.rs decode_transfer_events |
| Memo binding | YES | verifier.rs line 304-312 |
| Key from env var | YES | config.rs private_key_env field |
| Ambiguous tx rejection | YES | verifier.rs decode_transfer_events |
| Webhook SSRF | YES | config.rs validate_webhook_url |
| Header sanitization | YES | proxy.rs line 38-47 |
| Constant-time HMAC | YES | verifier.rs constant_time_eq (line 30-38) |
| Request body size limit | YES | main.rs gateway_handler |
| Response body size limit | YES | proxy.rs line 91-103 |

### 12. Observability

| Item | Implemented | Notes |
|------|:-----------:|-------|
| Health endpoint | YES | admin.rs /paygate/health |
| Prometheus metrics | YES | admin.rs /paygate/metrics |
| paygate_payments_verified_total | YES | metrics.rs |
| paygate_payment_verification_duration_seconds | YES | metrics.rs |
| paygate_upstream_request_duration_seconds | YES | metrics.rs |
| paygate_revenue_total_base_units | YES | metrics.rs |
| paygate_active_sessions | YES | metrics.rs |
| paygate_rate_limit_rejected_total | YES | metrics.rs |
| paygate_rpc_errors_total | YES | metrics.rs |
| paygate_db_errors_total | YES | metrics.rs |
| paygate_db_writer_queue_depth | YES | metrics.rs (but see Bug #7) |
| paygate_webhook_delivery_total | YES | metrics.rs |
| paygate_quotes_active | YES | metrics.rs (not renamed per CEO additions) |
| paygate_config_reloads_total | YES | metrics.rs |
| Receipts endpoint | YES | admin.rs /paygate/receipts/{tx_hash} |
| Structured JSON logging | YES | main.rs tracing_subscriber JSON |

**Issue:** `paygate_quotes_active` was recommended to be renamed to `paygate_quotes_unexpired` in eng-review-ceo-additions.md. Not renamed. Low priority.

**Issue:** The receipts endpoint is on the admin router, not the main gateway port. Spec says "Public endpoint on main port." See Bug #8.

### 14. Wave 1 Scope

All items listed in Wave 1 scope are present except:

| Item | Status |
|------|--------|
| Single Rust binary (paygate serve) | YES |
| TOML config with static pricing | YES |
| Config validation + SIGHUP reload | PARTIAL -- validation YES, SIGHUP NOT IMPLEMENTED (Bug #2) |
| 402 responses with quote IDs | YES |
| On-chain payment verification | YES |
| RPC failover | YES |
| Payer binding | YES |
| Request hash computation | YES |
| Replay protection | YES |
| Basic rate limiting | YES |
| paygate init wizard | YES |
| paygate demo | YES |
| paygate pricing --html | YES |
| paygate wallet | YES |
| TypeScript client SDK | YES |
| paygate revenue | YES |
| paygate test e2e | STUB (Bug #1) |
| Free-endpoint passthrough | YES |
| Request logging with retention | YES |
| Receipt verification endpoint | YES (wrong port - Bug #8) |
| X-Payment-Cost response header | YES |
| Webhook on payment | YES |
| Health check endpoint | YES |
| Prometheus metrics | YES |
| Structured JSON logging | YES |
| Graceful shutdown | YES |
| Defensive error handling | YES |

---

## 2. Error Handling Audit

Cross-referencing every error in error-rescue-registry.md:

| Error | Defined in Code | Correct HTTP Status | Retry-After | Notes |
|-------|:---:|:---:|:---:|-------|
| reqwest::Timeout (RPC) | YES (RpcError) | 503 YES | YES (2) | main.rs line 434-445 |
| TxNotFound | YES | 400 YES | YES (1) | main.rs line 421-432 |
| RpcError | YES | 503 YES | YES (2) | main.rs line 434-445 |
| InvalidTransfer | YES | 400 YES | NO | main.rs line 488-495 |
| AmbiguousTransfer | YES | 400 YES | NO | main.rs line 496-503 |
| InsufficientPayment | YES | 402 YES | NO | main.rs line 463-470 |
| PayerMismatch | YES | **403 YES** | NO | main.rs line 455-462 |
| ReplayDetected | YES | **409 YES** | NO | main.rs line 447-454 |
| ExpiredTransaction | YES | 400 YES | NO | main.rs line 472-479 |
| MemoMismatch | YES | 400 YES | NO | main.rs line 480-487 |
| QuoteExpired | YES | 402 (re-price) YES | NO | main.rs line 504-506 |
| SqliteError | YES (DbError) | 503 via RpcError | PARTIAL | verifier.rs line 350-353 wraps as RpcError |
| SqliteBusy | YES (Backpressure) | 503 | NOT PRESENT | Backpressure path does not return 503 to user (Bug #3) |
| Upstream timeout | YES | 504 YES | NO | main.rs line 327-330 |
| ConnectionError | YES | 502 YES | NO | main.rs line 337-342 |
| WebhookError | YES | N/A (transparent) | N/A | webhook.rs -- fire and forget |
| PoolTimeout | Covered by RpcError | 503 | YES (2) | Same path |
| RpcRateLimited | Covered by RpcError | 503 | YES (2) | Same path |
| JsonParseError | Covered by RpcError | 503 | YES (2) | Same path |

**Critical Rule Compliance:**
1. No bare catch-all -- COMPLIANT (every VerificationResult variant has a specific handler)
2. Every error logs context -- PARTIAL (verifier.rs only logs on success at line 398-405; failed verifications do NOT log tx_hash/payer/latency)
3. Every 503 includes Retry-After -- PARTIAL (RPC errors YES; SQLite backpressure path is missing - Bug #3)
4. HMAC constant-time -- YES (verifier.rs constant_time_eq, line 30-38)
5. Webhook never blocks -- YES (tokio::spawn with semaphore)

---

## 3. Test Coverage Gap Analysis

### Summary: 25 of 69 test cases covered, 44 NOT covered

### ENG-REVIEW Test Cases (T1-T29)

| # | Test | Covered | Location |
|---|------|:---:|----------|
| T1 | Rate limiter rejects at 429 | YES | rate_limit.rs test_rate_limiter_rejects_at_threshold |
| T2 | Free endpoint bypasses payment | YES | main.rs test_free_endpoint_bypasses_payment |
| T3 | 402 generation format | YES | mpp.rs test_402_response_format |
| T4 | Quote honored within TTL | YES | verifier.rs test_quote_honored_within_ttl |
| T5 | Quote expired fallback | YES | verifier.rs test_quote_expired_fallback |
| T6 | Receipt fetch / decode Transfer | YES | verifier.rs test_valid_payment_verification |
| T7 | Memo verify | YES | verifier.rs test_memo_mismatch_detection |
| T8 | Replay protection | YES | verifier.rs test_replay_rejection |
| T9 | Payer binding mismatch | YES | verifier.rs test_payer_mismatch_detection |
| T10 | TX age check | YES | verifier.rs test_expired_transaction_detection |
| T11 | Multiple Transfer events | YES | verifier.rs test_ambiguous_transfer_detection |
| T12 | Wrong amount | YES | verifier.rs test_insufficient_amount_detection |
| T13 | Wrong recipient | PARTIAL | Tested via decode_transfer_events filtering; no dedicated "wrong recipient returns error" test |
| T14 | Header sanitization | YES | proxy.rs test_header_sanitization |
| T15 | Upstream 5xx | NO | No test for upstream 5xx forwarding behavior |
| T16 | Request hash (shared vectors) | PARTIAL | Rust: basic tests in hash.rs, TS: hash.test.ts validates input_hex. But vectors lack expected_hash values (Bug #4) |
| T17 | Config parsing | YES | config.rs has tests for minimal, invalid, price parsing, endpoint pricing |
| T18 | Health endpoint | NO | No test for health endpoint |
| T19 | Metrics endpoint | NO | No test for metrics endpoint |
| T20 | Graceful shutdown | NO | No test for SIGTERM drain |
| T21 | RPC failover | YES | verifier.rs test_rpc_error_handling (tests failure); no failover-to-secondary test |
| T22 | TS SDK auto-pay | YES | sdk/tests/client.test.ts |
| T23 | TS SDK requestHash | YES | sdk/tests/hash.test.ts |
| T24 | paygate test e2e | NO | Stub implementation, no real e2e test |
| T25 | SQLite concurrency | NO | No concurrent insert test |
| T26 | Invalid RPC receipt (null) | YES | verifier.rs test_null_receipt_tx_not_found |
| T27 | Malformed event logs | PARTIAL | decode_transfer_events skips malformed logs, but no explicit "decode failure does not panic" test |
| T28 | SQLite write failure | NO | No disk-full simulation test |
| T29 | Upstream response OOM | NO | No response body size limit test |

### CEO Review Test Cases (1-30)

| # | Test | Covered | Notes |
|---|------|:---:|-------|
| 1 | Cross-language hash parity | PARTIAL | Input encoding verified, but no expected_hash in vectors |
| 2 | 402 response format | YES | |
| 3 | Valid payment -> 200 | YES | |
| 4 | Replay rejection | YES | |
| 5 | Payer mismatch -> 403 | YES | |
| 6 | Insufficient amount | YES | |
| 7 | Quote expiry re-price | YES | |
| 8 | Memo mismatch | YES | |
| 9 | Ambiguous tx | YES | |
| 10 | Tx too old | YES | |
| 11 | Null receipt | YES | |
| 12 | RPC error -> 503 | YES | |
| 13 | Free endpoint | YES | |
| 14 | Rate limiting | YES | |
| 15 | Header sanitization | YES | |
| 16 | Upstream timeout | NO | |
| 17 | Upstream 5xx pass-through | NO | |
| 18 | Config validation | YES | |
| 19 | Webhook delivery | NO | |
| 20 | Webhook failure transparent | NO | |
| 21 | Receipt endpoint found | NO | |
| 22 | Receipt not found | NO | |
| 23 | X-Payment-Cost header | NO | |
| 24 | paygate init (fresh) | NO | |
| 25 | paygate init (exists) | NO | |
| 26 | paygate revenue | NO | |
| 27 | paygate wallet | NO | |
| 28 | Graceful shutdown | NO | |
| 29 | HMAC constant-time | NO | (function exists, but no timing test) |
| 30 | Webhook URL SSRF | YES | config.rs test_private_webhook_url_rejected |

### CEO Additions Test Cases (40 cases) -- ALL NOT COVERED

None of the 40 additional test cases from eng-review-ceo-additions.md have corresponding test implementations:
- SE1-T1/T2 (demo command) -- NO
- SE2-T1 through SE2-T5 (receipts endpoint) -- NO
- SE3-T1 through SE3-T4 (pricing HTML) -- NO
- SE5-T1 through SE5-T4 (X-Payment-Cost) -- NO
- SE6-T1 through SE6-T4 (wallet command) -- NO
- WH-T1 through WH-T8 (webhook system) -- NO
- CR-T1 through CR-T5 (config reload) -- NO
- BP-T1 through BP-T4 (backpressure) -- NO
- RL-T1 through RL-T4 (retention cleanup) -- NO

### Missing Test Types

- **No integration tests** (full middleware stack with mock RPC) -- only unit tests and component tests
- **No load tests** -- no k6/wrk scripts
- **No SIGHUP/config reload tests** -- feature not implemented
- **No CLI integration tests** -- no testing of actual CLI output
- **No contract integration tests** -- PayGateRegistry.t.sol exists and is comprehensive (15 tests)

---

## 4. Cross-Language Hash Parity

### requestHash

**Rust** (`crates/paygate-common/src/hash.rs`):
```
input = method.as_bytes() + b' ' + path.as_bytes() + b'\n' + body
keccak256(input)
```

**TypeScript** (`sdk/src/hash.ts`):
```
input = TextEncoder.encode(method) + [0x20] + TextEncoder.encode(path) + [0x0a] + TextEncoder.encode(body)
keccak256(input)
```

**Verdict: IDENTICAL** -- Both use UTF-8 encoding, same concatenation order (method + space + path + newline + body), same keccak256.

### paymentMemo

**Rust** (`crates/paygate-common/src/hash.rs`):
```
input = b"paygate" + quote_id.as_bytes() + request_hash.as_slice()  // raw 32 bytes
keccak256(input)
```

**TypeScript** (`sdk/src/hash.ts`):
```
input = TextEncoder.encode('paygate') + TextEncoder.encode(quoteId) + toBytes(reqHash)  // raw 32 bytes from hex
keccak256(input)
```

**Verdict: IDENTICAL** -- Both use "paygate" as UTF-8 prefix, quote_id as UTF-8, and requestHash as raw 32 bytes (not hex string). The TS `toBytes(reqHash)` correctly converts the hex string back to raw bytes before concatenation.

### Shared Test Vectors

The file `tests/fixtures/request_hash_vectors.json` exists and is used by TS tests. However, it lacks `expected_hash` fields -- it only verifies `input_hex` (the pre-hash input bytes). The Rust tests in hash.rs do NOT reference this fixture file at all.

**Bug #4:** No actual cross-language hash comparison. The vectors verify input encoding parity but do not lock in expected keccak256 output hashes. Both test suites verify determinism independently but never assert the same output for the same input.

---

## 5. Integration Point Review

### main.rs Wiring

The gateway handler correctly wires:
1. Rate limiter as axum middleware layer (line 189-192)
2. Gateway handler as fallback (line 188)
3. Admin router with health, metrics, receipts (line 184)
4. Cleanup task spawned (line 196-199)
5. Graceful shutdown via SIGTERM/CTRL+C (line 263-274)
6. Both gateway and admin servers run via tokio::select (line 281-292)

### AppState Fields

All fields needed by all modules are present:
- config (ArcSwap) -- used by all handlers
- db_reader / db_writer -- used by verifier, mpp, admin
- http_client -- used by verifier (RPC), proxy (upstream)
- rate_limiter -- used by rate_limit middleware
- webhook_sender -- used by gateway_handler
- prometheus_handle -- used by admin metrics handler
- started_at -- available but not used in health response (minor gap)

### Unused Imports / Dead Code

1. `crate::metrics::set_active_sessions` -- defined but never called (Bug #9)
2. `crate::metrics::set_writer_queue_depth` -- defined but never called (Bug #7)
3. `crate::metrics::set_active_quotes` -- defined but never called (Bug #10)
4. `crate::metrics::record_config_reload` -- defined but never called (Bug #11)
5. `DbWriter::queue_depth()` -- always returns 0, never called (Bug #7)
6. `rate_limit::RateLimiter::check_402_flood` -- defined but never called (Bug #12)

### Missing Module Declarations

No missing `mod` declarations found. All source files are properly declared in main.rs.

---

## 6. Bug List

### Bug #1: paygate test/demo is a stub
- **File:** `/Users/saneel/projects/paygate/crates/paygate-gateway/src/main.rs`, lines 1003-1028
- **What's wrong:** `cmd_test()` prints hardcoded output instead of performing actual testnet interactions. Line 1021 says `// TODO: Implement actual testnet interactions`. Even with `PAYGATE_TEST_KEY` set, steps 2-6 just print fake success messages.
- **Severity:** HIGH -- This is listed as a Wave 1 deliverable. The spec says "End-to-end test against Tempo testnet."
- **Fix:** Implement actual testnet flow: start echo server, start gateway, send 402 request, fund wallet, pay, verify.

### Bug #2: SIGHUP config reload not implemented
- **File:** `/Users/saneel/projects/paygate/crates/paygate-gateway/src/main.rs`
- **What's wrong:** The spec (5.3) and CEO additions (Section 3) require config reload on SIGHUP using ArcSwap. The ArcSwap is set up in AppState, but no SIGHUP handler is registered. Config is loaded once at startup and never reloaded.
- **Severity:** MEDIUM -- Listed as Wave 1 scope in section 14.
- **Fix:** Add a tokio task that listens for SIGHUP, re-reads paygate.toml, validates, and swaps config via ArcSwap.

### Bug #3: DB backpressure does not return 503 to client
- **File:** `/Users/saneel/projects/paygate/crates/paygate-gateway/src/main.rs`, lines 390-401
- **What's wrong:** When `db_writer.log_request()` returns Backpressure error, it is silently ignored with `let _ = state.db_writer.log_request(...)`. However, when `db_writer.insert_payment()` returns Backpressure (verifier.rs line 380-386), it correctly returns an error. The issue is that the gateway_handler does not convert the insert_payment backpressure into a proper 503 response. The error is mapped to `RpcError("database write error: ...")` which becomes 503 -- this is correct but misleading (it's not an RPC error). Additionally, there is no Retry-After header on this 503 path.
- **Severity:** MEDIUM
- **Fix:** Add a dedicated backpressure match arm in gateway_handler that returns 503 with Retry-After: 1 header, similar to the RpcError handler.

### Bug #4: Shared test vectors lack expected hashes
- **File:** `/Users/saneel/projects/paygate/tests/fixtures/request_hash_vectors.json`
- **What's wrong:** The vectors contain `input_hex` but no `expected_hash` field. The JSON file's `notes` section says "Run tests to populate the expected_hash fields, then lock them in" -- this was never done. Rust hash.rs tests do NOT read this fixture file at all. TS tests only verify input_hex encoding, not the keccak256 output.
- **Severity:** HIGH -- Cross-language hash parity is the #1 P0 test in the CEO review test plan. Without locked-in expected hashes, a keccak256 implementation difference would go undetected.
- **Fix:** Add `expected_hash` fields to each vector, add a Rust test that reads the JSON fixture and asserts output matches, add a TS test that does the same.

### Bug #5: pricing --html does not HTML-escape values (XSS)
- **File:** `/Users/saneel/projects/paygate/crates/paygate-gateway/src/main.rs`, lines 744-807
- **What's wrong:** `print_pricing_html` interpolates `endpoint` names and `provider_name` directly into HTML without escaping. The eng-review-ceo-additions.md (SE-3) explicitly requires HTML-escaping to prevent XSS from malicious endpoint names like `POST /v1/<script>alert(1)</script>`.
- **Severity:** MEDIUM
- **Fix:** Add HTML entity escaping for `<`, `>`, `&`, `"`, `'` before interpolating config values into the HTML template.

### Bug #6: min_payment_interval_ms not enforced
- **File:** `/Users/saneel/projects/paygate/crates/paygate-gateway/src/rate_limit.rs`
- **What's wrong:** The config field `rate_limiting.min_payment_interval_ms` (spec 5.1, default 100ms) is parsed in config.rs but never used in the rate limiter. The rate limiter only enforces `requests_per_second` and `per_payer_per_second`, not the minimum interval between payment attempts.
- **Severity:** LOW -- The per_payer_per_second limit provides similar protection.
- **Fix:** Add a per-payer token bucket or sliding window that enforces the minimum interval.

### Bug #7: DB writer queue depth always returns 0
- **File:** `/Users/saneel/projects/paygate/crates/paygate-gateway/src/db.rs`, lines 299-304
- **What's wrong:** `DbWriter::queue_depth()` always returns 0 with a TODO comment. The `set_writer_queue_depth` metric function is never called. The spec requires `paygate_db_writer_queue_depth` gauge for backpressure monitoring.
- **Severity:** LOW -- Metric exists but always reads 0.
- **Fix:** Track channel capacity on each send/receive, or use `CHANNEL_CAPACITY - sender.capacity()`.

### Bug #8: Receipts endpoint on wrong port
- **File:** `/Users/saneel/projects/paygate/crates/paygate-gateway/src/admin.rs`, line 16
- **What's wrong:** The `/paygate/receipts/{tx_hash}` endpoint is registered on the admin router (admin port 8081), not the main gateway port (8080). The spec (12.4) says "Public endpoint on main port. Rate-limited (100 req/min per IP)."
- **Severity:** MEDIUM -- Consumers cannot query receipts from the public-facing port.
- **Fix:** Move the receipts route to the gateway router or add it to both routers. Apply the IP-based rate limiter (check_402_flood or a dedicated one) to it on the main port.

### Bug #9: Active sessions gauge never updated
- **File:** `/Users/saneel/projects/paygate/crates/paygate-gateway/src/metrics.rs`, line 23
- **What's wrong:** `set_active_sessions()` is defined but never called from anywhere in the codebase. The `paygate_active_sessions` gauge will always be 0 (or absent).
- **Severity:** LOW
- **Fix:** Call `set_active_sessions()` periodically in the cleanup task, or in the health handler.

### Bug #10: Active quotes gauge never updated
- **File:** `/Users/saneel/projects/paygate/crates/paygate-gateway/src/metrics.rs`, line 53
- **What's wrong:** `set_active_quotes()` is defined but never called. The `paygate_quotes_active` gauge will always be 0.
- **Severity:** LOW
- **Fix:** Call in the cleanup task every 5 minutes (as recommended in eng-review-ceo-additions.md).

### Bug #11: Config reload metrics never recorded
- **File:** `/Users/saneel/projects/paygate/crates/paygate-gateway/src/metrics.rs`, line 58
- **What's wrong:** `record_config_reload()` is defined but never called (because SIGHUP reload is not implemented -- see Bug #2).
- **Severity:** LOW (dependent on Bug #2)
- **Fix:** Implement SIGHUP reload and call this function on success/failure.

### Bug #12: 402 flood rate limiter never applied
- **File:** `/Users/saneel/projects/paygate/crates/paygate-gateway/src/rate_limit.rs`, line 55
- **What's wrong:** `RateLimiter::check_402_flood()` is defined (1000/min per IP) but never called in the middleware. The spec (10.3) requires "IP-based rate limit on 402 responses (1000/min)."
- **Severity:** MEDIUM -- Without this, attackers can flood 402 discovery requests without cost.
- **Fix:** Apply `check_402_flood()` in the gateway handler or rate limit middleware when generating 402 responses.

### Bug #13: Rust client SDK is entirely stubbed
- **File:** `/Users/saneel/projects/paygate/crates/paygate-client/src/client.rs` and `discovery.rs`
- **What's wrong:** Both files contain only stub comments: `// Stub -- implemented in feat/verifier worktree.` The spec (7.2) includes the Rust client SDK in the project structure and section 14 implies it is in scope.
- **Severity:** MEDIUM -- The TypeScript SDK IS implemented, but the Rust client SDK is not.
- **Fix:** Implement the Rust client SDK or explicitly document it as deferred.

### Bug #14: `format_usd` uses floating point
- **File:** `/Users/saneel/projects/paygate/crates/paygate-common/src/types.rs`, lines 111-115
- **What's wrong:** `format_usd()` uses `f64` division: `let cents = (base_units as f64) / (divisor as f64)`. The eng-review-ceo-additions.md (SE-5) explicitly says "Do NOT use floating point -- format from integer base units." While `format_amount()` (line 103-108) correctly uses integer math, `format_usd()` (used extensively in CLI output) uses f64. For values near `u64::MAX`, this will produce incorrect results.
- **Severity:** LOW -- For typical USDC amounts (< 10^12 base units), f64 has sufficient precision. But it violates the spec requirement.
- **Fix:** Rewrite `format_usd` using integer division: `format!("${}.{:02}", base_units / divisor, (base_units % divisor) / (divisor / 100))`.

---

## Summary

### By Severity

| Severity | Count | Bug IDs |
|----------|:---:|---------|
| Critical | 0 | |
| High | 2 | #1 (test stub), #4 (hash vectors) |
| Medium | 6 | #2 (SIGHUP), #3 (backpressure), #5 (XSS), #8 (receipts port), #12 (402 flood), #13 (Rust SDK) |
| Low | 6 | #6, #7, #9, #10, #11, #14 |

### Test Coverage

- **25 of 69 specified test cases implemented** (36% coverage)
- All 40 CEO-additions test cases are unimplemented
- No integration tests exercising the full middleware stack
- No load tests
- No CLI output tests
- Cross-language hash parity test is incomplete (no expected hash assertion)

### Recommendations (Priority Order)

1. **Fix Bug #4** -- Add expected_hash to test vectors and make both Rust and TS assert against them
2. **Fix Bug #1** -- Implement real testnet e2e test
3. **Fix Bug #8** -- Move receipts endpoint to main port
4. **Fix Bug #2** -- Implement SIGHUP config reload
5. **Fix Bug #5** -- HTML-escape pricing page values
6. **Fix Bug #12** -- Wire up 402 flood rate limiter
7. Implement the remaining 44 test cases, prioritizing P0/P1 tests from the CEO review
8. Add at least one full integration test exercising the complete request lifecycle with mock RPC
