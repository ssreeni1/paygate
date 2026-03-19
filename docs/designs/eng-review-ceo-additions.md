# Engineering Review: CEO Review Additions

**Date:** 2026-03-18
**Reviewer:** eng-review (targeted)
**Scope:** New items from `spec-amendments.md` NOT covered by `ENG-REVIEW.md`
**Status:** All decisions resolved

---

## 1. Scope Expansions — Engineering Analysis

### SE-1: `paygate demo` command

**Architecture fit:** Near-identical to the existing `paygate test` (ENG-REVIEW T24). Both start an echo server, fund from faucet, run a payment cycle, print results.

**Concern:** This duplicates `paygate test`. The only distinction is UX framing ("demo" for prospects, "test" for developers). Recommend implementing `paygate demo` as an alias or thin wrapper over `paygate test --verbose` rather than a separate code path.

**Resolution:** Implement as `paygate test --demo` flag that adds prettier output (timing breakdown, colored pass/fail). `paygate demo` is a clap alias. One code path, two entry points.

**Error handling:** Same as `paygate test` — faucet failure, RPC unreachable, timeout. Already covered by error-rescue-registry.md (reqwest::Timeout, RpcError).

**Security:** Requires `PAYGATE_TEST_KEY`. No new attack surface — testnet only.

**Test cases:**
| # | Test | Type |
|---|------|------|
| SE1-T1 | `paygate demo` runs end-to-end on testnet (alias for test --demo) | E2E |
| SE1-T2 | `paygate demo` without `PAYGATE_TEST_KEY` prints helpful error | Unit |

---

### SE-2: `GET /paygate/receipts/{tx_hash}` endpoint

**Architecture fit:** Clean read-only endpoint. Queries `payments` table by `tx_hash`. Runs on the main listener port (not admin), which is correct since this is consumer-facing.

**Concern 1 — Information disclosure:** The threat model (item 19) correctly notes tx data is already on-chain. However, the `endpoint` field reveals which API path was called, which is NOT on-chain. This leaks usage patterns.

**Resolution:** Return only on-chain-derivable fields: `{ tx_hash, payer_address, amount, token_address, verified_at, status }`. Omit `endpoint` and `request_hash` from the public response. If the provider wants full details, they use the admin port or `paygate revenue`.

**Concern 2 — tx_hash input validation:** Must validate format before SQLite query. A `tx_hash` is `0x` + 64 hex chars. Already specified in security-threat-model.md input validation table.

**Concern 3 — Rate limiting:** Spec says 100 req/min per IP. This endpoint runs on the main port, which already has the rate limiter middleware. Need to ensure the receipt endpoint has its own rate limit bucket separate from the payment rate limiter (which is per-payer, not per-IP).

**Resolution:** Add a dedicated `governor` rate limiter for `/paygate/receipts/*` keyed by IP, separate from the payment rate limiter. 100 req/min.

**Performance:** Single indexed lookup on `tx_hash` (UNIQUE index). Sub-millisecond. No concern.

**Test cases:**
| # | Test | Type |
|---|------|------|
| SE2-T1 | Valid tx_hash returns receipt JSON with correct fields | Integration |
| SE2-T2 | Unknown tx_hash returns 404 | Integration |
| SE2-T3 | Malformed tx_hash (wrong length, non-hex) returns 400 | Unit |
| SE2-T4 | Rate limit (101st request in 60s) returns 429 | Integration |
| SE2-T5 | Response does NOT include `endpoint` or `request_hash` | Unit |

---

### SE-3: `paygate pricing --html` generator

**Architecture fit:** Offline CLI command, no runtime impact. Reads `paygate.toml`, outputs static HTML to stdout or file.

**Concern 1 — XSS in generated HTML:** Endpoint paths from config are interpolated into HTML. If config contains `"POST /v1/<script>alert(1)</script>"`, the generated page is vulnerable.

**Resolution:** HTML-escape all config values (endpoint names, provider name, description) before interpolation. Use a minimal template engine or manual escaping (`<`, `>`, `&`, `"`, `'`).

**Concern 2 — Maintenance burden:** A custom HTML template embedded in a Rust binary is annoying to maintain.

