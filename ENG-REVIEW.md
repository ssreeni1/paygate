# Engineering Review: PayGate SPEC.md

**Date:** 2026-03-18
**Reviewer:** /plan-eng-review (gstack)
**Branch:** N/A (greenfield — no git repo)
**Status:** All decisions resolved

---

## Step 0: Scope Challenge

**Complexity check: TRIGGERED** — 23+ files across 4 packages in MVP.

**Decision:** User chose to keep all 4 packages as spec'd (paygate-gateway, paygate-common, paygate-client, sdk/). Scope accepted as-is.

---

## Architecture Review — 6 Issues

### Issue 1: CRITICAL — Session HMAC vs stored secret hash (SPEC.md:503 vs :189)

**Problem:** Spec says store `secret_hash` (SHA-256 of session secret) but HMAC verification requires the plaintext secret. These are mutually exclusive.

**Resolution:** Store plaintext session secret in DB. Update line 503 from `secret_hash TEXT NOT NULL -- SHA-256 of session secret (never store plaintext)` to `secret TEXT NOT NULL -- server-issued, used for HMAC verification`.

### Issue 2: Tempo placeholder values

**Problem:** Chain ID, RPC URL, USDC address, MPP headers all unresolved.

**Resolution:** Build against Tempo testnet. Add `network` config field. Swap to mainnet when values confirmed. Tracked in TODOS.md.

### Issue 3: Request hash body binding

**Problem:** Raw body bytes in hash can break if intermediaries modify JSON whitespace/ordering.

**Resolution:** Keep raw-bytes approach (works for non-JSON too). Add spec note: "PayGate MUST receive the identical request body bytes that the client used to compute requestHash. Do not place body-modifying proxies between client and PayGate."

### Issue 4: Single RPC SPOF

**Problem:** Single `rpc_url` means RPC outage = complete revenue outage.

**Resolution:** Change to `rpc_urls` (array) with failover. Config:
```toml
[tempo]
rpc_urls = ["https://rpc.tempo.xyz", "https://rpc2.tempo.xyz"]
failover_timeout_ms = 2000
```

### Issue 5: Quote consumption race condition (obvious fix)

**Problem:** Quote consumed on reference, not on successful verification. If verification fails, quote is wasted.

**Resolution:** Consume quote only after successful payment verification.

### Issue 6: Throughput claim unsubstantiated (obvious fix)

**Problem:** "~5,000 verified payments/sec" claim ignores RPC bottleneck (~50ms per call).

**Resolution:** Revise claim to "SQLite write throughput: ~50,000/sec with batching. End-to-end verification throughput limited by RPC latency (~1,000/sec with 50 concurrent connections)."

---

## Code Quality Review — 4 Issues

### Issue 7: requestHash DRY across Rust + TypeScript

**Problem:** Same hash algorithm implemented in 3 places across 2 languages. Divergence = silent payment failures.

**Resolution:** Create `tests/fixtures/request_hash_vectors.json` with test vectors. Both Rust and TS test suites validate against it. Include memo computation vectors too.

### Issue 8: Memo format ambiguity (obvious fix)

**Problem:** "truncated to bytes32" is redundant — keccak256 already outputs bytes32. Input encoding unclear.

**Resolution:** Specify: "Inputs are UTF-8 encoded and concatenated as raw bytes before hashing. keccak256 output is bytes32 (no truncation needed)." Add memo test vectors to shared fixture.

### Issue 9: RPC timeout during verification

**Problem:** Payment exists on-chain but gateway can't verify due to RPC timeout. What happens?

**Resolution:** Document: "On RPC timeout, return 503 + `Retry-After: 2` header. Consumer may retry same request with same `X-Payment-Tx`. Verification is idempotent — tx_hash not consumed until verification succeeds."

### Issue 10: Free endpoint middleware skip (obvious fix)

**Problem:** Middleware stack diagram doesn't show where price=0 endpoints bypass payment.

