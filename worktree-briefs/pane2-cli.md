# Pane 2 Brief: CLI Commands & Admin Server (feat/cli)

## Context
You are building all CLI commands and the admin HTTP server for PayGate — a reverse proxy that gates API access behind micropayments. The CLI is the primary developer interface. It must be polished, professional, and follow exact output specifications.

## Required Reading (do this first)
1. `SPEC.md` — Read in full, but focus on:
   - §9.1 CLI Output Conventions (formatting rules, error style, NO_COLOR)
   - §9.2 CLI Output Specifications (EXACT output for every command — these are the spec, match them precisely)
   - §9.3 Interaction State Coverage (every state: loading, empty, error, success, partial)
   - §9.4 Developer Experience
   - §12.1 Health Check (JSON format)
   - §12.2 Prometheus Metrics (metric names)
   - §12.4 Receipt Verification Endpoint
2. `DESIGN-REVIEW.md` — Read in full. This has the authoritative CLI output mockups with exact formatting.
3. `crates/paygate-gateway/src/` — Read ALL existing files:
   - `config.rs`: `Config::load()`, `Config::validate()`, `parse_price_to_base_units()`, all config structs
   - `db.rs`: `DbReader` (revenue_summary, revenue_by_endpoint, get_payment, active_quote_count), `DbWriter`, `init_db()`, `cleanup_task()`
   - `server.rs`: `AppState` struct
   - `metrics.rs`: All metric functions
4. `crates/paygate-common/src/types.rs` — `format_amount()`, `format_usd()`, `PaymentRecord`
5. `paygate.toml.example` — Reference config
6. `schema.sql` — Database schema

## What to Build

