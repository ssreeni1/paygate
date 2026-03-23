# Brief: Split main.rs into 4 Files

## Goal

Decompose `crates/paygate-gateway/src/main.rs` (2785 LOC) into 4 files. This is a **pure refactor** — zero behavioral changes. Every function signature stays identical. Tests move with their code.

## Target Structure

```
crates/paygate-gateway/src/
  main.rs     (~100 LOC)  — mod declarations, Cli/Commands structs, main() dispatch
  serve.rs    (~500 LOC)  — cmd_serve(), gateway_handler(), check_rpc_connectivity()
  cli.rs      (~1200 LOC) — all CLI subcommands + register helpers
  helpers.rs  (~200 LOC)  — shared utility functions
```

## What Goes Where

### main.rs (~100 LOC)

Keep:
- All existing `mod` declarations: admin, config, db, metrics, mpp, proxy, rate_limit, server, sponsor, verifier, webhook
- Add new `mod` declarations: `mod serve; mod cli; mod helpers;`
- The `Cli` struct (with `#[derive(Parser)]`) and `Commands` enum (lines 30-106)
- `#[tokio::main] async fn main()` dispatch (lines 108-126)
- Necessary `use` statements for clap, the Cli struct, and dispatching to serve/cli/helpers

The `main()` function calls `serve::cmd_serve`, `cli::cmd_init`, `cli::cmd_status`, etc.

### serve.rs (~500 LOC)

Move these functions verbatim:
- `cmd_serve()` (lines 130-381)
- `gateway_handler()` (lines 385-630)
- `check_rpc_connectivity()` (lines 632-653)

Add `use` imports at the top for everything these functions need (axum, config, db, metrics, mpp, proxy, rate_limit, server, sponsor, verifier, webhook, etc.).

Visibility:
- `pub(crate) async fn cmd_serve(config_path: &str)`
- `pub(crate) async fn gateway_handler(...)` — needed by cli.rs for `cmd_test`
- `pub(crate) async fn check_rpc_connectivity(...)` — needed by cli.rs for `cmd_status`

### cli.rs (~1200 LOC)

Move these functions verbatim:
- `cmd_init()` (lines 657-727)
- `prompt()` (lines 729-748)
- `cmd_status()` (lines 752-825)
- `cmd_pricing()` + `print_pricing_html()` (lines 829-941)
- `cmd_revenue()` + `print_revenue_empty()` (lines 945-1023)
- `cmd_wallet()` + `query_token_balance()` (lines 1027-1115)
- `cmd_test()` (lines 1119-1319)
- `cmd_sessions()` (lines 1323-1388)
- `cmd_register()` (lines 1406-1561) + the `alloy_sol_types::sol!` macro block (lines 1392-1404)
- All register RPC helpers: `rpc_get_nonce`, `rpc_gas_price`, `rpc_send_raw_tx`, `rpc_wait_for_receipt`, `decode_service_registered` (lines 1563-1689)
- `sign_legacy_tx()` + `trim_leading_zeros()` (lines 1691-1747)
- All RLP encoding helpers: `rlp_encode_u64`, `rlp_encode_u256`, `rlp_encode_bytes`, `rlp_encode_list` (lines 1750-1809)

Visibility: all `pub(crate)`.

cli.rs needs to reference `serve::gateway_handler` and `serve::check_rpc_connectivity` (used in `cmd_test` and `cmd_status`). Import them with `use crate::serve::{gateway_handler, check_rpc_connectivity};`.

cli.rs also needs `helpers::{load_config_or_exit, open_db_reader, truncate_address, truncate_id, format_number, format_file_size, html_escape}`. Import from `use crate::helpers::*;`.

### helpers.rs (~200 LOC)

Move these functions verbatim:
- `load_config_or_exit()` (lines 1813-1828)
- `open_db_reader()` (lines 1830-1837)
- `truncate_address()` (lines 1839-1845)
- `truncate_id()` (lines 1847-1853)
- `format_number()` (lines 1855-1868)
- `format_file_size()` (lines 1870-1878)
- `html_escape()` (lines 867-873)
- `prompt()` could go here too if preferred, but it's only used by `cmd_init`, so cli.rs is fine

All `pub(crate)`.

## Test Placement

### Tests that move to serve.rs

These tests use `gateway_handler` directly:
- `test_free_endpoint_bypasses_payment` (line 1890)
- `test_wrong_recipient_returns_error` (line 2313)
- `test_402_flood_rate_limiter` (line 2386)
- The `test_state_with_upstream()` helper (line 1983) — used by serve tests and admin tests

### Tests that move to cli.rs

These test register/ABI functionality defined in cli.rs:
- `test_register_service_abi_encoding` (line 2421)
- `test_service_registered_event_signature` (line 2457)

### Tests that stay in main.rs or move based on what they test

Admin endpoint tests (use `admin_router` but also `test_state_with_upstream` from serve):
- `test_health_endpoint_healthy` (line 2052)
- `test_health_endpoint_degraded` (line 2098)
- `test_metrics_endpoint_prometheus_format` (line 2129)
- `test_receipt_endpoint_found` (line 2171)
- `test_receipt_endpoint_not_found` (line 2218)
- `test_webhook_delivery` (line 2245)
- `test_webhook_failure_does_not_block` (line 2287)
- `test_transactions_endpoint_json` (line 2540)
- `test_transactions_limit_param` (line 2625)
- `test_transactions_cors_headers` (line 2710)
- `insert_test_payment` helper (line 2469)
- `test_recent_transactions_ordered` (line 2484)
- `test_recent_transactions_empty_db` (line 2507)
- `test_transaction_stats_correct` (line 2521)

These admin/webhook/db tests can either stay in a `tests` module in serve.rs (since they use `test_state_with_upstream`) or be moved to their respective modules. The simplest approach: keep them all in serve.rs alongside `test_state_with_upstream`, since they all depend on that helper.

## Rules

1. Every function keeps its **exact** signature — do not change parameter types, return types, or names
2. Add `pub(crate)` visibility only where needed for cross-module calls
3. `cargo check` must pass with zero new warnings (existing warnings are OK)
4. `cargo test` must pass with zero test failures and the same test count
5. Do NOT add, remove, or modify any production logic — this is a mechanical move
6. Preserve all comments, including the section headers (`// --- serve ---` etc.)
7. The `alloy_sol_types::sol!` macro and its `use` imports for alloy must move to cli.rs together

## Verification

After the split, run:
```bash
cargo check 2>&1 | grep -c "error"   # must be 0
cargo test 2>&1 | tail -5             # all tests pass
```
