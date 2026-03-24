Read worktree-briefs/pane-sessions-brief.md in full — it contains your complete build brief with every function signature, DB operation, integration point, and test spec.

Before writing any code, read these files:
- SPEC.md (full spec, focus on section 4.3 Session Flow)
- crates/paygate-gateway/src/serve.rs (gateway_handler you are modifying)
- crates/paygate-gateway/src/db.rs (writer pattern you must follow exactly)
- crates/paygate-gateway/src/config.rs (config structs — change discount default from 50 to 0, add no_charge_on_5xx)
- crates/paygate-gateway/src/verifier.rs (on-chain verification pattern to reuse for deposit verification)
- crates/paygate-gateway/src/server.rs (AppState struct)
- crates/paygate-common/src/mpp.rs (header constants — X-Payment-Session etc. already defined)
- crates/paygate-common/src/hash.rs (keccak256 helper)
- schema.sql (sessions table already exists)
- Cargo.toml files for both gateway and common crates (check existing deps before adding hmac/sha2/rand)

You are on a feature branch in a git worktree for sessions.

## Parallelization Strategy

Use subagents (the Agent tool) to parallelize independent work. This is a large feature — maximize throughput.

**Phase 1 — Foundation (parallel subagents):**
Launch 3 subagents simultaneously:
- **Subagent A**: Update schema.sql (add session_nonces table) + update config.rs (discount_percent default to 0, add no_charge_on_5xx vec and helper) + add crate dependencies to Cargo.toml (hmac, sha2, rand)
- **Subagent B**: Update db.rs — add FullSessionRecord, NonceRecord structs, WriteCommand variants (CreateSessionNonce, CreateSession, DeductSessionBalance, RefundSessionBalance), DbReader queries (get_session_nonce, get_session, count_active_sessions_for_payer), DbWriter methods
- **Subagent C**: Create sessions.rs skeleton — define SessionDeduction struct, SessionError enum, function signatures for handle_nonce, handle_create_session, verify_and_deduct with TODO bodies

Wait for all 3 to complete.

**Phase 2 — Core logic (sequential, depends on Phase 1):**
- Fill in sessions.rs function bodies (handle_nonce, handle_create_session, verify_and_deduct, no_charge_on_5xx refund logic)
- Update serve.rs — add session route wiring + session auth branch in gateway_handler
- Register `mod sessions` in main.rs

**Phase 3 — Verify + Review:**
8. Run `cargo check` — fix all errors
9. **Codex quality + security review (iteration 1):** Run:
   ```bash
   codex exec "Review the changes on this branch. Run git diff main to see the diff. Focus on: 1) HMAC implementation correctness (constant-time comparison, proper key derivation), 2) SQL injection or SQLite safety issues, 3) Race conditions in session balance deduction, 4) Timing attacks on session secret, 5) Integer overflow in balance arithmetic. For each finding: severity (critical/high/medium/low), file:line, and recommended fix. Be adversarial." -s read-only -c 'model_reasoning_effort="xhigh"' 2>&1 | tee /tmp/codex-sessions-review-1.txt
   ```
   Fix any critical or high severity findings before proceeding.
**Phase 4 — Tests (parallel subagents):**
Launch 2 subagents simultaneously to write tests in sessions.rs `#[cfg(test)] mod tests`:
- **Subagent D**: Tests 1-5 (nonce happy path, max concurrent exceeded, create session with valid deposit, invalid nonce, HMAC verification happy path)
- **Subagent E**: Tests 6-10 (invalid HMAC, stale timestamp, insufficient balance, session expired, no_charge_on_5xx refund)

After both complete, merge their test code into sessions.rs and run:
11. Run `cargo test` — all tests must pass
12. **Codex quality + security review (iteration 2):** Run:
    ```bash
    codex exec "Review the changes on this branch. Run git diff main to see the diff. This is a SECOND pass — focus on: 1) Test coverage gaps (are there untested error paths?), 2) Edge cases in session expiry, concurrent deduction, zero-balance handling, 3) Any new issues introduced by fixes from the first review, 4) Memory safety concerns. For each finding: severity, file:line, recommended fix." -s read-only -c 'model_reasoning_effort="xhigh"' 2>&1 | tee /tmp/codex-sessions-review-2.txt
    ```
    Fix any critical or high severity findings.

Commit your work with a descriptive message when done. The commit message should mention "sessions protocol", "HMAC auth", "no_charge_on_5xx", and "Codex-reviewed".
