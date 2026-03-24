Read worktree-briefs/pane-mcp-server-brief.md in full — it contains your complete build brief with every function signature, type definition, MCP tool schema, and test spec.

Before writing any code, read these files:

- worktree-briefs/pane-mcp-server-brief.md (your build brief — READ FIRST)
- sdk/src/client.ts (PayGateClient you are wrapping)
- sdk/src/hash.ts (hash functions used by SDK)
- sdk/src/types.ts (types you import/extend)
- sdk/src/discovery.ts (pricing discovery you reuse)
- sdk/src/index.ts (SDK exports)
- sdk/package.json (existing SDK package — do not modify)
- sdk/tsconfig.json (TypeScript config to match)

You are on a feature branch in a git worktree for the MCP server.

## Build Order

The MCP server is a new TypeScript package at `packages/mcp-server/`. It depends on `@paygate/sdk` via npm workspaces. The entry point runs as a stdio MCP server for Claude Code / Cursor.

Key constraints:
- npm workspaces: root `package.json` with `workspaces: ["sdk", "packages/*"]`
- The MCP server uses `@modelcontextprotocol/sdk` for the MCP protocol
- `@paygate/sdk` is linked via workspace, not published
- All output to stderr (stdout is reserved for MCP JSON-RPC transport)
- Every error returns structured JSON with one of 6 error codes
- Tests use vitest (same as SDK)

## Parallelization Strategy

### Phase 1 — Scaffold (3 parallel subagents)

Launch 3 subagents simultaneously:

**Subagent A — npm workspaces + package scaffold:**
1. Create root `/package.json` with `{ "private": true, "workspaces": ["sdk", "packages/*"] }`
2. Create `packages/mcp-server/package.json` (see brief for exact contents — name: `@paygate/mcp`, bin: `paygate-mcp`, dependencies on `@paygate/sdk` workspace and `@modelcontextprotocol/sdk`)
3. Create `packages/mcp-server/tsconfig.json` (match SDK tsconfig style, add reference to `../../sdk`)
4. Create empty directory structure: `packages/mcp-server/src/tools/`, `packages/mcp-server/tests/`
5. Run `npm install` from root to verify workspace linking works

**Subagent B — types + errors + utilities:**
1. Create `packages/mcp-server/src/types.ts` with all interfaces from the brief: `McpServerConfig`, `EndpointPricing`, `PricingCache`, `SessionState`, `SpendRecord`, `ActiveTrace`, `TraceEntry`, `PaygateToolSuccess`, `PaygateToolError`, `PaygateErrorCode`, and all tool input types (`DiscoverInput`, `CallInput`, `BudgetInput`, `EstimateInput`, `TraceInput`)
2. Create `packages/mcp-server/src/errors.ts` with all 6 error constructors + `classifyError()` + `errorToMcpContent()`
3. Create `packages/mcp-server/src/key-loader.ts` with `loadPrivateKey()` + `normalizeKey()` — handles `PAYGATE_PRIVATE_KEY_CMD` via `execSync` and `PAYGATE_PRIVATE_KEY` fallback
4. Create `packages/mcp-server/src/spend-tracker.ts` with `SpendTracker` class + `parseUsdcToBaseUnits()` + `formatUsd()`
5. Create `packages/mcp-server/src/pricing-cache.ts` with `PricingCacheManager` class

**Subagent C — llms.txt + session manager:**
1. Create `docs/llms.txt` with the content from the brief (PayGate description, quick start MCP config, tool list, typical workflow, payment flow, security notes)
2. Create `packages/mcp-server/src/session-manager.ts` with `SessionManager` class — `getSession()`, `getBalance()`, `deductBalance()`, `updateFromSdkResponse()`, `tryResumeSession()`, `setSession()`, `invalidate()`, `logShutdownState()`

Wait for all 3 to complete.

**Codex review gate 1:**
```bash
codex exec "Review the new files in packages/mcp-server/src/ and the root package.json. Check: 1) TypeScript types are consistent and complete, 2) Error codes match the 6 defined in the design doc, 3) SpendTracker UTC rollover logic is correct, 4) key-loader has no shell injection vulnerabilities in execSync, 5) PricingCacheManager TTL logic is correct. For each finding: severity, file:line, fix." -s read-only -c 'model_reasoning_effort="xhigh"' 2>&1 | tee /tmp/codex-mcp-review-1.txt
```
Fix any critical or high severity findings.

### Phase 2 — Core tools (sequential)

These depend on Phase 1 types/utilities.

1. Create `packages/mcp-server/src/tools/discover.ts` — `handleDiscover()` function. Takes `PricingCacheManager`, returns handler that accepts `DiscoverInput`. When `goal` is provided, rank endpoints by keyword overlap with goal using `tokenize()` + `rankByGoal()`. Include `computeRelevanceNote()`.

2. Create `packages/mcp-server/src/tools/call.ts` — `handleCall()` function. Takes `PayGateClient`, `McpServerConfig`, `SpendTracker`, `SessionManager`, `PricingCacheManager`, `Map<string, ActiveTrace>`. Validates method/path, checks spend limit via `spendTracker.checkLimit()`, calls `client.fetch()`, updates spend tracker, updates session manager from response headers, appends to all active traces, builds explorer link, returns structured result with payment metadata. Handles upstream 5xx with `upstreamError()`.

3. Create `packages/mcp-server/src/tools/budget.ts` — `handleBudget()` function. Takes `SpendTracker`, `SessionManager`, `McpServerConfig`. Returns session info, spending record, limits with remaining amounts.

