mod admin;
mod config;
mod db;
mod metrics;
mod mpp;
mod proxy;
mod rate_limit;
mod server;
mod verifier;
mod webhook;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, Request, StatusCode};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum::Router;
use clap::{Parser, Subcommand};
use config::{Config, ConfigError, parse_price_to_base_units};
use paygate_common::types::{format_amount, format_usd, VerificationResult, TOKEN_DECIMALS};
use proxy::ProxyError;
use server::AppState;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

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

    // Set up rate limiter (from feat/verifier)
    let rate_limiter = Arc::new(rate_limit::RateLimiter::new(
        config.rate_limiting.requests_per_second,
        config.rate_limiting.per_payer_per_second,
    ));

    // Set up webhook sender (from feat/verifier)
    let webhook_sender = if !config.webhooks.payment_verified_url.is_empty() {
        Some(webhook::WebhookSender::new(
            http_client.clone(),
            config.webhooks.payment_verified_url.clone(),
            config.webhooks.timeout_seconds,
        ))
    } else {
        None
    };

    // Set up Prometheus metrics exporter
    let prometheus_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install Prometheus recorder");

    let retention = config.storage.request_log_retention_days;

    // Create AppState with all fields from both branches
    let state = AppState {
        config: Arc::new(arc_swap::ArcSwap::from_pointee(config.clone())),
        db_reader: db_reader.clone(),
        db_writer,
        http_client: http_client.clone(),
        rate_limiter,
        webhook_sender,
        prometheus_handle,
        started_at: std::time::Instant::now(),
    };

    // Build admin router
    let admin_app = admin::admin_router(state.clone());

    // Build main gateway router with verifier's gateway_handler + rate limiter middleware
    let gateway_app = Router::new()
        .merge(admin::receipt_route())
        .fallback(gateway_handler)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit::rate_limit_middleware,
        ))
        .with_state(state.clone());

    // Spawn cleanup task
    let cleanup_reader = db_reader.clone();
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

    // Serve both gateway and admin
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

// ─── gateway handler (from feat/verifier) ────────────────────────────────────

