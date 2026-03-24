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

Build order:

Part A (Sponsorship E2E):
1. Update demo/paygate.toml — add [sponsorship] section with enabled = true
2. Create sdk/sponsor-e2e.mjs — standalone E2E test script
3. Test locally if possible, or document how to run against deployed instance

Part B (SDK Auto-Session):
4. Update sdk/src/types.ts — add autoSession, sessionDeposit to PayGateClientOptions, add session response types
5. Update sdk/src/hash.ts — add sessionMemo() and hmacSha256() functions
6. Update sdk/src/client.ts — add session state, createSession(), computeSessionHeaders(), update fetch() with auto-session logic
7. Run `npm run build` (or equivalent) — fix all TypeScript errors
8. Write tests in sdk/tests/client.test.ts — at least 6 tests covering auto-session lifecycle
9. Run `npm test` — all tests must pass

Important: The auto-session feature depends on the sessions endpoints built by Pane 1 (POST /paygate/sessions/nonce and POST /paygate/sessions). Your SDK code calls these endpoints. Make sure the request format matches what the brief specifies:
- Nonce request: POST with X-Payment-Payer header
- Session creation: POST with X-Payment-Tx, X-Payment-Payer headers and JSON body { "nonce": "..." }
- Session auth: X-Payment-Session, X-Payment-Session-Sig (HMAC-SHA256), X-Payment-Timestamp headers

Commit your work with a descriptive message when done. The commit message should mention "SDK auto-session" and "fee sponsorship E2E".