4. Create `packages/mcp-server/src/index.ts` — full entry point. Loads config from env, initializes `privateKeyToAccount`, creates `PayGateClient` with `autoSession: true`, creates all managers/trackers, defines MCP tool schemas, registers `ListToolsRequestSchema` and `CallToolRequestSchema` handlers, registers SIGINT/SIGTERM shutdown handlers that call `sessionManager.logShutdownState()`, starts `StdioServerTransport`, calls `tryResumeSession()`.

5. Verify: `cd packages/mcp-server && npx tsc --noEmit` — fix all type errors.

**Codex review gate 2:**
```bash
codex exec "Review packages/mcp-server/src/tools/ and src/index.ts. Check: 1) paygate_call correctly propagates X-Payment-Agent header, 2) SIGINT/SIGTERM handlers don't throw, 3) MCP tool schemas match the design doc exactly, 4) No secrets leak to stdout (only stderr), 5) Tool dispatch switch statement covers all 5 tools + default. For each finding: severity, file:line, fix." -s read-only -c 'model_reasoning_effort="xhigh"' 2>&1 | tee /tmp/codex-mcp-review-2.txt
```
Fix any critical or high severity findings.

### Phase 3 — Expansion tools + shutdown/resume (2 parallel subagents)

**Subagent D — paygate_estimate + paygate_trace:**
1. Create `packages/mcp-server/src/tools/estimate.ts` — `handleEstimate()` function. Takes `PricingCacheManager`, `SpendTracker`. Validates calls array, looks up each endpoint in pricing cache, computes subtotals and total, marks dynamic endpoints as approximate, checks `withinBudget` against remaining daily limit.

2. Create `packages/mcp-server/src/tools/trace.ts` — `handleTrace()` function. Takes `Map<string, ActiveTrace>`. Handles `start` (create trace entry in map) and `stop` (remove from map, aggregate entries by endpoint, compute totals, return breakdown with explorer links).

**Subagent E — shutdown/resume polish:**
1. Verify SIGINT/SIGTERM handlers in `index.ts` call `sessionManager.logShutdownState()` and log spend summary via `spendTracker.getRecord()`
2. Verify `tryResumeSession()` in `session-manager.ts` calls `GET /paygate/sessions?payer=<address>`, handles 4xx/5xx gracefully, logs found sessions to stderr
3. Verify the `#!/usr/bin/env node` shebang is at the top of `index.ts`
4. Add to `index.ts`: after `server.connect(transport)`, log startup info to stderr

Wait for both to complete.

5. Run `cd packages/mcp-server && npx tsc --noEmit` — fix all type errors.

### Phase 4 — Tests (3 parallel subagents)

**Subagent F — utility tests:**
1. `tests/key-loader.test.ts` — 4 tests: load from env, add 0x prefix, throw on missing, reject invalid format + 3 tests: load from CMD, CMD priority, CMD empty
2. `tests/spend-tracker.test.ts` — 5 tests: within limit, exceeds limit, cumulative tracking, unlimited remaining, computed remaining + parseUsdcToBaseUnits + formatUsd
3. `tests/errors.test.ts` — 5 tests: classify ECONNREFUSED, classify balance, classify nonce, classify unknown, non-Error + errorToMcpContent

**Subagent G — tool tests:**
1. `tests/discover.test.ts` — 3 tests: without goal, with goal ranking, gateway unreachable error
2. `tests/estimate.test.ts` — 5 tests: correct total, dynamic approximate, over budget, unknown endpoint, empty calls
3. `tests/trace.test.ts` — 4 tests: start, stop with summary, duplicate start, stop non-existent

**Subagent H — integration tests:**
1. `tests/session-manager.test.ts` — 6 tests: null when empty, returns when set, null when expired, deduct balance, log shutdown with session, log shutdown without session
2. `tests/budget.test.ts` — 2 tests: with session, without session
3. `tests/call.test.ts` — 3 tests: spend limit rejection, invalid method, missing method/path
4. `tests/pricing-cache.test.ts` — 5 tests: first fetch, cached second call, refresh after invalidate, lookup specific, lookup unknown
5. `tests/integration.test.ts` — 6 tests: all 5 tools defined, call requires method+path, estimate requires calls, trace requires action+name, discover goal optional, budget no required params

Wait for all 3 to complete.

6. Run `cd packages/mcp-server && npm test` — all tests must pass.

**Codex review gate 3:**
```bash
codex exec "Review all test files in packages/mcp-server/tests/. Check: 1) Are there untested error paths? List them. 2) Do mocks accurately represent real SDK/gateway behavior? 3) Are there race conditions in async test setup? 4) Is test isolation correct (env vars restored, mocks cleaned up)? 5) Do the 12+ tests provide adequate coverage of the 5 MCP tools + utilities? For each finding: severity, file:line, fix." -s read-only -c 'model_reasoning_effort="xhigh"' 2>&1 | tee /tmp/codex-mcp-review-3.txt
```
Fix any critical or high severity findings.

### Final Verification

```bash
# From repo root
npm install                          # workspace linking
cd packages/mcp-server && npx tsc   # full build
cd packages/mcp-server && npm test  # all tests
cd sdk && npm test                   # SDK tests still pass
```

Commit your work with a descriptive message when done. The commit message should mention "MCP server", "paygate_discover/call/budget/estimate/trace", "PAYGATE_PRIVATE_KEY_CMD", "session cleanup", and "Codex-reviewed".