**Resolution:** MPP Negotiator checks endpoint price. If price == 0, skip directly to Header Sanitizer → Reverse Proxy. Add to middleware stack diagram.

---

## Test Review — 29 Test Cases Required

### Test Diagram

```
                        ┌─────────────────────────────────────────────────────┐
                        │              PAYGATE REQUEST FLOW                    │
                        └─────────────────────────────────────────────────────┘

  Incoming Request
       │
       ▼
  ┌─────────────┐   over limit    ┌───────┐
  │ Rate Limiter │───────────────>│  429  │
  └──────┬──────┘                 └───────┘
         │ ok
         ▼
  ┌──────────────┐   price==0     ┌────────────────┐
  │MPP Negotiator│───────────────>│ Skip to Proxy  │──┐
  └──────┬───────┘                └────────────────┘  │
         │ price>0, no payment headers                │
         ├──────────────────────>┌───────┐            │
         │                      │  402  │            │
         │                      │+quote │            │
         │                      └───────┘            │
         │ has payment headers                        │
         ▼                                            │
  ┌──────────────────┐                                │
  │Payment Verifier  │                                │
  │                  │                                │
  │ ┌──────────────┐ │  tx not found / empty receipt  │
  │ │Fetch receipt │─┼──────────>┌───────┐           │
  │ └──────┬───────┘ │           │  402  │           │
  │        │ ok      │           └───────┘           │
  │ ┌──────────────┐ │  wrong token/amt/malformed    │
  │ │Decode events │─┼──────────>┌───────┐           │
  │ └──────┬───────┘ │           │  402  │           │
  │        │ ok      │           └───────┘           │
  │ ┌──────────────┐ │  memo mismatch                │
  │ │Verify memo   │─┼──────────>┌───────┐           │
  │ └──────┬───────┘ │           │  402  │           │
  │        │ ok      │           └───────┘           │
  │ ┌──────────────┐ │  replay                       │
  │ │Replay check  │─┼──────────>┌───────┐           │
  │ └──────┬───────┘ │           │  402  │           │
  │        │ ok      │           └───────┘           │
  │ ┌──────────────┐ │  payer mismatch               │
  │ │Payer binding │─┼──────────>┌───────┐           │
  │ └──────┬───────┘ │           │  402  │           │
  │        │ ok      │           └───────┘           │
  │ ┌──────────────┐ │  expired                      │
  │ │TX age check  │─┼──────────>┌───────┐           │
  │ └──────┬───────┘ │           │  402  │           │
  │        │ ok      │           └───────┘           │
  └────────┼─────────┘                                │
           │                                          │
           ▼                                          │
  ┌────────────────┐                                  │
  │Header Sanitize │<─────────────────────────────────┘
  │strip X-Payment │
  └───────┬────────┘
          │
          ▼
  ┌──────────────┐    5xx     ┌───────────────┐
  │ Reverse Proxy│───────────>│ 502 + receipt │
  └──────┬───────┘            └───────────────┘
         │ 2xx/3xx/4xx
         ▼
  ┌──────────────┐
  │Response Logger│
  │  (SQLite)    │
  └──────┬───────┘
         │
         ▼
  ┌────────────────┐
  │Receipt Injector│
  │X-Payment-Rcpt │
  └───────┬────────┘
          │
          ▼
     Response to Consumer
```

### Test Matrix (29 cases)

