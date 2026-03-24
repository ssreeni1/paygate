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

Build order:

Part A (Dynamic Pricing Gateway):
1. Update config.rs — add compute_cost() to DynamicPricingConfig, add default_header_source, update field names to match spec (base_cost_per_token, spread_per_token)
2. Update serve.rs gateway_handler — add dynamic pricing adjustment in session auth branch after proxy response
3. Update sessions.rs — add deduct_additional() helper
4. Update mpp.rs — add dynamic pricing note to 402 response when dynamic pricing is enabled
5. Run `cargo check` — fix all errors
6. Write 5 tests for dynamic pricing logic
7. Run `cargo test` — all tests must pass

Part B (Demo Server Headers):
8. Update demo/src/routes/summarize.ts — add X-Token-Count response header
9. Update demo/src/routes/search.ts — add X-Token-Count response header
10. Update demo/paygate.toml — add [pricing.dynamic] config section

Part C (Session Balance Widget):
11. Add GET /paygate/sessions route — handle_get_sessions handler + list_sessions_for_payer DB query
12. Wire GET route in serve.rs (note: POST /paygate/sessions already exists for session creation, GET is for querying)
13. Update docs/marketplace.html — add session balance widget HTML/CSS/JS with 5-second polling
14. Test the widget manually if possible

15. Run full test suite: `cargo test` and verify demo server builds
16. Commit with descriptive message mentioning "dynamic pricing", "X-Token-Count", and "session balance widget"
