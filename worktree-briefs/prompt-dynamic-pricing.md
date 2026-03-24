Read worktree-briefs/pane-dynamic-pricing-brief.md in full — it contains your complete build brief with every function signature, config change, and test spec.

IMPORTANT: This pane runs AFTER the sessions branch has been merged to main. Do NOT start until sessions are merged. You depend on sessions.rs, the session auth branch in serve.rs, and the session DB operations.

Before writing any code, read these files:
- SPEC.md (full spec, focus on section 4.3 Session Flow — dynamic pricing builds on top)
- crates/paygate-gateway/src/serve.rs (gateway_handler — you are modifying the session auth branch)
- crates/paygate-gateway/src/sessions.rs (session module — you are adding deduct_additional and handle_get_sessions)
- crates/paygate-gateway/src/config.rs (DynamicPricingConfig — you are updating with compute_cost method)
- crates/paygate-gateway/src/mpp.rs (402 response — adding dynamic pricing note)
- crates/paygate-gateway/src/db.rs (adding list_sessions_for_payer to DbReader)
- demo/src/routes/summarize.ts (adding X-Token-Count header)
- demo/src/routes/search.ts (adding X-Token-Count header)
- demo/paygate.toml (adding [pricing.dynamic] section)
- docs/marketplace.html (adding session balance widget)

You are on a feature branch for dynamic pricing, branched from main AFTER sessions merge.

## Parallelization Strategy

Parts A, B, and C are mostly independent. Use subagents to maximize throughput.

**Phase 1 — Foundation (3 parallel subagents):**

- **Subagent A (Gateway config + helpers):**
  1. Update config.rs — add compute_cost() to DynamicPricingConfig, add default_header_source, update field names to match spec (base_cost_per_token, spread_per_token)
  3. Update sessions.rs — add deduct_additional() helper
  4. Update mpp.rs — add dynamic pricing note to 402 response when dynamic pricing is enabled

- **Subagent B (Demo server headers):**
  8. Update demo/src/routes/summarize.ts — add X-Token-Count response header
  9. Update demo/src/routes/search.ts — add X-Token-Count response header
  10. Update demo/paygate.toml — add [pricing.dynamic] config section

- **Subagent C (Session balance widget — backend + frontend):**
  11. Add GET /paygate/sessions route — handle_get_sessions handler + list_sessions_for_payer DB query in sessions.rs and db.rs
  13. Update docs/marketplace.html — add session balance widget HTML/CSS/JS with 5-second polling

Wait for all 3 to complete.

**Phase 2 — Integration (depends on Subagent A):**
2. Update serve.rs gateway_handler — add dynamic pricing adjustment in session auth branch after proxy response
12. Wire GET /paygate/sessions route in serve.rs
5. Run `cargo check` — fix all errors
6. **Codex quality + security review (iteration 1 — gateway):** Run:
   ```bash
   codex exec "Review the changes on this branch. Run git diff main to see the diff. Focus on: 1) Dynamic pricing arithmetic — floating point precision, overflow on large token counts, rounding behavior, 2) Can a malicious upstream spoof X-Token-Count to drain session balance?, 3) Race condition between initial deduction and dynamic adjustment, 4) Config validation — what if base_cost + spread = 0? Negative values?, 5) 402 response for dynamic endpoints — does it leak pricing internals? For each finding: severity (critical/high/medium/low), file:line, and recommended fix. Be adversarial." -s read-only -c 'model_reasoning_effort="xhigh"' 2>&1 | tee /tmp/codex-dynamic-review-1.txt
   ```
   Fix any critical or high severity findings before proceeding.
**Phase 3 — Tests (parallel subagents):**
Launch 2 subagents simultaneously:
- **Subagent D**: Gateway tests 1-3 (dynamic pricing adjusts cost downward, adjusts cost upward, no X-Token-Count header falls back to static)
- **Subagent E**: Tests 4-5 (dynamic pricing disabled = no adjustment, compute_cost() unit test) + demo server header tests (summarize includes X-Token-Count, search includes X-Token-Count)

After both complete, merge test code and run:
15. Run full test suite: `cargo test` and verify demo server builds
16. **Codex quality + security review (iteration 2 — full diff):** Run:
    ```bash
    codex exec "Review the changes on this branch. Run git diff main to see the diff. This is a FINAL pass covering gateway + demo + marketplace widget. Focus on: 1) XSS in session balance widget (is balance data sanitized before DOM insertion?), 2) Polling security (can the GET /paygate/sessions endpoint leak other users' session data?), 3) Test coverage gaps, 4) Demo server X-Token-Count — could it be manipulated by upstream response?, 5) Any issues across the full diff. For each finding: severity, file:line, recommended fix." -s read-only -c 'model_reasoning_effort="xhigh"' 2>&1 | tee /tmp/codex-dynamic-review-2.txt
    ```
    Fix any critical or high severity findings.
17. Commit with descriptive message mentioning "dynamic pricing", "X-Token-Count", "session balance widget", and "Codex-reviewed"
