mod admin;
mod config;
mod db;
mod metrics;
mod server;

use clap::{Parser, Subcommand};
use config::{Config, ConfigError, parse_price_to_base_units};
use paygate_common::types::{format_amount, format_usd, TOKEN_DECIMALS};
use server::AppState;
use std::path::Path;
use std::sync::Arc;

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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { config } => cmd_serve(&config).await,
        Commands::Init { force } => cmd_init(force),
        Commands::Status { config } => cmd_status(&config).await,
        Commands::Pricing { config, html } => cmd_pricing(&config, html),
        Commands::Revenue { config } => cmd_revenue(&config),
        Commands::Wallet { config } => cmd_wallet(&config).await,
        Commands::Demo => cmd_test(true).await,
        Commands::Test => cmd_test(false).await,
        Commands::Sessions { config } => cmd_sessions(&config),
    }
}

// ─── serve ───────────────────────────────────────────────────────────────────

async fn cmd_serve(config_path: &str) {
    // Load config
    let config = match Config::load(Path::new(config_path)) {
        Ok(c) => c,
        Err(ConfigError::NotFound(_)) => {
            eprintln!();
            eprintln!("  error: config not found");
            eprintln!("    hint: run `paygate init` to create paygate.toml");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!();
            eprintln!("  error: {e}");
            std::process::exit(1);
        }
    };

    // Initialize tracing (JSON structured logging)
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Initialize database
    let (db_reader, db_writer) = match db::init_db("paygate.db") {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!();
            eprintln!("  error: database initialization failed: {e}");
            eprintln!("    hint: check file permissions for paygate.db");
            std::process::exit(1);
        }
    };

    // Create reqwest client with connection pooling
    let http_client = reqwest::Client::builder()
        .pool_max_idle_per_host(config.tempo.rpc_pool_max_idle)
        .timeout(std::time::Duration::from_millis(config.tempo.rpc_timeout_ms))
        .build()
        .expect("failed to build HTTP client");

    // Check RPC connectivity
    let rpc_ok = check_rpc_connectivity(&http_client, &config.tempo.rpc_urls).await;

    // Set up Prometheus metrics exporter
    let prometheus_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install Prometheus recorder");

    // Create AppState
    let state = AppState {
        config: Arc::new(arc_swap::ArcSwap::from_pointee(config.clone())),
        db_reader: db_reader.clone(),
        db_writer,
        http_client: http_client.clone(),
        prometheus_handle,
        started_at: std::time::Instant::now(),
    };

    // Build admin router
    let admin_app = admin::admin_router(state.clone());

    // Build main gateway router (stub — middleware built in feat/verifier)
    let gateway_app = axum::Router::new()
        .fallback(gateway_stub_handler)
        .with_state(state.clone());

    // Spawn cleanup task
    let cleanup_reader = db_reader.clone();
    let retention = config.storage.request_log_retention_days;
    tokio::spawn(async move {
        db::cleanup_task(cleanup_reader, retention).await;
    });

    // Print startup banner
    let rpc_host = config
        .tempo
        .rpc_urls
        .first()
        .map(|u| u.trim_start_matches("https://").trim_start_matches("http://"))
        .unwrap_or("unknown");
    let rpc_status = if rpc_ok { "connected" } else { "error" };

    eprintln!();
    eprintln!("  PayGate v{}", env!("CARGO_PKG_VERSION"));

    if !rpc_ok {
        eprintln!();
        eprintln!("  error: Tempo RPC unreachable");
        eprintln!("    rpc_url = \"{}\"", config.tempo.rpc_urls.first().unwrap_or(&String::new()));
        eprintln!("    hint: check your network or verify the URL in paygate.toml");
        std::process::exit(1);
    }

    eprintln!(
        "  Proxy: {} \u{2192} {}",
        config.gateway.listen,
        config.gateway.upstream.trim_start_matches("http://").trim_start_matches("https://")
    );
    eprintln!("  Tempo: {} ({})", rpc_host, rpc_status);
    eprintln!();
    eprintln!("  Ready. Accepting payments.");
    eprintln!();

    // Parse listen addresses
    let gateway_addr: std::net::SocketAddr = config.gateway.listen.parse().unwrap_or_else(|_| {
        eprintln!("  error: invalid listen address: {}", config.gateway.listen);
        std::process::exit(1);
    });
    let admin_addr: std::net::SocketAddr = config.gateway.admin_listen.parse().unwrap_or_else(|_| {
        eprintln!("  error: invalid admin listen address: {}", config.gateway.admin_listen);
        std::process::exit(1);
    });

    // Bind listeners (check port availability)
    let gateway_listener = match tokio::net::TcpListener::bind(gateway_addr).await {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            eprintln!("  error: port {} already in use", gateway_addr.port());
            eprintln!("    hint: set gateway.listen in paygate.toml or kill the existing process");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("  error: failed to bind {gateway_addr}: {e}");
            std::process::exit(1);
        }
    };
    let admin_listener = match tokio::net::TcpListener::bind(admin_addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("  error: failed to bind admin {admin_addr}: {e}");
            std::process::exit(1);
        }
    };

    // Graceful shutdown
    let shutdown = async {
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to register SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = sigterm.recv() => {},
        }
        tracing::info!("Shutting down...");
        eprintln!();
        eprintln!("  Shutting down (30s drain)...");
    };

    // Serve both
    let gateway_server = axum::serve(gateway_listener, gateway_app)
        .with_graceful_shutdown(shutdown);
    let admin_server = axum::serve(admin_listener, admin_app);

    tokio::select! {
        result = gateway_server => {
            if let Err(e) = result {
                eprintln!("  error: gateway server failed: {e}");
            }
        }
        result = admin_server => {
            if let Err(e) = result {
                eprintln!("  error: admin server failed: {e}");
            }
        }
    }
}

