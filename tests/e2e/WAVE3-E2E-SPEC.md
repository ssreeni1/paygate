# Wave 3 E2E Simulation Spec

## Overview

Comprehensive E2E simulation for PayGate v0.5.0 exercising every Wave 3 feature against Tempo Moderato testnet with real on-chain transactions. Extends the Wave 2 sim pattern (`tests/e2e/wave2-sim.mjs`).

## Output

- `tests/e2e/wave3-sim.mjs` — executable simulation script
- `tests/e2e/wave3-sim-{date}.json` — structured JSON log with explorer links

## Architecture

```
  wave3-sim.mjs
       │
       ├── Phase 0: Infrastructure
       │     ├── Start gateway (cargo run) with [governance] config
       │     ├── Start demo server (node)
       │     └── Fund test wallet via tempo_fundAddress
       │
       ├── Phase 1: Wave 2 Regression (quick)
       │     ├── 402 negotiation
       │     ├── Session creation (nonce → deposit → create)
       │     └── 3 HMAC-authenticated calls
       │
       ├── Phase 2: MCP Server Tools
       │     ├── paygate_discover (list APIs + AI goal ranking)
       │     ├── paygate_estimate (multi-call cost estimation)
       │     ├── paygate_call (auto-session, real payment)
       │     ├── paygate_budget (check spend status)
       │     └── paygate_trace (start → 3 calls → stop → grouped costs)
       │
       ├── Phase 3: Spend Governance
       │     ├── Agent identity (X-Payment-Agent on session + requests)
       │     ├── GET /paygate/spend with HMAC auth
       │     ├── GET /paygate/spend without auth → 401
       │     ├── Daily limit enforcement → 402 spend_limit_exceeded
       │     └── Verify spend tracking resets (check accumulator)
       │
       ├── Phase 4: SDK Features
       │     ├── estimateCost() — multi-endpoint estimation
       │     ├── failureMode: closed → throws on unreachable
       │     ├── failureMode: open → bypasses (mock upstream)
       │     └── agentName propagation in headers
       │
       ├── Phase 5: Session Lifecycle
       │     ├── Session resume on "restart" (reuse existing session)
       │     ├── Session exhaustion → auto-renew
       │     └── PAYGATE_PRIVATE_KEY_CMD (spawn child with command)
       │
       └── Phase 6: Report
             ├── Pass/fail summary
             ├── Explorer links for all on-chain txs
             ├── Timing data
             └── JSON log to disk
```

## Environment

```bash
# Required
PAYGATE_PRIVATE_KEY=0x...          # or PAYGATE_PRIVATE_KEY_CMD="echo 0x..."

# Optional
GATEWAY_URL=http://127.0.0.1:8080  # default
TEMPO_RPC_URL=https://rpc.moderato.tempo.xyz  # default
SKIP_INFRA=true                    # skip starting gateway/demo
PAYGATE_DAILY_LIMIT=0.02           # spend limit for governance tests (low so we hit it)
```

## Phase Details

### Phase 0: Infrastructure

1. Generate or use provided private key
2. Start demo server on port 3001
3. Create a temporary `paygate-test.toml` with:
   - All standard config from demo/paygate.toml
   - `[governance]` section with `enabled = true`, `default_daily_limit = "0.005"`, `default_monthly_limit = "1.00"`
   - Note: $0.005 daily limit means only 2 calls at $0.002 before limit is hit — this is intentionally low for testing
4. Start gateway with temporary config: `target/debug/paygate serve -c /tmp/paygate-test.toml`
5. Wait for health check (GET /v1/pricing returns 200)
6. Fund wallet via `tempo_fundAddress` if native balance < 0.001 TEMPO
7. Check USDC balance >= 0.1 USDC

### Phase 1: Wave 2 Regression (quick sanity)

Reuse functions from wave2-sim.mjs pattern:
1. Send unauthenticated request → verify 402 with `methods: ["direct", "session"]`
2. Create session: nonce → deposit (real tx) → create → verify response
3. Make 3 HMAC-authenticated calls → verify 200 + X-Payment-Cost header
4. This proves the existing session flow still works after Wave 3 changes

### Phase 2: MCP-Equivalent API Tests

The MCP tools are thin wrappers around the SDK + gateway APIs. For E2E testing, we test the underlying APIs directly — this proves the same functionality without requiring MCP server context initialization.