| # | Codepath | Test description | Type | Lang |
|---|----------|-----------------|------|------|
| T1 | Rate limiter | Rejects at threshold (429) | Unit | Rust |
| T2 | Free endpoint | price=0 skips payment, returns 200 | Integration | Rust |
| T3 | 402 generation | Correct headers, JSON body, quote stored | Unit | Rust |
| T4 | Quote honored | Quoted price accepted within TTL after price change | Integration | Rust |
| T5 | Quote expired | Expired quote falls back to current price | Integration | Rust |
| T6 | Receipt fetch | Mock RPC, decode TIP-20 Transfer event logs | Unit | Rust |
| T7 | Memo verify | keccak256("paygate" \|\| quoteId \|\| requestHash) matches | Unit | Rust+vectors |
| T8 | Replay protection | Same tx_hash rejected on second use | Integration | Rust |
| T9 | Payer binding | X-Payment-Payer mismatch → rejected | Unit | Rust |
| T10 | TX age check | Stale tx (> tx_expiry_seconds) rejected | Unit | Rust |
| T11 | Multiple events | TX with 2 matching Transfer events → rejected | Unit | Rust |
| T12 | Wrong amount | amount < price → 402 with shortfall | Unit | Rust |
| T13 | Wrong recipient | to != provider → rejected | Unit | Rust |
| T14 | Header sanitization | X-Payment-* stripped before upstream | Integration | Rust |
| T15 | Upstream 5xx | Returns 502 + receipt | Integration | Rust |
| T16 | Request hash | Matches shared test vectors | Unit | Rust+TS |
| T17 | Config parsing | Minimal, full, and invalid TOML configs | Unit | Rust |
| T18 | Health endpoint | Healthy + degraded (RPC down) states | Integration | Rust |
| T19 | Metrics endpoint | Prometheus counters increment correctly | Integration | Rust |
| T20 | Graceful shutdown | SIGTERM drains in-flight requests | Integration | Rust |
| T21 | RPC failover | Primary timeout → secondary succeeds | Unit | Rust |
| T22 | TS SDK auto-pay | 402 → pay → retry flow works transparently | Integration | TS |
| T23 | TS SDK requestHash | Matches shared test vectors | Unit | TS |
| T24 | `paygate test` e2e | Full testnet: faucet → pay → verify → response | E2E | Rust |
| T25 | SQLite concurrency | 100 concurrent inserts, no SQLITE_BUSY | Unit | Rust |
| T26 | Invalid RPC receipt | None/empty receipt → 402 "payment not found" | Unit | Rust |
| T27 | Malformed event logs | Decode failure → 402 (not panic) | Unit | Rust |
| T28 | SQLite write failure | Simulated disk full → 503 to pending verifications | Unit | Rust |
| T29 | Upstream response OOM | Response body > size limit → 502 | Integration | Rust |

---

## Performance Review — 3 Issues

### Issue 12: RPC connection pooling (obvious fix)

**Resolution:** Specify shared `reqwest::Client` with connection pooling for all RPC calls. Do not create per-request HTTP clients.

### Issue 13: SQLite batch writes

**Resolution:** Writer task batches INSERTs in a transaction. Flush every 10ms or 50 writes, whichever comes first.

### Issue 14: Quote TTL cleanup (obvious fix)

**Resolution:** Periodic cleanup task: `DELETE FROM quotes WHERE expires_at < now() - 3600` every 5 minutes. Run as part of writer task flush cycle.

---

## Failure Modes

| # | Codepath | Failure | Test | Error handling | User sees | Status |
|---|----------|---------|:---:|:---:|---|---|
| F1 | RPC call | Timeout | T21 | Yes (503) | Clear | OK |
| F2 | RPC call | Invalid/empty receipt | **T26** | **Add** | Was silent → now 402 | **FIXED** |
| F3 | Event decode | No matching Transfer | T13 | Yes (402) | Clear | OK |
| F4 | Event decode | Malformed log data | **T27** | **Add** | Was panic → now 402 | **FIXED** |
| F5 | SQLite write | Disk full | **T28** | **Add** | Was silent → now 503 | **FIXED** |
| F6 | Quote lookup | ID not found | T5 | Yes (current price) | Clear | OK |
| F7 | Upstream proxy | Connection refused | T15 | Yes (502) | Clear | OK |
| F8 | Upstream proxy | Response > memory | **T29** | **Add** | Was OOM → now 502 | **FIXED** |
| F9 | Config parse | Invalid TOML | T17 | Yes (startup error) | Clear | OK |
| F10 | Graceful shutdown | In-flight during drain | T20 | Yes (30s timeout) | Clear | OK |