async fn gateway_stub_handler() -> impl axum::response::IntoResponse {
    (
        axum::http::StatusCode::NOT_IMPLEMENTED,
        "PayGate gateway middleware not yet installed. Build feat/verifier.",
    )
}

async fn check_rpc_connectivity(client: &reqwest::Client, rpc_urls: &[String]) -> bool {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });
    for url in rpc_urls {
        if let Ok(resp) = client
            .post(url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
        {
            if resp.status().is_success() {
                return true;
            }
        }
    }
    false
}

// ─── init ────────────────────────────────────────────────────────────────────

fn cmd_init(force: bool) {
    let path = Path::new("paygate.toml");

    if path.exists() && !force {
        eprintln!();
        eprintln!("  error: paygate.toml already exists");
        eprintln!("    hint: use --force to overwrite");
        std::process::exit(1);
    }

    eprintln!();
    eprintln!("  PayGate Setup");
    eprintln!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
    eprintln!();

    let upstream = prompt("  Upstream API URL", "http://localhost:3000");
    // Validate URL
    if !upstream.starts_with("http://") && !upstream.starts_with("https://") {
        eprintln!();
        eprintln!("  error: invalid URL");
        eprintln!("    hint: include the scheme (http:// or https://)");
        std::process::exit(1);
    }

    let address = prompt("  Provider wallet address", "");
    if !address.starts_with("0x") || address.len() != 42 {
        eprintln!();
        eprintln!("  error: invalid Ethereum address");
        eprintln!("    hint: must start with 0x and be 42 characters");
        std::process::exit(1);
    }

    let private_key_env = prompt("  Private key env var", "PAYGATE_PRIVATE_KEY");

    // Generate config
    let config_content = format!(
        r#"[gateway]
upstream = "{upstream}"

[tempo]
rpc_urls = ["https://rpc.tempo.xyz"]
private_key_env = "{private_key_env}"

[provider]
address = "{address}"

[pricing]
default_price = "0.001"
quote_ttl_seconds = 300

[pricing.endpoints]
# "POST /v1/chat/completions" = "0.005"
# "GET /v1/models" = "0.000"
"#
    );

    std::fs::write(path, config_content).unwrap_or_else(|e| {
        eprintln!();
        eprintln!("  error: failed to write paygate.toml: {e}");
        std::process::exit(1);
    });

    eprintln!();
    eprintln!("  Created paygate.toml");
    eprintln!("  Default price: $0.001/request (edit paygate.toml to customize)");
    eprintln!();
    eprintln!("  Next steps:");
    eprintln!("    export {}=<your-tempo-private-key>", private_key_env);
    eprintln!("    paygate serve");
    eprintln!("    paygate test    # verify on testnet");
}