**Strategy:** Test the gateway APIs and SDK methods that MCP tools wrap, not the MCP tools themselves. MCP tool correctness is covered by the 56 MCP unit tests.

#### 2.1 API Discovery (paygate_discover equivalent)
- Call `GET /v1/pricing`
- Verify: returns array with 4+ APIs, each has endpoint, price, description
- Simulate AI goal ranking: keyword-match "search" against API descriptions, verify search endpoint ranks first

#### 2.2 Cost Estimation (paygate_estimate equivalent)
- Create SDK PayGateClient with `spendLimit: "0.02"` (matches gateway daily limit)
- Call `estimateCost([{ endpoint: "POST /v1/search", count: 5 }, { endpoint: "POST /v1/summarize", count: 2 }])`
- Expected: total = 5 × $0.002 + 2 × $0.003 = $0.016
- Verify: `withinBudget: true` (0.016 < 0.02 spendLimit)
- Also test over-budget: `estimateCost([{ endpoint: "POST /v1/search", count: 20 }])` → `withinBudget: false`

#### 2.3 Authenticated API Call with Agent Identity (paygate_call equivalent)
- Create new session WITH `X-Payment-Agent: "e2e-test-agent"` header on session creation
- Make 3 HMAC-authenticated API calls, each with `X-Payment-Agent` header
- Verify: each response includes X-Payment-Cost header
- Verify: session balance decreases correctly (query GET /paygate/sessions)

#### 2.4 Spend Status Check (paygate_budget equivalent)
- After the 3 calls, query `GET /paygate/spend?payer=<address>`
- Note: this endpoint is currently unauthenticated (auth is a design goal, not yet implemented)
- Verify: `response.daily.spent > 0` (calls were tracked)
- Verify: response includes `daily.limit`, `daily.spent`, `monthly.limit`, `monthly.spent` fields (nested structure per sessions.rs handle_get_spend)

#### 2.5 Workflow Cost Tracking (paygate_trace equivalent)
- Implement local trace tracking (in-memory, same pattern as MCP SpendTracker)
- Start trace: record timestamp
- Make 2 API calls, accumulate costs from X-Payment-Cost headers
- Stop trace: compute total
- Verify: total_cost = sum of 2 calls, calls = 2
- Note: session-authenticated calls don't have per-call tx hashes (off-chain deduction), so explorer links are for the session deposit tx only

### Phase 3: Spend Governance

#### 3.1 Agent Identity Verification
- Query `GET /paygate/sessions?payer=<address>` — verify the session created in Phase 2.3 has `agentName: "e2e-test-agent"`
- Note: /paygate/transactions does NOT currently support agent filtering or return agent_name. Agent identity is stored in request_log and sessions tables. For E2E purposes, we verify via the sessions endpoint.

#### 3.2 /paygate/spend Endpoint
- Call `GET /paygate/spend?payer=<address>` → verify 200 with spend data
- Note: this endpoint is currently unauthenticated per the implementation. HMAC auth was a design goal promoted from reviewer concerns but may not be implemented yet. Test the endpoint as-is.
- Verify: response includes `daily_spent`, `daily_limit` fields
- Call without payer param → verify 400

#### 3.3 Daily Limit Enforcement
- Gateway starts with daily_limit = $0.005 (set in Phase 0 temporary config — extremely low, triggers after 2-3 calls at $0.002 each)
- Query /paygate/spend to see current daily.spent
- Make calls until daily.spent + rate > daily.limit
- Verify: gateway returns 402 with `{"error": "spend_limit_exceeded", "period": "daily"}`
- Verify: the rejected call did NOT deduct from session balance (query balance before and after)

### Phase 4: SDK Features

#### 4.1 estimateCost()
- Create PayGateClient with gateway URL
- Call `estimateCost([{ endpoint: "POST /v1/search", count: 10 }])`
- Verify: returns `{ total: "0.020000", breakdown: [...], withinBudget: false }` (exceeds daily limit)

#### 4.2 failureMode: closed
- Create PayGateClient with `failureMode: 'closed'`, gateway URL = `http://192.0.2.1:9999` (RFC 5737 TEST-NET, guaranteed unreachable)
- Set a 2s timeout to avoid hanging
- Call fetch() → should throw error (not hang, not bypass)

