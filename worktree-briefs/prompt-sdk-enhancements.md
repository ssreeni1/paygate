Read worktree-briefs/pane-sdk-enhancements-brief.md in full — it contains your complete build brief with every type definition, function signature, integration point, and test spec.

Before writing any code, read these files:
- sdk/src/types.ts (types you are extending)
- sdk/src/client.ts (PayGateClient — the main file you are modifying)
- sdk/src/discovery.ts (pricing discovery — you are adding fetchEndpointPricing)
- sdk/src/hash.ts (hash functions — read-only, no changes needed, but understand the imports)
- sdk/src/index.ts (exports — you will add new exports)
- sdk/tests/client.test.ts (existing test patterns — match these exactly)
- sdk/package.json (verify dependencies — no new deps needed)
- CLAUDE.md (project conventions: no emoji except checkmark, 2-space indent, CLI output rules)

You are on a feature branch in a git worktree for SDK enhancements.

## Parallelization Strategy

Use subagents (the Agent tool) to parallelize independent work. Three features are independent at the type/implementation level but share the `PayGateClient` class, so we split by phase.

**Phase 1 — Types + Discovery (parallel subagents):**
Launch 2 subagents simultaneously:

- **Subagent A: Types + estimateCost infrastructure**
  1. Read `sdk/src/types.ts`
  2. Add `FailureMode` type alias
  3. Add `failureMode?`, `upstreamUrl?`, `agentName?`, `spendLimit?` fields to `PayGateClientOptions`
  4. Add `EndpointPricing` interface
  5. Add `EstimateCostEntry` interface
  6. Add `EstimateCostResult` interface

- **Subagent B: Discovery enhancement**
  1. Read `sdk/src/discovery.ts`
  2. Add `fetchEndpointPricing(baseUrl: string): Promise<Map<string, EndpointPricing>>` function
  3. Read `sdk/src/index.ts`
  4. Add `fetchEndpointPricing` to the discovery exports in `index.ts`
  5. Add new type exports to `index.ts` if needed

Wait for both to complete.

**Phase 2 — Client implementation (sequential, 2 parallel subagents):**
Launch 2 subagents simultaneously:

- **Subagent C: estimateCost + failureMode**
  1. Read `sdk/src/client.ts` (latest version after Phase 1 types are available)
  2. Add new private fields: `failureMode`, `upstreamUrl`, `spendLimit`, `pricingCache`, `pricingCacheExpiry`, `PRICING_CACHE_TTL_MS`
  3. Update constructor to parse new options + validate failureMode/upstreamUrl
  4. Add `formatUsdc()` helper (module-level function)
  5. Add `getOrFetchPricing()` private method
  6. Add `estimateCost()` public method
  7. Add `isNetworkError()` private method
  8. Add `bypassToUpstream()` private method
  9. Rename existing `fetch()` body to `private async _fetchInner()`
  10. Create new `fetch()` wrapper with try-catch for failureMode

