# Prompt: Split main.rs

Read the brief at `worktree-briefs/pane-split-main-brief.md`.

Then read `crates/paygate-gateway/src/main.rs` in full.

## Task

Split `crates/paygate-gateway/src/main.rs` (2785 LOC) into 4 files as described in the brief:

1. `main.rs` (~100 LOC) — mod declarations, Cli struct, Commands enum, main() dispatch
2. `serve.rs` (~500 LOC) — cmd_serve, gateway_handler, check_rpc_connectivity
3. `cli.rs` (~1200 LOC) — all CLI subcommands, register command + helpers, RLP encoding
4. `helpers.rs` (~200 LOC) — load_config_or_exit, open_db_reader, truncate_address, truncate_id, format_number, format_file_size, html_escape

Move tests with their code. Add `pub(crate)` visibility where needed for cross-module calls. Do NOT change any function signatures or logic.

## Steps

1. Read the brief thoroughly
2. Read main.rs in full to understand all dependencies between functions
3. Create `serve.rs` with the serve functions + their tests + necessary imports
4. Create `cli.rs` with all CLI functions + their tests + necessary imports
5. Create `helpers.rs` with utility functions + necessary imports
6. Rewrite `main.rs` to be the thin dispatch layer with mod declarations and use statements
7. Run `cargo check` — fix any compile errors (missing imports, visibility)
8. Run `cargo test` — verify all tests still pass with the same count
9. Commit with message: "refactor: split main.rs into serve.rs, cli.rs, helpers.rs"

## Constraints

- Zero behavioral changes — pure mechanical refactor
- Every function signature stays identical
- `cargo check` with zero new errors/warnings
- `cargo test` with zero failures and same test count
- Do not add or remove any functionality
