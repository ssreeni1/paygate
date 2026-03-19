Read worktree-briefs/pane1-verifier.md in full — it contains your complete build brief with every function signature, integration point, error handling requirement, and test spec.

Before writing any code, read these files:
- SPEC.md (full spec, focus on §3.2, §4.1, §4.2, §10, §11, §12)
- docs/designs/error-rescue-registry.md (every error type and rescue action)
- docs/designs/failure-modes.md (every failure scenario)
- crates/paygate-common/src/lib.rs, types.rs, hash.rs, mpp.rs (shared types you MUST use)
- crates/paygate-gateway/src/config.rs, db.rs, server.rs, metrics.rs (existing foundation)
- tests/fixtures/request_hash_vectors.json (test vectors)

You are on branch feat/verifier in a git worktree at ~/projects/paygate-wt-verifier.

Follow the brief exactly. Build all 5 files:
1. verifier.rs — payment verification pipeline (the core)
2. mpp.rs — 402 response generation + quote management
3. rate_limit.rs — global + per-payer + IP rate limiting
4. proxy.rs — reverse proxy + header sanitization + X-Payment-Cost
5. webhook.rs — fire-and-forget payment notifications

Then update main.rs to wire these as tower middleware in the correct order per SPEC §3.2.

Write all 15 tests listed in the brief. Make sure `cargo check` and `cargo test` pass before committing. Commit your work with a descriptive message when done.