async fn gateway_handler(State(state): State<AppState>, req: Request<Body>) -> Response {
    let method = req.method().to_string();
    let uri = req.uri().clone();
    let path = uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| uri.path().to_string());
    let endpoint = format!("{method} {}", uri.path());

    let config = state.current_config();
    let price = config.price_for_endpoint(&endpoint);

    let (parts, body) = req.into_parts();
    let body_bytes = match axum::body::to_bytes(body, config.security.max_request_body_bytes).await
    {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({"error": "request body too large"})),
            )
                .into_response();
        }
    };

    // Free endpoint: skip payment
    if price == 0 {
        let req = Request::from_parts(parts, Body::from(body_bytes));
        return match proxy::forward_request(&state, req, "", 0, &endpoint).await {
            Ok(resp) => resp,
            Err(ProxyError::Timeout) => (
                StatusCode::GATEWAY_TIMEOUT,
                Json(json!({"error": "upstream timeout"})),
            )
                .into_response(),
            Err(ProxyError::PayloadTooLarge) => (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": "response too large"})),
            )
                .into_response(),
            Err(e) => (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": format!("upstream error: {e}")})),
            )
                .into_response(),
        };
    }

    // Check for payment headers
    if !mpp::has_payment_headers(&parts.headers) {
        return mpp::payment_required_response(&state, &endpoint).await;
    }

    let payment = match mpp::extract_payment_headers(&parts.headers) {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "missing or invalid payment headers"})),
            )
                .into_response();
        }
    };

    let request_hash = paygate_common::hash::request_hash(&method, &path, &body_bytes);

    let result = verifier::verify_payment(
        &state,
        &payment.tx_hash,
        &payment.payer_address,
        payment.quote_id.as_deref(),
        &endpoint,
        &request_hash,
    )
    .await;

    match result {
        VerificationResult::Valid(proof) => {
            if let Some(ref wh) = state.webhook_sender {
                wh.notify_payment_verified(
                    &payment.tx_hash,
                    &payment.payer_address,
                    proof.amount,
                    &endpoint,
                );
            }

            let req = Request::from_parts(parts, Body::from(body_bytes));
            match proxy::forward_request(&state, req, &payment.tx_hash, proof.amount, &endpoint)
                .await
            {
                Ok(resp) => {
                    let status_code = resp.status().as_u16() as i32;
                    let _ = state
                        .db_writer
                        .log_request(
                            Some(payment.tx_hash),
                            None,
                            endpoint,
                            payment.payer_address,
                            proof.amount,
                            Some(status_code),
                            None,
                        )
                        .await;
                    resp
                }
                Err(ProxyError::Timeout) => (
                    StatusCode::GATEWAY_TIMEOUT,
                    Json(json!({"error": "upstream timeout"})),
                )
                    .into_response(),
                Err(ProxyError::PayloadTooLarge) => (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({"error": "response too large"})),
                )
                    .into_response(),
                Err(e) => (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({"error": format!("upstream error: {e}")})),
                )
                    .into_response(),
            }
        }
        VerificationResult::TxNotFound => {
            let mut resp = (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "tx_not_found",
                    "message": "Transaction not yet indexed, retry shortly"
                })),
            )
                .into_response();
            resp.headers_mut()
                .insert("Retry-After", HeaderValue::from_static("1"));
            resp
        }
        VerificationResult::RpcError(_) => {
            let mut resp = (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "service_unavailable",
                    "message": "Payment verification temporarily unavailable"
                })),
            )
                .into_response();
            resp.headers_mut()
                .insert("Retry-After", HeaderValue::from_static("2"));
            resp
        }
        VerificationResult::ReplayDetected => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "replay_detected",
                "message": "Transaction already used"
            })),
        )
            .into_response(),
        VerificationResult::PayerMismatch { .. } => (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "payer_mismatch",
                "message": "Payer address does not match on-chain sender"
            })),
        )
            .into_response(),
        VerificationResult::InsufficientAmount { expected, actual } => {
            let mut resp = mpp::payment_required_response(&state, &endpoint).await;
            let shortfall = expected.saturating_sub(actual);
            if let Ok(v) = HeaderValue::from_str(&shortfall.to_string()) {
                resp.headers_mut()
                    .insert(paygate_common::mpp::HEADER_PAYMENT_SHORTFALL, v);
            }
            resp
        }
        VerificationResult::ExpiredTransaction => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "expired_transaction",
                "message": "Transaction too old"
            })),
        )
            .into_response(),
        VerificationResult::MemoMismatch { .. } => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "memo_mismatch",
                "message": "Memo verification failed"
            })),
        )
            .into_response(),
        VerificationResult::InvalidTransfer(msg) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid_transfer",
                "message": msg
            })),
        )
            .into_response(),
        VerificationResult::AmbiguousTransfer => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "ambiguous_transfer",
                "message": "Ambiguous transaction: multiple matching Transfer events"
            })),
        )
            .into_response(),
        VerificationResult::QuoteExpired => {
            mpp::payment_required_response(&state, &endpoint).await
        }
    }
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

    // Step 0: Start a real echo server on a random port
    let echo_app = Router::new().fallback(|| async { "echo ok" });
    let echo_listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("  error: failed to start echo server: {e}");
            std::process::exit(1);
        }
    };
    let echo_addr = echo_listener.local_addr().unwrap();
    tokio::spawn(axum::serve(echo_listener, echo_app).into_future());
    eprintln!("  Started echo server on {echo_addr}");

    // Step 0b: Start the gateway pointing at the echo server
    let db_path = format!("/tmp/paygate_test_{}.db", uuid::Uuid::new_v4());
    let (db_reader, db_writer) = match db::init_db(&db_path) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("  error: failed to init test DB: {e}");
            std::process::exit(1);
        }
    };

    let prometheus_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .build_recorder()
        .handle();

    let test_config = config::Config {
        gateway: config::GatewayConfig {
            listen: "127.0.0.1:0".into(),
            admin_listen: "127.0.0.1:0".into(),
            upstream: format!("http://{echo_addr}"),
            upstream_timeout_seconds: 5,
            max_response_body_bytes: 10_485_760,
        },
        tempo: config::TempoConfig {
            network: "testnet".into(),
            rpc_urls: vec!["http://127.0.0.1:1".into()],
            failover_timeout_ms: 2000,
            rpc_pool_max_idle: 10,
            rpc_timeout_ms: 5000,
            chain_id: 0,
            private_key_env: "PAYGATE_TEST_KEY".into(),
            accepted_token: "0x0000000000000000000000000000000000000001".into(),
        },
        provider: config::ProviderConfig {
            address: "0x7F3a000000000000000000000000000000000001".into(),
            name: "Test".into(),
            description: String::new(),
        },
        sponsorship: Default::default(),
        sessions: Default::default(),
        pricing: config::PricingConfig {
            default_price: "0.001".into(),
            quote_ttl_seconds: 300,
            endpoints: {
                let mut m = std::collections::HashMap::new();
                m.insert("GET /v1/models".into(), "0.000".into());
                m
            },
            dynamic: Default::default(),
            tiers: Default::default(),
        },
        rate_limiting: Default::default(),
        security: Default::default(),
        webhooks: Default::default(),
        storage: Default::default(),
    };

    let state = server::AppState {
        config: Arc::new(arc_swap::ArcSwap::new(Arc::new(test_config))),
        db_reader,
        db_writer,
        http_client: reqwest::Client::new(),
        rate_limiter: Arc::new(rate_limit::RateLimiter::new(1000, 100)),
        webhook_sender: None,
        prometheus_handle,
        started_at: std::time::Instant::now(),
    };

    let gw_app = Router::new()
        .fallback(gateway_handler)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit::rate_limit_middleware,
        ))
        .with_state(state);

    let gw_listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("  error: failed to start gateway: {e}");
            std::process::exit(1);
        }
    };
    let gw_addr = gw_listener.local_addr().unwrap();
    tokio::spawn(axum::serve(gw_listener, gw_app).into_future());
    eprintln!("  Started gateway on {gw_addr} \u{2192} {echo_addr}");
    eprintln!();

    let client = reqwest::Client::new();
    let mut passed = 0u32;
    let mut failed = 0u32;

    // Step 1: Request to a paid endpoint without payment headers -> 402
    {
        let resp = client
            .post(format!("http://{gw_addr}/v1/chat/completions"))
            .body("{}")
            .send()
            .await;
        match resp {
            Ok(r) if r.status().as_u16() == 402 => {
                // Verify 402 response format
                let headers = r.headers();
                let has_required = headers.get("X-Payment-Required").is_some();
                let has_amount = headers.get("X-Payment-Amount").is_some();
                let has_recipient = headers.get("X-Payment-Recipient").is_some();
                let has_methods = headers.get("X-Payment-Methods").is_some();
                let has_quote_id = headers.get("X-Payment-Quote-Id").is_some();

                let body: serde_json::Value = r.json().await.unwrap_or_default();
                let has_error = body["error"] == "payment_required";
                let has_pricing = body["pricing"].is_object();
                let has_help_url = body["help_url"].is_string();

                if has_required && has_amount && has_recipient && has_methods
                    && has_quote_id && has_error && has_pricing && has_help_url
                {
                    eprintln!("  [1/6] Request without payment     402 \u{2713}  (format validated)");
                    passed += 1;
                } else {
                    eprintln!("  [1/6] Request without payment     402 FAIL (missing headers/fields)");
                    if !has_required { eprintln!("    missing: X-Payment-Required"); }
                    if !has_amount { eprintln!("    missing: X-Payment-Amount"); }
                    if !has_recipient { eprintln!("    missing: X-Payment-Recipient"); }
                    if !has_methods { eprintln!("    missing: X-Payment-Methods"); }
                    if !has_quote_id { eprintln!("    missing: X-Payment-Quote-Id"); }
                    if !has_error { eprintln!("    missing: error=payment_required in body"); }
                    if !has_pricing { eprintln!("    missing: pricing object in body"); }
                    if !has_help_url { eprintln!("    missing: help_url in body"); }
                    failed += 1;
                }
            }
            Ok(r) => {
                eprintln!("  [1/6] Request without payment     {} FAIL (expected 402)", r.status());
                failed += 1;
            }
            Err(e) => {
                eprintln!("  [1/6] Request without payment     FAIL ({e})");
                failed += 1;
            }
        }
    }

    // Steps 2-6: Need testnet key for on-chain payment interactions
    if !has_key {
        eprintln!("  [2/6] Fund test wallet            -- skipped (no PAYGATE_TEST_KEY)");
        eprintln!("  [3/6] Pay and retry               -- skipped");
        eprintln!("  [4/6] Replay same tx              -- skipped");
        eprintln!("  [5/6] Wrong payer address         -- skipped");
        eprintln!("  [6/6] Insufficient amount         -- skipped");
        eprintln!();
        eprintln!(
            "  {passed} passed, {failed} failed, 5 skipped (set PAYGATE_TEST_KEY for full e2e)."
        );
        eprintln!("    hint: export PAYGATE_TEST_KEY=<your-tempo-testnet-private-key>");
    } else {
        // TODO: Implement actual on-chain payment steps when testnet keys are available
        eprintln!("  [2/6] Fund test wallet            -- skipped (on-chain not yet wired)");
        eprintln!("  [3/6] Pay and retry               -- skipped");
        eprintln!("  [4/6] Replay same tx              -- skipped");
        eprintln!("  [5/6] Wrong payer address         -- skipped");
        eprintln!("  [6/6] Insufficient amount         -- skipped");
        eprintln!();
        eprintln!(
            "  {passed} passed, {failed} failed, 5 skipped (on-chain steps pending implementation)."
        );
    }

    // Cleanup temp DB
    let _ = std::fs::remove_file(&db_path);

    if failed > 0 {
        std::process::exit(1);
    }
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

