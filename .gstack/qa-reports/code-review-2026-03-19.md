# PayGate Pre-Landing Code Review

**Date:** 2026-03-19
**Reviewer:** /review (Claude)
**Scope:** Full codebase — all 4 merged branches (verifier, cli, ts-sdk, contracts)
**Files reviewed:** 25 source files (~5,500 lines), 6 test files, schema, config, fixtures
**Verdict:** **SHIP WITH 2 FIXES** (1 high, 1 medium)

---

## Executive Summary

The codebase is solid. All SQL uses parameterized queries. Constant-time comparison is correctly implemented for memo verification. Header sanitization works. Error handling is comprehensive and matches the error-rescue-registry. The 4 worktree branches integrated cleanly with no conflicting patterns.

Two issues should be fixed before v0.1.0 ship. The rest are observations for future work.

---

## MUST FIX (before v0.1.0)

### H1: Fragile replay detection via string matching — `verifier.rs:381`

```rust
if e.to_string().contains("UNIQUE") {
    return VerificationResult::ReplayDetected;
}
```

**Risk:** This detects concurrent replay by matching the SQLite error message string. If rusqlite changes error formatting, or the message is localized, this silently becomes a 503 "database write error" instead of a 409 "replay detected." An attacker could potentially exploit the timing window.

**Fix:** Match on `rusqlite::Error::SqliteFailure` with error code `SQLITE_CONSTRAINT_UNIQUE` (extended code 2067):

```rust
if let DbError::Sqlite(rusqlite::Error::SqliteFailure(err, _)) = &e {
    if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE {
        return VerificationResult::ReplayDetected;
    }
}
```

This requires exposing the underlying `rusqlite::Error` variant from `DbError::Sqlite`, which it already does.

### H2: Receipt endpoint leaks internal DB errors — `admin.rs:113`

```rust
Err(e) => (
    StatusCode::INTERNAL_SERVER_ERROR,
    Json(json!({"error": format!("database error: {e}")})),
)
```

**Risk:** The raw SQLite error message is returned to the client. This could leak internal file paths, table names, or schema details. Per the error-rescue-registry, database errors should return a generic "Service unavailable" message.

**Fix:** Log the error internally, return a generic message:

```rust
Err(e) => {
    tracing::error!(tx_hash = %tx_hash, error = %e, "receipt lookup failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": "internal error"})),
    )
}
```

---

## SHOULD FIX (non-blocking, address post-ship)

### M1: TS SDK `parse402Response` doesn't validate `quote_id` — `sdk/src/discovery.ts:14-35`

The parser validates `pricing`, `recipient`, `amount_base_units`, and `token`, but not `quote_id` or `quote_expires_at`. The client then passes `pricing.quote_id` directly to `paymentMemo()` at `client.ts:36`. If the server sends a malformed 402 without `quote_id`, the memo computation uses `undefined` which would produce an incorrect hash, and the payment would be wasted.

**Fix:** Add validation: `if (!body.pricing.quote_id) throw new Error('402 response missing pricing.quote_id');`

### M2: `cmd_test` / `cmd_demo` payment verification is a TODO — `main.rs:1239`

```rust
// TODO: Implement actual on-chain payment steps when testnet keys are available
```

The `paygate test` and `paygate demo` commands currently skip the actual on-chain payment and verification steps. This means the most critical user-facing verification path is untested end-to-end via CLI. The test just prints "SKIP" for steps 2-6.

**Impact:** Low for v0.1.0 (the gateway verifier has unit tests), but `paygate test` is the primary user-facing verification tool. Should be implemented before any production deployment.

### M3: `verifier.rs:148` — `.unwrap()` on confirmed-length vec

```rust
1 => Ok(matches.into_iter().next().unwrap()),
```

This is technically safe (the match arm guarantees len==1), but `.unwrap()` in production payment verification code is a code smell. Consider:

```rust
1 => Ok(matches.into_iter().next().expect("len checked")),
```

Or use `matches.pop().unwrap()` which is idiomatic for consuming the only element.

---

## 1. SQL Safety — PASS

All 14 SQL queries in `db.rs` use `rusqlite::params![]` macro with `?` placeholders. No string interpolation in any query. The schema in `schema.sql` uses `CREATE TABLE IF NOT EXISTS` for idempotent initialization.

Spot-checked:
- `is_tx_consumed` — parameterized
- `get_quote` — parameterized with time comparison
- `insert_payment` — parameterized (11 fields)
- `cleanup_task` DELETE queries — parameterized

**Verdict:** No SQL injection vectors found.

---

## 2. Trust Boundaries — PASS

Payment data flow is correct:

1. **Untrusted → Trusted:** `X-Payment-Tx` header → `verify_payment()` → RPC verification → DB insert. The tx_hash is never trusted until verified on-chain.

2. **Header Sanitization:** `proxy.rs:38-47` strips all `X-Payment-*` headers before forwarding using `mpp::is_payment_header()`. Upstream never sees payment headers. Verified by integration test `test_header_sanitization`.

3. **Payer Binding:** Verified at `verifier.rs:331-343`. The `X-Payment-Payer` header is compared against the on-chain `from` address after parsing both to `Address` type (case-insensitive byte comparison).

4. **Receipt Headers:** `X-Payment-Receipt` and `X-Payment-Cost` are only added to responses AFTER successful verification and upstream forwarding (`proxy.rs:107-115`).

5. **Free Endpoint Protection:** Price-0 endpoints correctly skip to proxy without payment check (`main.rs:359-376`). Non-free endpoints NEVER reach proxy without verification.

---

## 3. Concurrency — PASS

- **SQLite Writer:** Single-writer pattern via bounded mpsc channel (`db.rs`, capacity 10,000). Batches writes in transactions (flush every 10ms or 50 writes). Backpressure returns 503 when channel is full. No concurrent writes possible.

- **Rate Limiter:** Uses `governor` crate which is internally thread-safe (atomic operations + DashMap for keyed limiters). Three independent limiters: global, per-payer, per-IP-402.

- **Config Reload:** Uses `Arc<ArcSwap<Config>>` for lock-free reads. SIGHUP handler stores new config atomically. In-flight requests see either old or new config (never partial).

- **Replay Race Condition:** Handled by the SQLite UNIQUE constraint on `tx_hash`. Two concurrent verifications of the same tx will produce one success and one UNIQUE violation → `ReplayDetected`. (Fix H1 above improves the detection mechanism.)

---

## 4. Error Handling — PASS (with H1, H2 noted above)

Cross-referenced every error in `error-rescue-registry.md` against the code:

| Error | Spec HTTP | Actual HTTP | Match? |
|-------|-----------|-------------|--------|
| TxNotFound | 400 + Retry-After:1 | 400 + Retry-After:1 | YES |
| RpcError | 503 + Retry-After:2 | 503 + Retry-After:2 | YES |
| InvalidTransfer | 400 | 400 | YES |
| AmbiguousTransfer | 400 | 400 | YES |
| InsufficientPayment | 402 + shortfall | 402 + shortfall | YES |
| PayerMismatch | 403 | 403 | YES |
| ReplayDetected | 409 | 409 | YES |
| ExpiredTransaction | 400 | 400 | YES |
| MemoMismatch | 400 | 400 | YES |
| Upstream timeout | 504 | 504 | YES |
| Connection error | 502 | 502 | YES |
| Backpressure | 503 + Retry-After | 503 + Retry-After:1 | YES |
| Rate limit | 429 | 429 | YES |

**Unwrap/expect audit (production code only):**

| Location | Call | Safe? | Reason |
|----------|------|-------|--------|
| `main.rs:142` | `.expect("failed to build HTTP client")` | YES | Startup — should abort |
| `main.rs:167` | `.expect("failed to install Prometheus")` | YES | Startup — should abort |
| `main.rs:269,292` | `.expect("signal handler")` | YES | Startup — OS failure |
| `rate_limit.rs:36,39,42` | `NonZeroU32::new(...).unwrap()` | YES | `.max(1)` ensures non-zero |
| `verifier.rs:148` | `.next().unwrap()` | YES* | Len==1 guaranteed by match arm |
| `mpp.rs:47` | `.unwrap_or_default()` | YES | Fallback provided |
| `verifier.rs:139` | `u64::try_from(...).unwrap_or(u64::MAX)` | YES | Capped fallback |

No panicking unwraps in production request paths.

---

## 5. Security — PASS

### Constant-Time Comparison
Correctly implemented at `verifier.rs:30-39` and used for memo verification at line 307. XOR accumulator pattern, length check before comparison. Sound implementation.

### Private Key Handling
Private key is read from environment variable specified by `private_key_env` config field (default: `PAYGATE_PRIVATE_KEY`). Never stored in config file, never logged.

### Webhook SSRF Protection
`config.rs:328-371` validates webhook URLs:
- Requires HTTPS
- Blocks 127.0.0.1, localhost, 10.x.x.x, 192.168.x.x, 169.254.x.x, 172.16-31.x.x
- Validated at config load time (not at runtime)

### Header Sanitization
All headers matching `x-payment-*` prefix (case-insensitive) are stripped in `proxy.rs:38-47` before upstream forwarding. Verified by test.