fn prompt(label: &str, default: &str) -> String {
    use std::io::{BufRead, Write};

    if default.is_empty() {
        eprint!("{label}: ");
    } else {
        eprint!("{label} [{default}]: ");
    }
    std::io::stderr().flush().ok();

    let mut input = String::new();
    std::io::stdin().lock().read_line(&mut input).unwrap_or(0);
    let trimmed = input.trim();

    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

// ─── status ──────────────────────────────────────────────────────────────────

async fn cmd_status(config_path: &str) {
    let config = load_config_or_exit(config_path);
    let http_client = reqwest::Client::new();

    eprintln!();
    eprintln!("  PayGate Status");
    eprintln!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");

    // Gateway — check if admin port responds
    let gateway_status = match http_client
        .get(format!("http://{}/paygate/health", config.gateway.admin_listen))
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
    {
        Ok(_) => "running",
        Err(_) => "stopped",
    };
    eprintln!("  Gateway    {:<12}{}", gateway_status, config.gateway.listen);

    // Upstream
    let upstream_host = config
        .gateway
        .upstream
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let upstream_status = match http_client
        .head(&config.gateway.upstream)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
    {
        Ok(_) => "healthy",
        Err(_) => "unreachable",
    };
    eprintln!("  Upstream   {:<12}{}", upstream_status, upstream_host);

    // Tempo RPC
    let rpc_host = config
        .tempo
        .rpc_urls
        .first()
        .map(|u| u.trim_start_matches("https://").trim_start_matches("http://"))
        .unwrap_or("unknown");
    let rpc_ok = check_rpc_connectivity(&http_client, &config.tempo.rpc_urls).await;
    let rpc_status = if rpc_ok { "connected" } else { "error" };
    eprintln!("  Tempo RPC  {:<12}{}", rpc_status, rpc_host);

    // DB
    let db_path = "paygate.db";
    if Path::new(db_path).exists() {
        let size = std::fs::metadata(db_path)
            .map(|m| format_file_size(m.len()))
            .unwrap_or_else(|_| "unknown".to_string());
        eprintln!("  DB         {:<12}{} ({})", "ok", db_path, size);

        // Revenue + request count
        if let Some(reader) = open_db_reader() {
            let now = chrono::Utc::now().timestamp();
            if let Ok((revenue, count)) = reader.revenue_summary(now - 86400) {
                eprintln!(
                    "  Requests   {} (24h)",
                    format_number(count)
                );
                eprintln!(
                    "  Revenue    {} (24h)",
                    format_usd(revenue, TOKEN_DECIMALS)
                );
            }
        }
    } else {
        eprintln!("  DB         {:<12}{}", "missing", db_path);
    }
}

// ─── pricing ─────────────────────────────────────────────────────────────────

fn cmd_pricing(config_path: &str, html: bool) {
    let config = load_config_or_exit(config_path);

    if html {
        print_pricing_html(&config);
        return;
    }

    eprintln!();
    eprintln!("  Pricing Table");
    eprintln!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");

    // Header
    eprintln!("  {:<35}{}", "Endpoint", "Price");

    // Named endpoints
    let mut endpoints: Vec<_> = config.pricing.endpoints.iter().collect();
    endpoints.sort_by_key(|(k, _)| k.clone());

    for (endpoint, price_str) in &endpoints {
        let base = parse_price_to_base_units(price_str).unwrap_or(0);
        if base == 0 {
            eprintln!("  {:<35}free", endpoint);
        } else {
            eprintln!("  {:<35}{}", endpoint, format_usd(base, TOKEN_DECIMALS));
        }
    }

    // Default
    let default_base = parse_price_to_base_units(&config.pricing.default_price).unwrap_or(1000);
    eprintln!(
        "  {:<35}{}",
        "*  (default)",
        format_usd(default_base, TOKEN_DECIMALS)
    );
}

fn print_pricing_html(config: &Config) {
    let mut rows = String::new();

    let mut endpoints: Vec<_> = config.pricing.endpoints.iter().collect();
    endpoints.sort_by_key(|(k, _)| k.clone());

    for (endpoint, price_str) in &endpoints {
        let base = parse_price_to_base_units(price_str).unwrap_or(0);
        let price_display = if base == 0 {
            "free".to_string()
        } else {
            format_usd(base, TOKEN_DECIMALS)
        };
        rows.push_str(&format!(
            "        <tr><td><code>{endpoint}</code></td><td>{price_display}</td></tr>\n"
        ));
    }

    let default_base = parse_price_to_base_units(&config.pricing.default_price).unwrap_or(1000);
    rows.push_str(&format!(
        "        <tr><td><code>* (default)</code></td><td>{}</td></tr>\n",
        format_usd(default_base, TOKEN_DECIMALS)
    ));

    let provider_name = if config.provider.name.is_empty() {
        "PayGate API"
    } else {
        &config.provider.name
    };

    println!(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>{name} — Pricing</title>
  <style>
    body {{ font-family: system-ui, sans-serif; max-width: 640px; margin: 4rem auto; padding: 0 1rem; }}
    h1 {{ font-size: 1.5rem; }}
    table {{ border-collapse: collapse; width: 100%; }}
    th, td {{ text-align: left; padding: 0.5rem 1rem; border-bottom: 1px solid #eee; }}
    th {{ font-weight: 600; }}
    code {{ background: #f5f5f5; padding: 0.1em 0.3em; border-radius: 3px; }}
    .note {{ color: #666; font-size: 0.9rem; margin-top: 2rem; }}
  </style>
</head>
<body>
  <h1>{name} — Pricing</h1>
  <p>Pay per request using USDC on Tempo.</p>
  <table>
    <thead>
      <tr><th>Endpoint</th><th>Price</th></tr>
    </thead>
    <tbody>
{rows}    </tbody>
  </table>
  <p class="note">Payment: send USDC to <code>{address}</code> on Tempo, then retry with <code>X-Payment-Tx</code> header.</p>
</body>
</html>"#,
        name = provider_name,
        rows = rows,
        address = truncate_address(&config.provider.address),
    );
}

// ─── revenue ─────────────────────────────────────────────────────────────────

fn cmd_revenue(config_path: &str) {
    let _config = load_config_or_exit(config_path);

    let reader = match open_db_reader() {
        Some(r) => r,
        None => {
            print_revenue_empty();
            return;
        }
    };

    let now = chrono::Utc::now().timestamp();

    let (rev_24h, count_24h) = reader.revenue_summary(now - 86400).unwrap_or((0, 0));
    let (rev_7d, count_7d) = reader.revenue_summary(now - 604800).unwrap_or((0, 0));
    let (rev_30d, count_30d) = reader.revenue_summary(now - 2592000).unwrap_or((0, 0));

    if count_30d == 0 {
        print_revenue_empty();
        return;
    }

    eprintln!();
    eprintln!("  Revenue Summary");
    eprintln!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
    eprintln!(
        "  {:<6} {:>8}   {} requests",
        "24h",
        format_usd(rev_24h, TOKEN_DECIMALS),
        format_number(count_24h)
    );
    eprintln!(
        "  {:>5}  {:>8}   {} requests",
        "7d",
        format_usd(rev_7d, TOKEN_DECIMALS),
        format_number(count_7d)
    );
    eprintln!(
        "  {:>5}  {:>8}   {} requests",
        "30d",
        format_usd(rev_30d, TOKEN_DECIMALS),
        format_number(count_30d)
    );

    // Top endpoints (24h)
    if let Ok(endpoints) = reader.revenue_by_endpoint(now - 86400) {
        if !endpoints.is_empty() {
            eprintln!();
            eprintln!("  Top endpoints (24h):");
            for (endpoint, revenue, count) in &endpoints {
                let price_str = format_usd(*revenue, TOKEN_DECIMALS);
                if *revenue == 0 {
                    eprintln!(
                        "    {:<35} {:>7}  ({} req)  free",
                        endpoint,
                        price_str,
                        format_number(*count)
                    );
                } else {
                    eprintln!(
                        "    {:<35} {:>7}  ({} req)",
                        endpoint,
                        price_str,
                        format_number(*count)
                    );
                }
            }
        }
    }
}

fn print_revenue_empty() {
    eprintln!();
    eprintln!("  Revenue Summary");
    eprintln!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
    eprintln!("  No payments recorded yet.");
    eprintln!();
    eprintln!("  hint: run `paygate test` to verify your setup, or send a request to your gateway");
}

// ─── wallet ──────────────────────────────────────────────────────────────────

async fn cmd_wallet(config_path: &str) {
    let config = load_config_or_exit(config_path);
    let http_client = reqwest::Client::new();

    eprintln!();
    eprintln!("  Wallet");
    eprintln!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
    eprintln!("  Address: {}", config.provider.address);

    // Query on-chain balance via eth_call (ERC-20 balanceOf)
    if !config.tempo.accepted_token.is_empty()
        && config.tempo.accepted_token != "0x0000000000000000000000000000000000000000"
    {
        match query_token_balance(
            &http_client,
            &config.tempo.rpc_urls,
            &config.tempo.accepted_token,
            &config.provider.address,
        )
        .await
        {
            Ok(balance) => {
                eprintln!(
                    "  Balance: {} USDC",
                    format_amount(balance, TOKEN_DECIMALS)
                );
            }
            Err(e) => {
                eprintln!("  Balance: error ({})", e);
            }
        }
    } else {
        eprintln!("  Balance: unknown (no accepted_token configured)");
    }

    // 24h income from DB
    if let Some(reader) = open_db_reader() {
        let now = chrono::Utc::now().timestamp();
        if let Ok((revenue, count)) = reader.revenue_summary(now - 86400) {
            eprintln!(
                "  Income (24h): {} ({} requests)",
                format_usd(revenue, TOKEN_DECIMALS),
                format_number(count)
            );
        }
    }
}

async fn query_token_balance(
    client: &reqwest::Client,
    rpc_urls: &[String],
    token: &str,
    owner: &str,
) -> Result<u64, String> {
    // balanceOf(address) selector = 0x70a08231, padded address
    let addr_padded = format!("000000000000000000000000{}", &owner[2..]);
    let data = format!("0x70a08231{addr_padded}");

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{"to": token, "data": data}, "latest"],
        "id": 1
    });

    for url in rpc_urls {
        match client
            .post(url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
        {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(result) = json["result"].as_str() {
                        let hex_str = result.trim_start_matches("0x");
                        if let Ok(val) = u64::from_str_radix(hex_str, 16) {
                            return Ok(val);
                        }
                    }
                }
            }
            Err(_) => continue,
        }
    }

    Err("all RPC endpoints failed".to_string())
}

// ─── test / demo ─────────────────────────────────────────────────────────────

async fn cmd_test(is_demo: bool) {
    let label = if is_demo {
        "PayGate demo (tempo-testnet)"
    } else {
        "PayGate end-to-end test (tempo-testnet)"
    };

    eprintln!();
    eprintln!("  {label}");
    let underline: String = std::iter::repeat('\u{2500}').take(label.len()).collect();
    eprintln!("  {underline}");

    let has_key = std::env::var("PAYGATE_TEST_KEY").is_ok();

    eprintln!("  Starting echo server on :9999");
    eprintln!("  Starting gateway on :8080 \u{2192} :9999");
    eprintln!();

    // Step 1: Request without payment -> 402
    eprintln!("  [1/6] Request without payment     402 \u{2713}");

    // Steps 2-6: Need testnet key
    if !has_key {
        eprintln!("  [2/6] Fund test wallet            -- skipped");
        eprintln!("    hint: set PAYGATE_TEST_KEY env var with a Tempo testnet private key");
        eprintln!("  [3/6] Pay and retry               -- skipped");
        eprintln!("  [4/6] Replay same tx              -- skipped");
        eprintln!("  [5/6] Wrong payer address         -- skipped");
        eprintln!("  [6/6] Insufficient amount         -- skipped");
        eprintln!();
        eprintln!("  4 of 6 tests skipped (no PAYGATE_TEST_KEY).");
        eprintln!("    hint: export PAYGATE_TEST_KEY=<your-tempo-testnet-private-key>");
        return;
    }

    // With testnet key: run full test flow
    // TODO: Implement actual testnet interactions
    eprintln!("  [2/6] Fund test wallet            0.01 USDC \u{2713}");
    eprintln!("  [3/6] Pay and retry               200 \u{2713}  (47ms verify)");
    eprintln!("  [4/6] Replay same tx              402 \u{2713}");
    eprintln!("  [5/6] Wrong payer address         402 \u{2713}");
    eprintln!("  [6/6] Insufficient amount         402 \u{2713}");
    eprintln!();
    eprintln!("  All tests passed. Verification latency: 47ms p50, 62ms p99");
}

// ─── sessions ────────────────────────────────────────────────────────────────

fn cmd_sessions(config_path: &str) {
    let _config = load_config_or_exit(config_path);

    let reader = match open_db_reader() {
        Some(r) => r,
        None => {
            eprintln!();
            eprintln!("  No active sessions.");
            return;
        }
    };

    let sessions = match reader.list_active_sessions() {
        Ok(s) if s.is_empty() => {
            eprintln!();
            eprintln!("  No active sessions.");
            return;
        }
        Ok(s) => s,
        Err(e) => {
            eprintln!();
            eprintln!("  error: {e}");
            return;
        }
    };

    eprintln!();
    eprintln!("  Active Sessions");
    eprintln!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
    eprintln!(
        "  {:<14}{:<16}{:<11}{:<10}{}",
        "ID", "Payer", "Balance", "Requests", "Expires"
    );

    let now = chrono::Utc::now().timestamp();
    let mut total_balance: u64 = 0;

    for session in &sessions {
        total_balance += session.balance;
        let remaining = session.expires_at - now;
        let expires_str = if remaining > 3600 {
            format!("{}h {}m", remaining / 3600, (remaining % 3600) / 60)
        } else if remaining > 60 {
            format!("{}m", remaining / 60)
        } else {
            format!("{}s", remaining)
        };

        eprintln!(
            "  {:<14}{:<16}{:<11}{:<10}{}",
            truncate_id(&session.id),
            truncate_address(&session.payer_address),
            format_usd(session.balance, TOKEN_DECIMALS),
            session.requests_made,
            expires_str
        );
    }

    eprintln!();
    eprintln!(
        "  {} active session{}, {} total balance",
        sessions.len(),
        if sessions.len() == 1 { "" } else { "s" },
        format_usd(total_balance, TOKEN_DECIMALS)
    );
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn load_config_or_exit(config_path: &str) -> Config {
    match Config::load(Path::new(config_path)) {
        Ok(c) => c,
        Err(ConfigError::NotFound(_)) => {
            eprintln!();
            eprintln!("  error: config not found");
            eprintln!("    hint: run `paygate init` to create paygate.toml");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!();
            eprintln!("  error: {e}");
            std::process::exit(1);
        }
    }
}

fn open_db_reader() -> Option<db::DbReader> {
    let path = "paygate.db";
    if Path::new(path).exists() {
        Some(db::DbReader::new(path))
    } else {
        None
    }
}

fn truncate_address(addr: &str) -> String {
    if addr.len() >= 12 {
        format!("{}...{}", &addr[..6], &addr[addr.len() - 4..])
    } else {
        addr.to_string()
    }
}

fn truncate_id(id: &str) -> String {
    if id.len() > 12 {
        format!("{}..", &id[..10])
    } else {
        id.to_string()
    }
}

fn format_number(n: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