// Test 13: Free endpoint bypasses payment
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use crate::rate_limit::RateLimiter;
    use std::collections::HashMap;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_free_endpoint_bypasses_payment() {
        // Start a mock upstream
        let upstream_app = Router::new().fallback(|| async { "free endpoint response" });
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(upstream_listener, upstream_app).into_future());

        let db_path = format!("/tmp/paygate_test_{}.db", uuid::Uuid::new_v4());
        let (db_reader, db_writer) = crate::db::init_db(&db_path).unwrap();

        let prometheus_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
            .build_recorder()
            .handle();

        let config = Config {
            gateway: GatewayConfig {
                listen: "127.0.0.1:0".into(),
                admin_listen: "127.0.0.1:0".into(),
                upstream: format!("http://{upstream_addr}"),
                upstream_timeout_seconds: 5,
                max_response_body_bytes: 10_485_760,
            },
            tempo: TempoConfig {
                network: "testnet".into(),
                rpc_urls: vec!["http://localhost:1".into()],
                failover_timeout_ms: 2000,
                rpc_pool_max_idle: 10,
                rpc_timeout_ms: 5000,
                chain_id: 0,
                private_key_env: "PAYGATE_PRIVATE_KEY".into(),
                accepted_token: "0x1234000000000000000000000000000000000001".into(),
            },
            provider: ProviderConfig {
                address: "0x7F3a000000000000000000000000000000000001".into(),
                name: "Test".into(),
                description: String::new(),
            },
            sponsorship: Default::default(),
            sessions: Default::default(),
            pricing: PricingConfig {
                default_price: "0.001".into(),
                quote_ttl_seconds: 300,
                endpoints: {
                    let mut m = HashMap::new();
                    m.insert("GET /v1/models".into(), "0.000".into());
                    m
                },
                dynamic: Default::default(),
                tiers: Default::default(),
            },
            rate_limiting: Default::default(),
            security: Default::default(),
            webhooks: Default::default(),
            storage: Default::default(),
        };

        let state = AppState {
            config: Arc::new(arc_swap::ArcSwap::new(Arc::new(config))),
            db_reader,
            db_writer,
            http_client: reqwest::Client::new(),
            rate_limiter: Arc::new(RateLimiter::new(100, 10)),
            webhook_sender: None,
            prometheus_handle,
            started_at: std::time::Instant::now(),
        };

        let app = Router::new()
            .fallback(gateway_handler)
            .with_state(state);

        // Send request to free endpoint WITHOUT payment headers
        let req = Request::builder()
            .method("GET")
            .uri("/v1/models")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "free endpoint should bypass payment and return 200"
        );

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"free endpoint response");
    }
}
