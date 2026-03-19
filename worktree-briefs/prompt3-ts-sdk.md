Read worktree-briefs/pane3-ts-sdk.md in full — it contains your complete build brief with every type definition, function signature, the CRITICAL hash parity requirement, and test specs.

Before writing any code, read these files:
- SPEC.md (focus on §4.1 402 format, §4.2 payment flow + hash computation, §7.1 SDK example)
- crates/paygate-common/src/hash.rs (the Rust hash implementation — your TypeScript MUST match byte-for-byte)
- crates/paygate-common/src/mpp.rs (header constant names)
- tests/fixtures/request_hash_vectors.json (shared test vectors your tests MUST validate against)

You are on branch feat/ts-sdk in a git worktree at ~/projects/paygate-wt-ts-sdk.

Follow the brief exactly. Build all files in sdk/:
1. package.json — @paygate/sdk with viem, typescript, vitest
2. tsconfig.json
3. src/types.ts — all TypeScript types
4. src/hash.ts — requestHash + paymentMemo (MUST match Rust identically)
5. src/discovery.ts — parse 402 responses
6. src/client.ts — PayGateClient with auto-pay fetch()
7. src/index.ts — public exports
8. tests/hash.test.ts — cross-language hash parity (MOST IMPORTANT TEST)
9. tests/client.test.ts — mock 402→pay→retry flow

The hash parity test is the #1 most critical test in the entire project. If Rust and TypeScript produce different hashes, payments will silently fail.

Run `cd sdk && npm install && npm test` to verify ALL tests pass. Commit your work with a descriptive message when done.
