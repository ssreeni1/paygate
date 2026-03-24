Read worktree-briefs/pane-sponsorship-brief.md in full — it contains your complete build brief with every function signature, SDK change, and test spec.

Before writing any code, read these files:
- SPEC.md (full spec, focus on section 4.4 Fee Sponsorship Flow)
- crates/paygate-gateway/src/sponsor.rs (existing sponsor handler — you are NOT modifying this, just testing it)
- sdk/src/client.ts (PayGateClient you are modifying for auto-session)
- sdk/src/types.ts (types you are extending)
- sdk/src/hash.ts (adding sessionMemo + hmacSha256)
- sdk/src/discovery.ts (parse402Response — you need pricing info)
- sdk/tests/ (existing test patterns)
- sdk/package.json (check existing deps, you may need to add crypto imports)
- demo/paygate.toml (adding sponsorship config)

You are on a feature branch in a git worktree for sponsorship + SDK auto-session.

## Parallelization Strategy

Part A and Part B are completely independent. Use subagents to build both simultaneously.

**Phase 1 — Parallel build (launch 2 subagents simultaneously):**

- **Subagent A (Sponsorship E2E):**
  1. Update demo/paygate.toml — add [sponsorship] section with enabled = true
  2. Create sdk/sponsor-e2e.mjs — standalone E2E test script
  3. Document how to run against deployed instance

- **Subagent B (SDK Auto-Session — types + hash):**
  4. Update sdk/src/types.ts — add autoSession, sessionDeposit to PayGateClientOptions, add session response types (SessionNonceResponse, SessionCreateResponse, SessionInfo)
  5. Update sdk/src/hash.ts — add sessionMemo() and hmacSha256() functions

Wait for both to complete.

**Phase 2 — Client logic (depends on Subagent B):**
6. Update sdk/src/client.ts — add session state, createSession(), computeSessionHeaders(), hasActiveSession(), invalidateSession(), update fetch() with auto-session logic
7. Run `npm run build` (or equivalent) — fix all TypeScript errors
8. **Codex quality + security review (iteration 1):** Run:
   ```bash
   codex exec "Review the changes on this branch. Run git diff main to see the diff. Focus on: 1) HMAC-SHA256 implementation correctness in TypeScript (proper key encoding, constant-time comparison if applicable), 2) Session secret handling (is the secret stored safely in memory? cleared after use?), 3) Auto-session state management (race conditions if multiple requests fire concurrently), 4) Input validation on session responses from gateway, 5) Any credentials or secrets that could leak in error messages or logs. For each finding: severity (critical/high/medium/low), file:line, and recommended fix. Be adversarial." -s read-only -c 'model_reasoning_effort="xhigh"' 2>&1 | tee /tmp/codex-sponsorship-review-1.txt
   ```
   Fix any critical or high severity findings before proceeding.
**Phase 3 — Tests (parallel subagents):**
Launch 2 subagents simultaneously:
- **Subagent C**: Tests 1-3 (auto-session first call creates session, subsequent call uses HMAC, session exhausted triggers auto-renew)
- **Subagent D**: Tests 4-6 (non-auto-session mode unchanged, sessionMemo produces correct hash, hmacSha256 produces correct signature)

After both complete, merge test code and run:
10. Run `npm test` — all tests must pass
11. **Codex quality + security review (iteration 2):** Run:
    ```bash
    codex exec "Review the changes on this branch. Run git diff main to see the diff. This is a SECOND pass — focus on: 1) Test coverage gaps, 2) Edge cases (session expiry mid-request, deposit verification failure, network timeout during session creation), 3) Fee sponsorship config — could a malicious consumer drain the sponsor wallet?, 4) Any new issues from first-review fixes. For each finding: severity, file:line, recommended fix." -s read-only -c 'model_reasoning_effort="xhigh"' 2>&1 | tee /tmp/codex-sponsorship-review-2.txt
    ```
    Fix any critical or high severity findings.

Important: The auto-session feature depends on the sessions endpoints built by Pane 1 (POST /paygate/sessions/nonce and POST /paygate/sessions). Your SDK code calls these endpoints. Make sure the request format matches what the brief specifies:
- Nonce request: POST with X-Payment-Payer header
- Session creation: POST with X-Payment-Tx, X-Payment-Payer headers and JSON body { "nonce": "..." }
- Session auth: X-Payment-Session, X-Payment-Session-Sig (HMAC-SHA256), X-Payment-Timestamp headers

Commit your work with a descriptive message when done. The commit message should mention "SDK auto-session", "fee sponsorship E2E", and "Codex-reviewed".
