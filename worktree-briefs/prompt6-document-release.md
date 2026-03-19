You are running /document-release on the PayGate project — creating launch documentation for v0.1.0.

This is a greenfield project at ~/projects/paygate with no README, CHANGELOG, or CONTRIBUTING docs yet. The codebase is complete for Wave 1 (MVP).

Read these files for context:
- SPEC.md (full specification — sections 1-2 for overview, 9 for CLI, 14 for waves)
- DESIGN-REVIEW.md (CLI output specifications)
- TODOS.md (known blockers and deferred work)
- paygate.toml.example (reference config)
- Cargo.toml (workspace structure)
- sdk/package.json (TS SDK)

Create these documents:

1. **README.md** — The public face of the project:
   - One-line description: "Wrap any API behind per-request stablecoin micropayments on Tempo"
   - The consumer→gateway→chain→upstream ASCII diagram from SPEC §2
   - Quick start (3 commands: install, init, serve)
   - How it works (402 flow explained simply)
   - Configuration reference (link to paygate.toml.example)
   - Client SDK usage (TypeScript example from SPEC §7.1)
   - CLI reference (all commands with one-line descriptions)
   - Architecture overview (components table from SPEC §3.1)
   - Security model (brief summary of §10)
   - Roadmap (Waves 1-4 from §14)
   - License: MIT

2. **CHANGELOG.md** — First release:
   ```
   # Changelog

   ## [0.1.0] - 2026-03-19

   ### Added
   - (list every Wave 1 feature)
   ```

3. **CLAUDE.md** — Instructions for AI assistants working on this codebase:
   - Project overview (what PayGate does)
   - How to build: `cargo build`, `cargo test`, `cd sdk && npm test`
   - Project structure (3 Rust crates + TS SDK + Solidity contracts)
   - Key architectural decisions (tower middleware, SQLite WAL writer task, ArcSwap config)
   - Testing: run command, test directory, shared hash vectors requirement
   - Important: cross-language hash parity between Rust and TypeScript must be maintained
   - Important: all payment verification errors must map to specific HTTP status codes per error-rescue-registry.md

4. **Update TODOS.md** — Mark completed items, add any new items discovered

Do NOT modify any source code. Only create/update documentation files. Commit when done.