#### 4.3 failureMode: open
- Create PayGateClient with `failureMode: 'open'`, `upstreamUrl: 'http://127.0.0.1:3001'` (demo server directly)
- Gateway URL = `http://192.0.2.1:9999` (unreachable)
- Set a 2s timeout
- Call fetch() → should bypass to upstream and return results (without payment)

#### 4.4 agentName
- Create PayGateClient with `agentName: 'sdk-test-agent'`
- Make a request through the gateway
- Verify: X-Payment-Agent header was sent (check via /paygate/transactions)

### Phase 5: Session Lifecycle

#### 5.1 Session Resume (via stored credentials)
- During Phase 2.3 session creation, save the `sessionId` and `sessionSecret` to a variable
- Simulate "restart": create a brand new client instance (no session state)
- Manually set the saved sessionId + sessionSecret on the new client
- Make an HMAC-authenticated call using the restored credentials
- Verify: call succeeds (200), proving session credentials survive client restart
- Note: This tests the concept — the MCP SessionManager.tryResumeSession() has a separate code path that reads from GET /paygate/sessions, but that endpoint doesn't expose secrets. The practical resume mechanism is persisting credentials locally.

#### 5.2 Session Exhaustion
- Note: daily limit may block this if limit is very low. If so, use a separate session with a higher limit config, or skip and note "blocked by spend limit"
- Keep calling until session balance runs out
- Verify: gateway returns 402 with `insufficient_session_balance`
- Note: auto-renew (new deposit) depends on the SDK autoSession flow which requires a running payFunction — test this only if the full SDK client is configured with a pay function

#### 5.3 PAYGATE_PRIVATE_KEY_CMD
- Write a temp script to /tmp/paygate-key-cmd-test.mjs:
  ```javascript
  import { execSync } from 'child_process';
  const cmd = process.env.PAYGATE_PRIVATE_KEY_CMD;
  const key = execSync(cmd).toString().trim();
  if (key.startsWith('0x') && key.length === 66) { console.log('KEY_LOADED'); process.exit(0); }
  else { console.error('INVALID_KEY'); process.exit(1); }
  ```
- Spawn with `PAYGATE_PRIVATE_KEY_CMD="echo 0x<test-key>"`
- Verify: child exits 0 and stdout contains "KEY_LOADED"
- Also test failure: `PAYGATE_PRIVATE_KEY_CMD="exit 1"` → child exits non-zero

### Phase 6: Report

```json
{
  "version": "0.5.0",
  "date": "2026-03-24",
  "duration_ms": 12345,
  "phases": {
    "wave2_regression": { "tests": 4, "passed": 4 },
    "mcp_tools": { "tests": 5, "passed": 5 },
    "governance": { "tests": 4, "passed": 4 },
    "sdk_features": { "tests": 4, "passed": 4 },
    "session_lifecycle": { "tests": 3, "passed": 3 }
  },
  "total": { "tests": 20, "passed": 20, "failed": 0 },
  "transactions": [
    { "step": "session.deposit", "tx_hash": "0x...", "explorer": "https://explore.moderato.tempo.xyz/tx/0x..." },
    ...
  ],
  "steps": [ ... full step log ... ]
}
```

## Error Handling

- If any phase fails, log the failure and continue to the next phase (don't abort)
- Exception: Phase 0 failures (infra not starting) abort the entire sim
- Each step logs: step name, status (PASS/FAIL/SKIP), timing, error message if failed
- Final report shows which phases passed/failed

## Dependencies

- `viem` (from sdk/node_modules or tests/e2e/node_modules)
- `crypto` (Node.js built-in)
- Gateway binary (`target/debug/paygate`)
- Demo server (`demo/dist/server.js`)
- Tempo Moderato testnet (rpc.moderato.tempo.xyz)

## Success Criteria

- All 20 tests pass
- At least 2 real on-chain transactions confirmed (session deposits)
- Spend limit enforcement verified (402 returned)
- /paygate/spend HMAC auth verified (401 without, 200 with)
- Agent identity tracked in transactions
- JSON log written with explorer links
- Total execution time < 60 seconds

## Not in Scope

- MCP protocol-level testing (stdio transport, tool registration) — tested by MCP unit tests
- Monthly limit testing (would require mocking time)
- Multi-payer scenarios
- Mainnet testing