**Resolution:** Keep it minimal — a single `const TEMPLATE: &str` with `{}` placeholders. No external template files. Under 100 lines of HTML. This is a developer tool, not a production website.

**Performance:** N/A — offline command.

**Test cases:**
| # | Test | Type |
|---|------|------|
| SE3-T1 | Generated HTML contains all endpoints and prices from config | Unit |
| SE3-T2 | Free endpoints (price=0) display as "Free" | Unit |
| SE3-T3 | HTML-escapes endpoint names (XSS prevention) | Unit |
| SE3-T4 | Works with minimal config (only default_price, no endpoints) | Unit |

---

### SE-4: Webhook on payment (fire-and-forget POST)

This is the most complex addition. Detailed review in Section 2 below.

**Architecture fit:** Fires from the Receipt Injector layer (after upstream response, before sending response to consumer). Must be non-blocking — spawns a tokio task, does not await completion in the request path.

**Concern — spawn location:** The webhook must fire after payment is verified AND after the upstream response is received (so we know the request succeeded). The correct insertion point is inside the Response Logger / Receipt Injector, NOT in the Payment Verifier.

**Resolution:** After response logging completes, `tokio::spawn` the webhook delivery. The spawned task has its own timeout and error handling. The response to the consumer is sent without waiting.

**Test cases:** See Section 2.

---

### SE-5: `X-Payment-Cost` response header

**Architecture fit:** Trivial addition to the Receipt Injector layer. The cost is already known (it is the price that was verified or debited).

