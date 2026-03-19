# PayGate Design Review — Plan Phase

**Date:** 2026-03-18
**Reviewed by:** /plan-design-review
**Scope:** SPEC.md (full plan, pre-implementation)
**Initial score:** 3/10 | **Final score:** 8/10

---

## Design Decisions Made

### 1. CLI Tone: Minimal + Professional

All CLI output follows the nginx/caddy philosophy — clean, quiet, let the logs speak.
No banners, no emoji (except checkmarks in test output), no color by default.

### 2. CLI Output Conventions

- 2-space indent for all output blocks
- Section headers: text + `───` underline
- Errors: `error: <message>` + indented `hint: <fix>` (Rust compiler style)
- Timestamps: ISO 8601 in request logs
- Status markers: `✓` (pass), `✗` (fail)
- No emoji. No color by default (respect `NO_COLOR` env var).
- All monetary amounts: `$X.XX` format (USD equivalent)
- Wallet addresses: truncated `0x7F3a...Provider`
- Terminal width: degrade gracefully in narrow terminals

### 3. CLI Output Specifications

#### `paygate serve`

```
$ paygate serve

  PayGate v0.1.0
  Proxy: 0.0.0.0:8080 → localhost:3000
  Tempo: rpc.tempo.xyz (connected)

  Ready. Accepting payments.

2026-03-18T12:00:01Z  POST /v1/chat  402  →  0.005 USDC
2026-03-18T12:00:03Z  POST /v1/chat  200  ←  0.005 USDC  tx:0xab..cd  47ms
```

**Error states:**

```
$ paygate serve

  PayGate v0.1.0

  error: Tempo RPC unreachable
    rpc_url = "https://rpc.tempo.xyz"
    hint: check your network or verify the URL in paygate.toml

$ paygate serve

  error: port 8080 already in use
    hint: set gateway.listen in paygate.toml or kill the existing process

$ paygate serve

  error: config not found
    hint: run `paygate init` to create paygate.toml
```

#### `paygate init`

3-question minimal wizard. Ask only what can't be inferred.

```
$ paygate init

  PayGate Setup
  ──────────────

  Upstream API URL: http://localhost:3000
  Provider wallet address: 0x7F3a...
  Private key env var [PAYGATE_PRIVATE_KEY]:

  Created paygate.toml
  Default price: $0.001/request (edit paygate.toml to customize)

  Next steps:
    export PAYGATE_PRIVATE_KEY=<your-tempo-private-key>
    paygate serve
    paygate test    # verify on testnet
```

**Error states:**
- Invalid URL → `error: invalid URL` + `hint: include the scheme (http:// or https://)`
- Invalid address → `error: invalid Ethereum address` + `hint: must start with 0x and be 42 characters`
- paygate.toml already exists → `error: paygate.toml already exists` + `hint: use --force to overwrite`

#### `paygate revenue`

```
$ paygate revenue

  Revenue Summary
  ───────────────
  24h     $12.45   2,490 requests
   7d     $67.30  13,460 requests
  30d    $245.10  49,020 requests

  Top endpoints (24h):
    POST /v1/chat/completions   $10.20  (2,040 req)
    POST /v1/embeddings          $2.15    (430 req)
    GET  /v1/models              $0.00     (20 req)  free
```

**Empty state (zero requests):**
```
$ paygate revenue

  Revenue Summary
  ───────────────
  No payments recorded yet.

  hint: run `paygate test` to verify your setup, or send a request to your gateway
```

#### `paygate test`

```
$ paygate test

  PayGate end-to-end test (tempo-testnet)
  ─────────────────────────────────────
  Starting echo server on :9999
  Starting gateway on :8080 → :9999

  [1/6] Request without payment     402 ✓
  [2/6] Fund test wallet            0.01 USDC ✓
  [3/6] Pay and retry               200 ✓  (47ms verify)
  [4/6] Replay same tx              402 ✓
  [5/6] Wrong payer address         402 ✓
  [6/6] Insufficient amount         402 ✓

  All tests passed. Verification latency: 47ms p50, 62ms p99
```

**Failure state:**
```
  [1/6] Request without payment     402 ✓
  [2/6] Fund test wallet            0.01 USDC ✓
  [3/6] Pay and retry               FAIL ✗
    expected: 200, got: 402
    tx_hash: 0xabc...def
    hint: check that the gateway can reach Tempo RPC

  1 of 6 tests failed.
```

#### `paygate status`

```
$ paygate status

  PayGate Status
  ──────────────
  Gateway    running  0.0.0.0:8080
  Upstream   healthy  localhost:3000
  Tempo RPC  connected  rpc.tempo.xyz
  DB         ok  paygate.db (1.2 MB)
  Uptime     4h 23m
  Requests   2,490 (24h)
  Revenue    $12.45 (24h)
```

**Degraded states:**
```
  Upstream   unreachable  localhost:3000
  Tempo RPC  error  rpc.tempo.xyz (timeout)
```

#### `paygate sessions`

