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

Build order:
1. Update schema.sql — add session_nonces table
2. Update config.rs — change discount_percent default to 0, add no_charge_on_5xx vec and helper
3. Update db.rs — add FullSessionRecord, NonceRecord structs, WriteCommand variants, DbReader queries, DbWriter methods
4. Create sessions.rs — handle_nonce, handle_create_session, verify_and_deduct, SessionDeduction, SessionError
5. Update serve.rs — add session route wiring in cmd_serve, add session auth branch in gateway_handler with no_charge_on_5xx
6. Register mod sessions in main.rs or lib.rs
7. Add crate dependencies if needed (hmac, sha2, rand)
8. Run `cargo check` — fix all errors
9. Write all 10 tests in sessions.rs
10. Run `cargo test` — all tests must pass

Commit your work with a descriptive message when done. The commit message should mention "sessions protocol", "HMAC auth", and "no_charge_on_5xx".