**Critical gaps: 4 identified, all resolved** by adding T26-T29 and corresponding error handling to the spec.

---

## NOT in Scope

| Item | Rationale |
|------|-----------|
| Smart contracts (Registry, Escrow) | Wave 2+. MVP uses direct TIP-20 transfers. |
| Sessions (pay-as-you-go) | Wave 2. Well-defined but deferred. |
| Fee sponsorship | Wave 2. Requires Tempo fee payer protocol. |
| Dynamic pricing | Wave 2. Requires sessions/escrow. |
| Dashboard (React) | Wave 3. |
| Multi-instance / PostgreSQL | Wave 3. |
| SSE streaming payments | Wave 3. |
| Multi-chain support | Wave 4. |
| Formal MPP spec compliance | Blocked on Tempo. Tracked in TODOS.md. |
| Mainnet chain config | Deferred. Tracked in TODOS.md. |

## What Already Exists

| Sub-problem | Existing solution | Reused? |
|------------|------------------|:---:|
| Reverse proxy | axum + tower-http | Yes |
| TOML config | serde + toml crate | Yes |
| keccak256 | alloy-primitives / tiny-keccak | Yes |
| TIP-20 event decode | alloy-sol-types | Yes |
| SQLite WAL | rusqlite | Yes |
| Rate limiting | governor crate | Spec should specify |
| Prometheus metrics | metrics + metrics-exporter-prometheus | Yes |
| HTTP client for RPC | reqwest (connection pooled) | Yes |

---

## Spec Changes Required (Summary)

1. **Line 503:** Change `secret_hash` to `secret`, remove "never store plaintext" comment
2. **Section 5.1:** Change `rpc_url` to `rpc_urls` (array), add `failover_timeout_ms`
3. **Section 4.1:** Add note about body-modifying intermediaries
4. **Section 10.2:** Fix quote consumption: consume on successful verification, not on reference
5. **Section 8 intro:** Revise throughput claim to separate SQLite vs end-to-end (RPC-limited)
6. **Section 4.2:** Clarify memo encoding (UTF-8 bytes, no truncation needed)
7. **Section 3.2:** Add free-endpoint skip path to middleware diagram
8. **Section 11:** Add RPC timeout retry semantics (503 + Retry-After, idempotent retry)
9. **Section 8:** Add batch write semantics for writer task (10ms / 50-write flush)
10. **New:** Add periodic quote cleanup task
11. **New:** Add response body size limit config (`max_response_body_bytes`)
12. **New:** Add defensive error handling requirements for F2/F4/F5/F8
13. **New:** Add test matrix (29 cases) to Section 16
14. **New:** Add shared test vectors fixture requirement (`tests/fixtures/request_hash_vectors.json`)
15. **Section 5.1:** Add `network = "testnet"` config field

---

## Completion Summary

- **Step 0: Scope Challenge** — Scope accepted as-is (user chose to keep all 4 packages)
- **Architecture Review:** 6 issues found (4 decided, 2 obvious fixes)
- **Code Quality Review:** 4 issues found (2 decided, 2 obvious fixes)
- **Test Review:** Diagram produced, 29 test cases identified, 0 gaps (after adding T26-T29)
- **Performance Review:** 3 issues found (1 decided, 2 obvious fixes)
- **NOT in scope:** Written
- **What already exists:** Written
- **TODOS.md updates:** 2 items added (mainnet config, MPP compatibility)
- **Failure modes:** 4 critical gaps flagged — all resolved
- **Lake Score:** 7/7 recommendations chose complete option
- **Unresolved decisions:** 0