```
$ paygate sessions

  Active Sessions
  ───────────────
  ID            Payer          Balance    Requests  Expires
  sess_a1b2..   0x9E2b...Con   $0.032     36        2h 15m
  sess_c3d4..   0x4F1a...Bot   $0.008     84        45m

  2 active sessions, $0.040 total balance
```

**Empty state:**
```
$ paygate sessions

  No active sessions.
```

#### `paygate pricing`

```
$ paygate pricing

  Pricing Table
  ─────────────
  Endpoint                       Price
  POST /v1/chat/completions      $0.005
  POST /v1/embeddings            $0.001
  GET  /v1/models                free
  *  (default)                   $0.001
```

### 4. 402 Response — Human-Readable Enhancement

The 402 JSON body now includes `message` and `help_url` fields for developer debugging:

```json
{
  "error": "payment_required",
  "message": "Send 0.005000 USDC to 0x7F3a...Provider on Tempo, then retry with X-Payment-Tx header.",
  "help_url": "https://ssreeni1.github.io/paygate/quickstart#paying",
  "pricing": {
    "amount": "0.005000",
    "amount_base_units": 5000,
    "decimals": 6,
    "token": "0x...USDC",
    "recipient": "0x7F3a...Provider",
    "quote_id": "qt_a1b2c3d4",
    "quote_expires_at": "2026-03-18T12:00:00Z",
    "methods": ["direct", "session"]
  }
}
```

### 5. Interaction State Coverage

| Feature | Loading | Empty | Error | Success | Partial |
|---------|---------|-------|-------|---------|---------|
| `paygate serve` startup | N/A | N/A | error+hint | startup block | N/A |
| `paygate revenue` | N/A | "No payments recorded" + hint | DB error+hint | summary table | N/A |
| `paygate test` | step-by-step progress | N/A | per-step FAIL with details | all passed + latency | partial pass count |
| `paygate init` | N/A | N/A | per-field validation+hint | created + next steps | N/A |
| `paygate status` | N/A | N/A | per-component degraded | all healthy | mixed healthy/degraded |
| `paygate sessions` | N/A | "No active sessions" | DB error | session table | N/A |
| 402 response | N/A | N/A | N/A | message+help_url+pricing | N/A |
| Health endpoint | N/A | N/A | per-component status | all healthy JSON | degraded JSON |

---

## NOT in Scope

| Item | Rationale |
|------|-----------|
| Dashboard UI design | Deferred to v0.3. Needs full /design-consultation before implementation. |
| DESIGN.md / full design system | CLI tool doesn't warrant a full design system. CLI Output Conventions section covers it. |
| Color scheme / theming | Respecting NO_COLOR by default. Color is a nice-to-have, not a design decision for MVP. |
| Mobile/responsive | CLI tool — not applicable. Dashboard will need this in v0.3. |

## What Already Exists

Nothing — greenfield project. SPEC.md is the only artifact.

## Deferred Design Work (TODOs)

### 1. Dashboard Design (v0.3)

**What:** Full design specification for the React revenue dashboard — screens, data visualization, interactions, responsive behavior.
**Why:** Currently described as "React revenue analytics" with zero design. Without design work, it will ship as generic AI-generated dashboard slop.
**Depends on:** v0.2 completion, /design-consultation run.

### 2. Quickstart Documentation for Payment Flow

**What:** Write `ssreeni1.github.io/paygate/quickstart#paying` — the page linked from every 402 response's `help_url`.
**Why:** The `help_url` field in 402 responses is useless without actual docs. This is a pre-launch requirement for v0.1.
**Depends on:** Finalized 402 response format, working payment flow.

---

## Completion Summary

```
+====================================================================+
|         DESIGN PLAN REVIEW — COMPLETION SUMMARY                    |
+====================================================================+
| System Audit         | No DESIGN.md, no code, spec-only stage      |
| Step 0               | 3/10 initial, CLI + DX focus areas           |
| Pass 1  (Info Arch)  |  2/10 →  7/10 after CLI mockups              |
| Pass 2  (States)     |  1/10 →  7/10 after error/empty specs        |
| Pass 3  (Journey)    |  3/10 →  7/10 after init wizard + arc        |
| Pass 4  (AI Slop)    |  6/10 →  8/10 (protocol spec, low risk)      |
| Pass 5  (Design Sys) |  2/10 →  8/10 after CLI conventions          |
| Pass 6  (Responsive) |  N/A  →  7/10 (CLI-appropriate a11y)         |
| Pass 7  (Decisions)  |  3 resolved, 0 deferred                      |
+--------------------------------------------------------------------+
| NOT in scope         | written (4 items)                            |
| What already exists  | written (nothing — greenfield)               |
| TODOS.md updates     | 2 items proposed                             |
| Decisions made       | 8 added to plan                              |
| Decisions deferred   | 0                                            |
| Overall design score | 3/10 → 8/10                                  |
+====================================================================+
```

Plan is design-complete for MVP scope. Run /design-review after implementation for visual QA of CLI output.