### Rate Limiting
Three levels: global (config RPS), per-payer (config per-payer RPS), 402 flood (1000/min per IP). Keyed by `X-Payment-Payer` header or IP fallback.

### Receipt Endpoint Input Validation
`admin.rs:89`: Validates tx_hash format (must be `0x`-prefixed, 66 chars). Prevents DB injection via path parameter.

### XSS in HTML Pricing
`main.rs` includes `html_escape()` function used in `print_pricing_html()` output. Escapes `<`, `>`, `&`, `"`, `'`.

---

## 6. Architectural Coherence — PASS

The 4 branches integrated cleanly:

| Branch | Module | Integration |
|--------|--------|-------------|
| verifier | verifier.rs, mpp.rs, proxy.rs, rate_limit.rs, webhook.rs | Core middleware; called by gateway_handler in main.rs |
| cli | main.rs (commands), admin.rs | Uses AppState, DbReader, Config |
| ts-sdk | sdk/src/*.ts | Independent; shares hash test vectors via fixtures |
| contracts | contracts/src/*.sol | Independent; no Rust integration needed for MVP |

**Type sharing:** `paygate-common` crate correctly centralizes `VerificationResult`, `PaymentProof`, `Quote`, `PaymentRecord`, `BaseUnits`, header constants, and hash functions. Both gateway modules and tests import from common.

**State sharing:** `AppState` (`server.rs`) holds all shared state. Clean separation between `DbReader` (concurrent reads) and `DbWriter` (serialized writes).

**No circular dependencies** between modules.

---

## 7. Correctness — PASS (with caveats)

### Spec Compliance Verified

- **SPEC §3.2 Middleware Stack:** Rate limit → MPP check → Verify → Proxy matches code
- **SPEC §4.1 402 Response:** All required headers present, JSON body matches spec format
- **SPEC §4.2 Verification Steps:** 10-step pipeline in verifier.rs matches spec ordering
- **SPEC §10 Security:** All threat mitigations implemented (replay, front-running, stale tx, wrong amount/recipient, ambiguous tx, header sanitization, webhook SSRF)
- **SPEC §11 Edge Cases:** RPC unreachable (503), null receipt (400+Retry-After), price change during quote (honored), response too large (502)
- **SPEC §12 Observability:** Health endpoint, Prometheus metrics, structured JSON logging, receipt verification endpoint

### Cross-Language Hash Parity
Verified: Rust `hash.rs` and TypeScript `hash.ts` both compute `keccak256(method + " " + path + "\n" + body)` with UTF-8 encoding. Shared test vectors in `tests/fixtures/request_hash_vectors.json` validate parity. Both test suites (Rust unit tests, TS `hash.test.ts`) load and verify against the same fixture.

### Solidity Contract
`PayGateRegistry.sol` is clean: no reentrancy risk (no external calls), proper `onlyProvider` access control, events for all state changes. Test coverage is thorough (17 test cases). No issues found.

---

## 8. Dead Code — LOW

The following dead code exists but is acceptable for MVP:

- **Config fields** for sessions, sponsorship, dynamic pricing, tiers — these are declared in the config struct for forward-compatibility but not used in v0.1.0. Removing them would break `paygate.toml` parsing for users who include these sections.

- **`PayGateEscrow.sol`** — Empty stub with TODO. Acceptable as deferred v0.2 work.

- **`DbWriter::queue_depth()`** — Returns 0 (unimplemented). Referenced by comment but no callers.

- **Several `metrics::` functions** (`set_active_sessions`, `set_writer_queue_depth`, `set_active_quotes`, `record_config_reload`) — Defined but not called from main request paths. Prometheus metrics are still registered, just never updated. Benign.

**Recommendation:** No cleanup required before ship. These are all forward-looking stubs that don't affect correctness.

---

## Test Coverage Summary

| Test Category | Count | Status |
|---------------|-------|--------|
| Config parsing | 6 | PASS |
| Hash vectors (Rust) | 6+ | PASS |
| Hash vectors (TS) | 5 | PASS |
| Payment verification | 11 | PASS |
| 402 response format | 1 | PASS |
| Header sanitization | 1 | PASS |
| Rate limiting | 2 | PASS |
| Free endpoint bypass | 1 | PASS |
| TS SDK client | 5 | PASS |
| Solidity contract | 17 | PASS |
| **Total** | **55+** | **ALL PASS** |

---

## Final Verdict

**SHIP with 2 fixes:**

1. **H1:** Replace string-matching replay detection with `rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE` error code check
2. **H2:** Don't leak raw database errors in receipt endpoint responses

Everything else is solid. The security model is correctly implemented, SQL is safe, concurrency is sound, error handling matches the spec, and the 4 branches integrate cleanly. Ship it.
