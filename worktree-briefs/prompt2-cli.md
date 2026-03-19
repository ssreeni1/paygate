Read worktree-briefs/pane2-cli.md in full — it contains your complete build brief with every CLI command spec, exact output formats, admin endpoint specs, and test requirements.

Before writing any code, read these files:
- SPEC.md (full spec, focus on §9.1-9.4 CLI specs, §12.1-12.4 observability)
- DESIGN-REVIEW.md (authoritative CLI output mockups — match these EXACTLY)
- crates/paygate-gateway/src/config.rs, db.rs, server.rs, metrics.rs (existing foundation)
- crates/paygate-common/src/types.rs (format_amount, format_usd helpers)
- paygate.toml.example (reference config)
- schema.sql (database schema)

You are on branch feat/cli in a git worktree at ~/projects/paygate-wt-cli.

Follow the brief exactly. Build:
1. main.rs — full clap CLI with ALL subcommands (serve, init, status, pricing, revenue, wallet, demo, test, sessions)
2. admin.rs — admin HTTP server with /paygate/health, /paygate/metrics, /paygate/receipts/{tx_hash}

CLI output MUST match the mockups in DESIGN-REVIEW.md exactly:
- 2-space indent, ─── underlines, error:+hint: format
- NO_COLOR support, $X.XX monetary format, ✓/✗ markers
- Every command must handle its empty state and error states

For `paygate serve`: set up axum router, graceful shutdown (SIGTERM 30s drain), spawn cleanup task, Prometheus metrics exporter.

Make sure `cargo check` passes before committing. Commit your work with a descriptive message when done.