### 1. `crates/paygate-gateway/src/main.rs` — Full CLI with clap

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "paygate", version, about = "Micropayment-gated API gateway")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the gateway proxy
    Serve {
        #[arg(short, long, default_value = "paygate.toml")]
        config: String,
    },
    /// Interactive setup wizard
    Init {
        #[arg(long)]
        force: bool,
    },
    /// Show gateway status
    Status {
        #[arg(short, long, default_value = "paygate.toml")]
        config: String,
    },
    /// Display pricing table
    Pricing {
        #[arg(short, long, default_value = "paygate.toml")]
        config: String,
        #[arg(long)]
        html: bool,
    },
    /// Revenue summary
    Revenue {
        #[arg(short, long, default_value = "paygate.toml")]
        config: String,
    },
    /// Show provider wallet balance
    Wallet {
        #[arg(short, long, default_value = "paygate.toml")]
        config: String,
    },
    /// Run demo with echo server
    Demo,
    /// End-to-end test on testnet
    Test,
    /// List active sessions
    Sessions {
        #[arg(short, long, default_value = "paygate.toml")]
        config: String,
    },
}
```

#### `paygate serve` implementation:
1. Load and validate config
2. Initialize tracing (JSON structured logging to stdout)
3. Call `db::init_db("paygate.db")` to get (reader, writer)
4. Create `reqwest::Client` with connection pooling (pool_max_idle from config)
5. Create `AppState` with ArcSwap config
6. Build axum Router for the main gateway (listen on config.gateway.listen)
   - For now, routes can be stubs that return 501 — the middleware is built in feat/verifier
   - Add a fallback handler that catches all routes
7. Build admin Router (listen on config.gateway.admin_listen) — see admin.rs below
8. Set up Prometheus metrics exporter
9. Spawn `db::cleanup_task(reader.clone(), config.storage.request_log_retention_days)`
10. Set up graceful shutdown on SIGTERM/SIGINT:
    ```rust
    let shutdown = async {
        tokio::signal::ctrl_c().await.ok();
        // OR tokio::signal::unix::signal(SignalKind::terminate())
        info!("Shutting down...");
    };
    ```
11. Print startup banner (match DESIGN-REVIEW.md format EXACTLY):
    ```
      PayGate v0.1.0
      Proxy: 0.0.0.0:8080 → localhost:3000
      Tempo: rpc.tempo.xyz (connected)

      Ready. Accepting payments.
    ```
12. Handle startup errors with `error:` + `hint:` format:
    - Config not found → `error: config not found` + `hint: run 'paygate init' to create paygate.toml`
    - Port in use → `error: port 8080 already in use` + `hint: set gateway.listen in paygate.toml`
    - RPC unreachable → `error: Tempo RPC unreachable` + `hint: check your network`

#### `paygate init` implementation:
1. Check if paygate.toml exists
   - If exists and not --force: show what would change, prompt "Overwrite? [y/N]"
   - If exists and --force: overwrite silently
2. Ask 3 questions (use stdin readline):
   - "Upstream API URL:" (default: http://localhost:3000)
   - "Provider wallet address:" (validate 0x + 42 chars)
   - "Private key env var [PAYGATE_PRIVATE_KEY]:" (default: PAYGATE_PRIVATE_KEY)
3. Generate paygate.toml with defaults for everything else
4. Print success message matching DESIGN-REVIEW.md format exactly

#### `paygate revenue` implementation:
1. Load config, open DB reader
2. Query revenue_summary for 24h, 7d, 30d (timestamps: now - 86400, now - 604800, now - 2592000)
3. Query revenue_by_endpoint for 24h
4. Print in EXACT format from DESIGN-REVIEW.md:
   ```
     Revenue Summary
     ───────────────
     24h     $12.45   2,490 requests
      7d     $67.30  13,460 requests
     30d    $245.10  49,020 requests

     Top endpoints (24h):
       POST /v1/chat/completions   $10.20  (2,040 req)
   ```
5. Empty state: "No payments recorded yet." + hint

#### `paygate status` implementation:
1. Load config, check each component:
   - Gateway: check if listen port is bound
   - Upstream: HTTP HEAD request to upstream URL
   - Tempo RPC: eth_blockNumber call
   - DB: open and query count
2. Print in format from DESIGN-REVIEW.md

#### `paygate pricing` implementation:
1. Load config, print endpoint pricing table
2. With --html: generate standalone HTML page with pricing table, provider info, example curls

#### `paygate wallet` implementation:
1. Load config
2. Query Tempo RPC for token balance: `eth_call` to ERC-20 `balanceOf(provider_address)`
3. Query DB for 24h revenue
4. Print balance + income

#### `paygate test` implementation:
1. Start built-in echo server on :9999
2. Start gateway in test mode pointing at :9999
3. Run 6 test steps with ✓/✗ output (match DESIGN-REVIEW.md exactly):
   ```
   [1/6] Request without payment     402 ✓
   [2/6] Fund test wallet            0.01 USDC ✓
   ...
   ```
4. Steps that need testnet interaction (2, 3) can be stubs that print a message if PAYGATE_TEST_KEY is not set

#### `paygate demo` implementation:
- Same as test but more user-friendly framing
- Can share implementation with test

#### `paygate sessions` implementation:
1. Query sessions table
2. Print table or "No active sessions."

### 2. `crates/paygate-gateway/src/admin.rs` — Admin HTTP Server

```rust
pub fn admin_router(state: AppState) -> axum::Router {
    axum::Router::new()
        .route("/paygate/health", get(health_handler))
        .route("/paygate/metrics", get(metrics_handler))
        .route("/paygate/receipts/:tx_hash", get(receipt_handler))
        .with_state(state)
}
```

#### GET /paygate/health
Return JSON matching SPEC §12.1:
```json
{
  "status": "healthy",
  "tempo_rpc": "connected",
  "upstream": "reachable",
  "active_sessions": 12,
  "db": "ok"
}
```
Check each component. If any is degraded, overall status = "degraded".

#### GET /paygate/metrics
Use `metrics-exporter-prometheus` to export all metrics defined in metrics.rs.

#### GET /paygate/receipts/{tx_hash}
1. Look up payment via `state.db_reader.get_payment(tx_hash)`
2. Return JSON: `{ tx_hash, payer_address, amount, verified_at, status }` or 404
3. Rate limit: 100 requests/min per IP (use a simple in-memory counter or governor)
4. Do NOT include `endpoint` or `request_hash` fields (per eng review — avoids leaking usage patterns)

## CLI Output Rules (from SPEC §9.1 — violating these is a bug)
- 2-space indent for ALL output blocks
- Section headers: text + `───` underline (EXACT character: U+2500 BOX DRAWINGS LIGHT HORIZONTAL)
- Errors: `error: <message>` on one line, `  hint: <fix>` indented on next line
- NO emoji anywhere (except ✓ and ✗ in test output)
- NO color by default. Check `NO_COLOR` env var and `--no-color` flag.
- Monetary amounts: `$X.XX` format
- Wallet addresses: truncated `0x7F3a...Provider` (first 6 + last 8 chars)
- All timestamps: ISO 8601

## Tests to Write
1. Config loading (minimal, full, invalid)
2. Admin health endpoint (healthy + degraded)
3. Admin receipt endpoint (found, not found, rate limited)
4. Revenue calculation from DB
5. Pricing table generation
6. CLI argument parsing

## Commit
Make sure `cargo check` passes. Commit with descriptive message.