- **Subagent D: agentName propagation**
  1. Read `sdk/src/client.ts` (same version as Subagent C reads)
  2. Add `agentName` private field (if Subagent C hasn't already — coordinate via constructor)
  3. Modify `mergeHeaders()` to always inject `X-Payment-Agent` when `agentName` is set
  4. Wrap all standalone `fetch(url, init)` calls (the initial probing requests that don't go through `mergeHeaders`) to use `this.mergeHeaders(init, {})` instead
  5. Update `createSession()` nonce request headers to include `X-Payment-Agent`
  6. Update `createSession()` session creation headers to include `X-Payment-Agent`

**IMPORTANT**: Subagents C and D both modify `client.ts`. Subagent C owns the constructor, new methods, and the fetch wrapper. Subagent D owns `mergeHeaders()` and `createSession()` header objects. If running truly in parallel, merge carefully — or run D after C completes (safer). Recommended: run C first, then D.

**Phase 3 — Tests (parallel subagents):**
Launch 3 subagents simultaneously:

- **Subagent E: estimateCost tests (tests 5.2-5.7 from brief)**
  Write tests in `sdk/tests/client.test.ts` inside a new `describe('estimateCost', ...)` block:
  1. Happy path — multiple endpoints, verify total + breakdown
  2. withinBudget true when under limit
  3. withinBudget false when over limit
  4. Pricing cache hit on second call (verify fetch called only once)
  5. Unknown endpoint throws
  6. Empty calls array returns zero
  7. Dynamic endpoint marked in breakdown

- **Subagent F: failureMode tests (tests 5.8-5.11 from brief)**
  Write tests in `sdk/tests/client.test.ts` inside a new `describe('failureMode', ...)` block:
  1. Closed (default) throws on network error
  2. Open bypasses to upstream on network error, verify correct upstream URL
  3. Open does NOT bypass on HTTP 5xx (returns the 500 response as-is)
  4. Open without upstreamUrl throws at construction time

- **Subagent G: agentName tests (tests 5.12-5.13 from brief)**
  Write tests in `sdk/tests/client.test.ts` inside a new `describe('agentName', ...)` block:
  1. X-Payment-Agent on every outgoing request (direct payment flow)
  2. X-Payment-Agent on session nonce + creation requests (auto-session flow)

After all 3 complete, merge test code into `sdk/tests/client.test.ts`.

**Phase 4 — Verify + Review:**

1. Run `cd sdk && npm run build` — fix any TypeScript compilation errors
2. Run `cd sdk && npm test` — all tests must pass (existing + new)
3. **Codex quality review (iteration 1):** Run:
   ```bash
   codex exec "Review the changes on this branch. Run git diff main to see the diff. Focus on: 1) Does failureMode only trigger on network errors (TypeError, AbortError) and NOT on 4xx/5xx responses? 2) Is X-Payment-Agent injected on ALL outgoing fetch calls including session nonce and creation? 3) Does bypassToUpstream correctly strip X-Payment-* headers? 4) Is the pricing cache TTL correctly implemented (60s expiry, not infinite)? 5) Are there any TypeScript type errors or 'any' escapes? For each finding: severity (critical/high/medium/low), file:line, and recommended fix." -s read-only -c 'model_reasoning_effort="xhigh"' 2>&1 | tee /tmp/codex-sdk-enhancements-review-1.txt
   ```
   Fix any critical or high severity findings before proceeding.
4. **Codex quality review (iteration 2):** Run:
   ```bash
   codex exec "Review the changes on this branch. Run git diff main to see the diff. This is a SECOND pass — focus on: 1) Test coverage gaps — are there untested paths in failureMode or estimateCost? 2) Edge cases: what if fetch throws a non-TypeError non-network error in open mode? What if pricing endpoint returns empty apis array? What if agentName contains special characters? 3) Does the bypass path correctly preserve query string parameters? 4) Any issues introduced by first-round fixes. For each finding: severity, file:line, recommended fix." -s read-only -c 'model_reasoning_effort="xhigh"' 2>&1 | tee /tmp/codex-sdk-enhancements-review-2.txt
   ```
   Fix any critical or high severity findings.

## Build Order Summary

```
Phase 1 (parallel):  types.ts  |  discovery.ts + index.ts
                         \           /
Phase 2 (sequential): client.ts (estimateCost + failureMode, then agentName)
                              |
Phase 3 (parallel):  estimateCost tests | failureMode tests | agentName tests
                              \              |              /
Phase 4 (sequential): npm build → npm test → codex review x2 → fix → commit
```

## Key Constraints

- Do NOT modify any Rust code. This stream is TypeScript SDK only.
- Do NOT add new npm dependencies. All features use built-in `fetch()` and existing `viem` dep.
- Match existing code style: single quotes, 2-space indent, explicit return types on public methods, `Record<string, string>` for header objects.
- All `fetch()` calls that leave the SDK must go through `mergeHeaders()` so that `X-Payment-Agent` is consistently applied.
- The `bypassToUpstream()` path must strip `X-Payment-*` headers (security: don't leak payment metadata to upstream).
- Constructor must throw synchronously if `failureMode: 'open'` is set without `upstreamUrl`.

Commit your work with a descriptive message when done. The commit message should mention "estimateCost", "failureMode", "agentName", and "Codex-reviewed".