**Concern 1 — Format:** Spec says "decimal string" (`0.001000`). Must be consistent: always 6 decimal places for USDC (matching the token's decimals). Do NOT use floating point — format from integer base units: `amount_base_units / 10^decimals`.

**Resolution:** Format as `format!("{}.{:0>width$}", whole, frac, width = decimals)` from integer base units. Never use `f64`.

**Concern 2 — Free endpoints:** For price=0 endpoints that skip payment, should we still add the header?

**Resolution:** Yes — set `X-Payment-Cost: 0.000000`. This lets clients distinguish "free endpoint" from "header missing for unknown reason."

**Performance:** String formatting. Negligible.

**Test cases:**
| # | Test | Type |
|---|------|------|
| SE5-T1 | Paid request has `X-Payment-Cost` with correct decimal value | Integration |
| SE5-T2 | Free endpoint has `X-Payment-Cost: 0.000000` | Integration |
| SE5-T3 | Formatting from base units (1000 with 6 decimals = "0.001000") | Unit |
| SE5-T4 | No floating point precision errors (test edge cases like 999999 base units) | Unit |

---

### SE-6: `paygate wallet` command

**Architecture fit:** CLI command that makes two queries: (1) RPC call for on-chain token balance, (2) SQLite query for 24h revenue.

**Concern 1 — RPC dependency:** This reuses the same RPC client config as the gateway. If `paygate wallet` is run while the gateway is NOT running, it needs to initialize its own RPC client. This means the config-loading and RPC-client-creation code must be factored out of the server startup path.

**Resolution:** Extract `create_rpc_client(config: &Config) -> reqwest::Client` and `load_config(path: &Path) -> Config` as standalone functions in `paygate-gateway`. CLI commands call these directly without starting the server.

**Concern 2 — Token balance query:** Requires calling `balanceOf(provider_address)` on the TIP-20 token contract. This means we need the token contract ABI (at minimum the `balanceOf` function). Use `alloy-sol-types` to define just the `balanceOf` call.

**Concern 3 — Revenue query:** `SELECT SUM(amount) FROM payments WHERE verified_at > ? AND token_address = ?` for 24h. If the gateway isn't running, the SQLite DB may not exist yet.

**Resolution:** Handle missing DB gracefully: print "No payment history found (database not initialized)."

**Performance:** One RPC call + one SQLite query. Fine.

**Test cases:**
| # | Test | Type |
|---|------|------|
| SE6-T1 | Displays balance and 24h revenue with correct formatting | Integration |
| SE6-T2 | Handles missing/empty database gracefully | Unit |
| SE6-T3 | Handles RPC unreachable with clear error message | Unit |
| SE6-T4 | Formats token amounts correctly (base units to decimal) | Unit |

---

## 2. Webhook System — Detailed Review

### 2.1 Fire-and-forget pattern

**Timeout:** 5 seconds (spec'd). Correct — this is long enough for a well-behaved receiver, short enough to not accumulate spawned tasks. At 100 payments/sec with 5s timeouts, worst case is 500 concurrent webhook tasks. Acceptable.

**Retry policy:** None (fire-and-forget). This is the right call for v0.1. A retry system adds a queue, persistence, exponential backoff — significant complexity. Webhook consumers who need reliability should acknowledge and enqueue on their end.

**Resolution:** No retries in v0.1. Log failures with full context (URL, status code, latency). The `paygate_webhook_delivery_total` metric with `status=failure|timeout` gives operators visibility. Document that webhooks are best-effort.

**Connection pooling:** The webhook sender MUST share a single `reqwest::Client` (with connection pooling) across all webhook deliveries. Do NOT create a new client per delivery.

**Resolution:** Create one `reqwest::Client` at startup (if webhook URL is configured), store in app state. All spawned webhook tasks reference it via `Arc`.

### 2.2 SSRF validation

**Config-time validation (spec'd):** Reject private IPs, localhost, link-local. This is necessary but NOT sufficient.

**Concern — DNS rebinding:** An attacker configures `webhook_url = "https://evil.com/hook"` where `evil.com` initially resolves to a public IP (passes config validation) but later resolves to `127.0.0.1` (hits internal services).

**Resolution:** DNS rebinding is a lower risk here because the config is set by the gateway OPERATOR (the API provider), not by untrusted callers. The operator is attacking their own infrastructure. However, for defense-in-depth:
1. Validate IP at config load (already spec'd).
2. Use `reqwest`'s `resolve` or a custom DNS resolver that re-checks the resolved IP before connecting. The `trust-dns-resolver` crate can do this. OR: accept the risk for v0.1 since the operator controls the URL. Add a note: "Webhook URL is operator-configured. DNS rebinding is an accepted risk for v0.1."

**Decision:** Accept DNS rebinding risk for v0.1. The operator is a trusted party configuring their own webhook. Add a doc note. Revisit if webhooks become user-configurable (they should not in v0.1).

**Scheme validation:** HTTPS only in production. Allow HTTP for `localhost` in development (useful for testing). Validate at config load.

### 2.3 Payload format and size

**Payload (spec'd):** `{ event, tx_hash, payer_address, amount, endpoint, timestamp }`.

**Concern — Size:** This is a fixed-structure JSON payload, roughly 300-400 bytes. No risk of large payloads. No user-controlled content that could inflate size (endpoint is from config, not from the request).

**Concern — Content-Type:** Must set `Content-Type: application/json`.

**Concern — Authentication:** No HMAC signature on the webhook payload. The receiver cannot verify the webhook came from PayGate vs. a spoofed POST.

**Resolution:** For v0.1, skip webhook signing. The receiver can verify the `tx_hash` on-chain or via the receipt endpoint if they need authenticity. Add webhook signing (HMAC with a shared secret) as a v0.2 item. Document this limitation.

### 2.4 Load behavior

At high throughput (e.g., 1,000 payments/sec), the system spawns 1,000 tokio tasks/sec, each holding a connection for up to 5 seconds. Worst case: 5,000 concurrent tasks, 5,000 connections to the webhook receiver.

**Concern:** This could overwhelm the webhook receiver or exhaust local file descriptors.

**Resolution:** Add a `tokio::sync::Semaphore` to cap concurrent webhook deliveries. Default: 50 concurrent. When the semaphore is full, new webhook deliveries are dropped (logged as `status=dropped`). This protects both PayGate and the receiver.

```toml
[webhooks]
max_concurrent = 50    # max in-flight webhook deliveries
```

**Test cases (all webhook):**
| # | Test | Type |
|---|------|------|
| WH-T1 | Webhook fires on verified payment with correct payload | Integration |
| WH-T2 | Webhook timeout (>5s) does not block response to consumer | Integration |
| WH-T3 | Webhook failure logged, consumer response unaffected | Integration |
| WH-T4 | Private IP in webhook URL rejected at config load | Unit |
| WH-T5 | HTTP scheme rejected (non-localhost) at config load | Unit |
| WH-T6 | Semaphore limits concurrent deliveries; excess are dropped | Unit |
| WH-T7 | `paygate_webhook_delivery_total` metric incremented correctly | Integration |
| WH-T8 | Empty/unset webhook URL means no webhook task spawned | Unit |

---

## 3. Config Hot-Reload (ArcSwap)

### 3.1 What can change at runtime?

| Field | Hot-reloadable | Rationale |
|-------|:---:|---|
| `pricing.*` | YES | Core use case — adjust prices without downtime |
| `rate_limiting.*` | YES | Tune limits under load |
| `webhooks.*` | YES | Change webhook URL, but re-validate SSRF on swap |
| `sponsorship.*` | YES | Toggle sponsorship, adjust budgets |
| `rpc_urls` | MAYBE | Changing RPC endpoints is useful for failover, but requires rebuilding the HTTP client pool. Allow it but log a warning. |
| `gateway.upstream` | NO | Changing upstream mid-flight risks routing requests to wrong backend. Require restart. |
| `gateway.listen` / `admin_listen` | NO | Cannot rebind TCP listeners at runtime. Require restart. |
| `tempo.chain_id` | NO | Changing chain is nonsensical at runtime. |
| `provider.address` | NO | Changing payment recipient mid-flight would cause payment verification failures for in-flight quotes. |
| `tempo.accepted_token` | NO | Same reasoning as provider address. |
| `security.*` | NO | Changing security parameters at runtime is dangerous. |

### 3.2 Signal handling

**SIGHUP:** Standard Unix convention for config reload. Use `tokio::signal::unix::signal(SignalKind::hangup())` in a dedicated task.

**Concern — macOS:** `SIGHUP` works on macOS. No issue.

**Concern — File watching:** Spec mentions "file change" as an alternative trigger. Do NOT implement inotify/kqueue file watching for v0.1 — it adds a dependency and complexity. SIGHUP only.

**Resolution:** SIGHUP only. File watching deferred.

### 3.3 In-flight requests during swap

**Concern:** A request starts with price A, swap happens mid-request, response logger sees price B. Inconsistency.

**Resolution:** `ArcSwap::load()` returns an `Arc<Config>` snapshot. Each request grabs a snapshot at the start of its middleware chain and uses that snapshot for the entire request lifecycle. The swap is atomic — old config is valid until the last `Arc` reference is dropped. This is the standard `arc_swap` pattern and handles this correctly by design.

**Implementation note:** The config reference must be captured in a tower layer or axum extension at request start, NOT re-loaded in each middleware layer.

### 3.4 Validation before swap

**Concern:** A typo in `paygate.toml` after SIGHUP could break the gateway.

**Resolution (spec'd):** Validate new config fully before swapping. On validation failure, log the error with the specific field, keep old config, increment `paygate_config_reloads_total{status=failure}`.

**Validation checklist (same as startup validation):**
- Addresses: valid hex format
- Prices: non-negative, parseable
- URLs: valid format
- Webhook URL: SSRF check (re-run on reload)
- No required fields missing from reloadable sections

**Test cases:**
| # | Test | Type |
|---|------|------|
| CR-T1 | SIGHUP with valid new config swaps pricing | Integration |
| CR-T2 | SIGHUP with invalid config keeps old config, logs error | Integration |
| CR-T3 | In-flight request uses config snapshot from request start | Integration |
| CR-T4 | Non-reloadable field change logged as warning, ignored | Unit |
| CR-T5 | `paygate_config_reloads_total` incremented on success and failure | Integration |

---

## 4. SQLite Writer Backpressure

### 4.1 Channel capacity

**Proposed: 10,000.** Each message is a small enum (payment record, log entry, quote) — roughly 200-500 bytes. 10,000 messages = ~2-5 MB memory ceiling. Acceptable.

**Concern — Is 10,000 too large or too small?**
- Too large: 10,000 buffered writes at 50 writes/batch (from ENG-REVIEW Issue 13) = 200 batches = ~2 seconds of buffer at full throughput. This is reasonable — it absorbs SQLite hiccups (WAL checkpoint, fsync) without dropping requests.
- Too small: At 5,000 payments/sec, 10,000 messages = 2 seconds of buffer. A WAL checkpoint can pause writes for 50-100ms. 2 seconds is plenty. Not too small.

**Resolution:** 10,000 is correct. Make it configurable:
```toml
[storage]
writer_channel_capacity = 10000
```

### 4.2 What happens when full?

**Proposed: 503.** Correct. When `mpsc::send()` fails (channel full), the payment verification layer returns 503 + `Retry-After: 1`. The payment is NOT consumed — the consumer can retry with the same `X-Payment-Tx`.

**Concern — Which writes hit the channel?** Not all writes are equal:
- **Payment record INSERT** (replay protection): CRITICAL. If this write is dropped, the same tx_hash could be accepted twice.
- **Request log INSERT**: Non-critical. Could be dropped without correctness impact.
- **Quote INSERT**: Moderate. A lost quote means the 402 response has a quote_id that doesn't exist in DB.

**Resolution:** All writes go through the bounded channel. If the channel is full, reject the entire request with 503. Do NOT selectively drop writes — that creates subtle correctness bugs (e.g., payment accepted but replay protection not recorded). The 503 is the correct backpressure signal.

### 4.3 Monitoring

**`paygate_db_writer_queue_depth` (gauge):** Requires periodic sampling. The `tokio::sync::mpsc` channel does not expose length directly, but `tokio::sync::mpsc::Sender::capacity()` returns remaining capacity. Queue depth = `total_capacity - remaining_capacity`.

**Resolution:** Sample queue depth every time a message is sent (cheap: one subtraction). Expose via gauge. Alert threshold: >8,000 (80% full) warrants investigation.

### 4.4 Recovery behavior

When the channel drains back below capacity, the system automatically recovers — new `send()` calls succeed, requests stop getting 503. No manual intervention needed.

**Concern — Thundering herd after recovery:** If 10,000 requests backed up and all retry simultaneously, the channel fills again instantly.

**Resolution:** The `Retry-After: 1` header provides natural jitter (clients retry at different times). Combined with rate limiting (100 req/s global), this is sufficient. No additional circuit-breaker needed.

**Test cases:**
| # | Test | Type |
|---|------|------|
| BP-T1 | Channel full returns 503 + Retry-After | Unit |
| BP-T2 | Payment tx_hash NOT consumed when 503 returned (retry works) | Integration |
| BP-T3 | Queue depth gauge reports correct value | Unit |
| BP-T4 | Recovery after channel drains (requests succeed again) | Integration |

---

## 5. Request Log Retention

### 5.1 DELETE performance on large tables

**Proposed:** `DELETE FROM request_log WHERE created_at < strftime('%s', 'now', '-30 days')` hourly.

**Concern:** After 30 days at 1,000 req/sec, `request_log` has ~2.6 billion rows. A single `DELETE` touching millions of rows will:
1. Hold the write lock for seconds to minutes.
2. Generate massive WAL growth (all deleted rows written to WAL before checkpoint).
3. Block the writer task (and therefore all payment verification) during the delete.

This is a **real performance problem**, not theoretical.

**Resolution:** Batch deletes in chunks:
```sql
DELETE FROM request_log WHERE rowid IN (
  SELECT rowid FROM request_log WHERE created_at < ? LIMIT 5000
);
```
Run in a loop with `tokio::time::sleep(100ms)` between batches. This keeps each transaction small (~5,000 rows), limits WAL growth, and yields the write lock between batches so payment writes can interleave.

### 5.2 ROWID range vs timestamp

**Concern:** The `created_at` index scan on a huge table is slower than a ROWID range scan. Since `request_log` uses `INTEGER PRIMARY KEY AUTOINCREMENT`, ROWIDs are monotonically increasing and loosely correlated with time.

**Resolution:** Use a hybrid approach:
1. Find the approximate ROWID boundary: `SELECT rowid FROM request_log WHERE created_at < ? ORDER BY rowid DESC LIMIT 1`
2. Delete by ROWID range: `DELETE FROM request_log WHERE rowid <= ? LIMIT 5000`

This is faster because ROWID lookups are B-tree primary key lookups (O(log n)), not secondary index scans. However, the complexity increase is marginal for v0.1.

**Decision:** Use the simple batched `DELETE ... WHERE created_at < ? LIMIT 5000` for v0.1. The `idx_request_log_created` index makes this efficient enough. Optimize to ROWID-based if monitoring shows cleanup taking >10 seconds total.

### 5.3 Vacuum after large deletes

**Concern:** SQLite does not reclaim disk space after DELETE. The DB file remains large. `VACUUM` rebuilds the entire database, which locks it exclusively for the duration.

**Resolution:** Do NOT run `VACUUM` automatically. SQLite reuses freed pages for new inserts, so disk space is reclaimed naturally over time. If the operator needs to reclaim space, they can run `VACUUM` manually during a maintenance window. Document this in operational docs.

**Alternative:** `PRAGMA auto_vacuum = INCREMENTAL` can be set at DB creation time, and `PRAGMA incremental_vacuum(N)` can reclaim N pages without a full rebuild. Consider this for v0.2.

**Test cases:**
| # | Test | Type |
|---|------|------|
| RL-T1 | Retention cleanup deletes rows older than configured days | Integration |
| RL-T2 | Cleanup runs in batches (no single large transaction) | Unit |
| RL-T3 | Cleanup does not block concurrent payment writes | Integration |
| RL-T4 | Zero rows to delete is a no-op (no error) | Unit |

---

## 6. New Test Cases Summary

Tests from this review that extend beyond the 29 in ENG-REVIEW.md:

| # | Area | Test | Type |
|---|------|------|------|
| SE1-T1 | demo command | End-to-end testnet demo | E2E |
| SE1-T2 | demo command | Missing test key error | Unit |
| SE2-T1 | receipts endpoint | Valid tx returns receipt | Integration |
| SE2-T2 | receipts endpoint | Unknown tx returns 404 | Integration |
| SE2-T3 | receipts endpoint | Malformed hash returns 400 | Unit |
| SE2-T4 | receipts endpoint | Rate limit at 100/min | Integration |
| SE2-T5 | receipts endpoint | No endpoint leak in response | Unit |
| SE3-T1 | pricing HTML | Correct endpoints and prices | Unit |
| SE3-T2 | pricing HTML | Free endpoints display correctly | Unit |
| SE3-T3 | pricing HTML | XSS prevention | Unit |
| SE3-T4 | pricing HTML | Minimal config works | Unit |
| SE5-T1 | cost header | Correct decimal value | Integration |
| SE5-T2 | cost header | Free endpoint shows 0.000000 | Integration |
| SE5-T3 | cost header | Base units formatting | Unit |
| SE5-T4 | cost header | No float precision errors | Unit |
| SE6-T1 | wallet command | Balance and revenue display | Integration |
| SE6-T2 | wallet command | Missing DB handled | Unit |
| SE6-T3 | wallet command | RPC unreachable error | Unit |
| SE6-T4 | wallet command | Amount formatting | Unit |
| WH-T1 | webhook | Correct payload on payment | Integration |
| WH-T2 | webhook | Timeout doesn't block response | Integration |
| WH-T3 | webhook | Failure logged, response clean | Integration |
| WH-T4 | webhook | Private IP rejected at config | Unit |
| WH-T5 | webhook | HTTP rejected (non-localhost) | Unit |
| WH-T6 | webhook | Semaphore limits concurrency | Unit |
| WH-T7 | webhook | Metric incremented | Integration |
| WH-T8 | webhook | No URL = no task spawned | Unit |
| CR-T1 | config reload | Valid reload swaps pricing | Integration |
| CR-T2 | config reload | Invalid reload keeps old | Integration |
| CR-T3 | config reload | In-flight uses snapshot | Integration |
| CR-T4 | config reload | Non-reloadable warned | Unit |
| CR-T5 | config reload | Metrics incremented | Integration |
| BP-T1 | backpressure | Channel full returns 503 | Unit |
| BP-T2 | backpressure | tx_hash not consumed on 503 | Integration |
| BP-T3 | backpressure | Queue depth gauge correct | Unit |
| BP-T4 | backpressure | Recovery after drain | Integration |
| RL-T1 | retention | Deletes old rows | Integration |
| RL-T2 | retention | Batched deletes | Unit |
| RL-T3 | retention | No write blocking | Integration |
| RL-T4 | retention | Zero rows is no-op | Unit |

**Total new test cases: 40** (on top of ENG-REVIEW's 29, grand total: 69).

---

## 7. Additional Metrics Review

### `paygate_db_errors_total` (counter)

**Verdict: KEEP.** Essential for operational visibility. Label with `operation` (insert_payment, insert_quote, insert_log, delete_retention) to pinpoint which writes are failing.

**Concern:** Do not label with the full SQL query — that creates high cardinality. Use a short operation name.

### `paygate_db_writer_queue_depth` (gauge)

**Verdict: KEEP.** Critical for backpressure monitoring. This is the earliest signal that the system is approaching overload.

**Concern:** Gauge must be updated on every send, not sampled on a timer. Timer-based sampling misses spikes.

**Resolution:** Update on every `mpsc::send()` and every `mpsc::recv()`. Two atomic operations per message — negligible cost.

### `paygate_webhook_delivery_total` (counter, labels: status)

**Verdict: KEEP.** Labels: `status=success|failure|timeout|dropped`. The `dropped` label covers the semaphore-based overflow from Section 2.4.

**Concern:** Do not include the webhook URL as a label — it is a single configured URL, and including it wastes cardinality budget for no benefit.

### `paygate_quotes_active` (gauge)

**Verdict: KEEP, but rename to `paygate_quotes_unexpired`.** "Active" is ambiguous (could mean "in use by an in-flight request"). "Unexpired" is precise.

**Concern — Measurement cost:** Counting unexpired quotes requires `SELECT COUNT(*) FROM quotes WHERE expires_at > ?`. This is a full index scan on `idx_quotes_expires`. At high throughput (thousands of quotes), this is expensive to run on every request.

**Resolution:** Sample periodically (every 30 seconds) in the cleanup task, not on every request. The gauge value may be up to 30 seconds stale — acceptable for monitoring.

### `paygate_config_reloads_total` (counter, labels: status)

**Verdict: KEEP.** Labels: `status=success|failure`. Low-frequency metric (only fires on SIGHUP). Useful for operational dashboards and alerting on repeated reload failures.

**No concerns.**

---

## Spec Amendments Required (Summary)

1. **`paygate demo`:** Implement as alias for `paygate test --demo`, not a separate command
2. **Receipts endpoint:** Omit `endpoint` and `request_hash` from public response; add dedicated IP-based rate limiter
3. **Pricing HTML:** HTML-escape all interpolated config values
4. **Webhook concurrency:** Add `tokio::sync::Semaphore` (default 50) to cap concurrent deliveries; add `max_concurrent` config
5. **Webhook signing:** Document as v0.2 item; v0.1 webhooks are unsigned
6. **Webhook DNS rebinding:** Accept risk for v0.1 (operator-controlled URL); document
7. **Config reload:** SIGHUP only (no file watching in v0.1); define reloadable vs non-reloadable fields per table in Section 3.1
8. **Config snapshot:** Capture `Arc<Config>` at request start, use throughout request lifecycle
9. **Writer backpressure 503:** When channel full, do NOT consume tx_hash — consumer can retry
10. **Request log retention:** Batch deletes in chunks of 5,000 with yielding; no auto-VACUUM
11. **`paygate_quotes_active`:** Rename to `paygate_quotes_unexpired`; sample every 30s, not per-request
12. **`X-Payment-Cost`:** Format from integer base units, never float; include on free endpoints as `0.000000`
13. **`paygate wallet`:** Factor out config loading and RPC client creation for standalone CLI use; handle missing DB

---

## Completion Summary

- **Scope expansions reviewed:** 6 items, all with architecture fit, error handling, and test cases
- **Webhook system:** 8 test cases, semaphore concurrency limit, DNS rebinding risk accepted for v0.1
- **Config hot-reload:** Reloadable field matrix defined, ArcSwap snapshot pattern specified, 5 test cases
- **SQLite backpressure:** Channel capacity validated (10,000), 503 behavior correct, 4 test cases
- **Request log retention:** Batched delete strategy, no auto-VACUUM, 4 test cases
- **New test cases:** 40 (total with ENG-REVIEW: 69)
- **Metrics:** All 5 kept, 1 renamed, measurement strategies specified
- **Unresolved decisions:** 0
