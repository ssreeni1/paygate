mod admin;
mod config;
mod db;
mod metrics;
mod mpp;
mod proxy;
mod rate_limit;
mod server;
mod sponsor;
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
    /// Register service in on-chain PayGateRegistry
    Register {
        /// Service name
        #[arg(long)]
        name: String,

        /// Price per request in USDC (e.g., "0.001")
        #[arg(long)]
        price: String,

        /// Accepted TIP-20 token address
        #[arg(long, default_value = "0x20c0000000000000000000000000000000000000")]
        token: String,

        /// URL to pricing/metadata JSON
        #[arg(long, default_value = "")]
        metadata_url: String,

        /// PayGateRegistry contract address
        #[arg(long)]
        registry: String,

        /// Config file path
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
        Commands::Register { name, price, token, metadata_url, registry, config } => {
            cmd_register(&name, &price, &token, &metadata_url, &registry, &config).await
        }
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
    let mut gateway_app = Router::new()
        .merge(admin::receipt_route())
        .fallback(gateway_handler)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit::rate_limit_middleware,
        ))
        .with_state(state.clone());

    // Wire fee sponsorship endpoint (if enabled)
    if config.sponsorship.enabled {
        match sponsor::SponsorService::new(
            state.config.clone(),
            state.http_client.clone(),
        ) {
            Ok(sponsor_service) => {
                let sponsor_path = config.sponsorship.sponsor_listen.clone();
                sponsor_service.spawn_balance_checker();
                gateway_app = gateway_app.route(
                    &sponsor_path,
                    axum::routing::post(sponsor::handle_sponsor)
                        .with_state(sponsor_service),
                );
                info!("fee sponsorship enabled at {sponsor_path}");
            }
            Err(e) => {
                eprintln!();
                eprintln!("  error: {e}");
                eprintln!("    hint: export PAYGATE_PRIVATE_KEY=<your-tempo-private-key> or set sponsorship.enabled = false");
                std::process::exit(1);
            }
        }
    }

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
        eprintln!("  warning: Tempo RPC unreachable at startup");
        eprintln!("    rpc_url = \"{}\"", config.tempo.rpc_urls.first().unwrap_or(&String::new()));
        eprintln!("    hint: payment verification will fail until RPC is reachable");
        // Don't exit — start the server anyway. Payments will return 503 but free endpoints work.
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

    // SIGHUP config reload task
    {
        let config_arc = state.config.clone();
        let config_path_owned = config_path.to_string();
        tokio::spawn(async move {
            let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
                .expect("failed to register SIGHUP handler");
            loop {
                sighup.recv().await;
                tracing::info!("SIGHUP received, reloading config from {}", config_path_owned);
                match Config::load(Path::new(&config_path_owned)) {
                    Ok(new_config) => {
                        config_arc.store(Arc::new(new_config));
                        metrics::record_config_reload("success");
                        tracing::info!("Config reloaded successfully");
                    }
                    Err(e) => {
                        metrics::record_config_reload("failure");
                        tracing::error!("Config reload failed: {e}");
                    }
                }
            }
        });
    }

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

    // Extract client IP for rate limiting (from X-Forwarded-For or fallback)
    let client_ip = parts
        .headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .unwrap_or("unknown")
        .trim()
        .to_string();

    // Check for payment headers
    if !mpp::has_payment_headers(&parts.headers) {
        if !state.rate_limiter.check_402_flood(&client_ip) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "message": "Too many payment discovery requests. Please slow down.",
                    "retry_after": 60
                })),
            )
                .into_response();
        }
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
        VerificationResult::RpcError(ref msg) if msg.contains("backpressure") => {
            let mut resp = (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "service_unavailable",
                    "message": "Server under load, please retry shortly"
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

/// Escape a string for safe HTML interpolation, preventing XSS.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
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
            "        <tr><td><code>{}</code></td><td>{}</td></tr>\n",
            html_escape(endpoint),
            html_escape(&price_display),
        ));
    }

    let default_base = parse_price_to_base_units(&config.pricing.default_price).unwrap_or(1000);
    rows.push_str(&format!(
        "        <tr><td><code>* (default)</code></td><td>{}</td></tr>\n",
        html_escape(&format_usd(default_base, TOKEN_DECIMALS))
    ));

    let provider_name_raw = if config.provider.name.is_empty() {
        "PayGate API"
    } else {
        &config.provider.name
    };
    let provider_name = html_escape(provider_name_raw);

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
        address = html_escape(&truncate_address(&config.provider.address)),
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

// ─── register ─────────────────────────────────────────────────────────────────

use alloy_primitives::{Address, FixedBytes, U256, keccak256};
use alloy_sol_types::{SolCall, SolEvent};

alloy_sol_types::sol! {
    function registerService(
        string name,
        uint256 pricePerRequest,
        address acceptedToken,
        string metadataUri
    ) external returns (bytes32 serviceId);

    event ServiceRegistered(bytes32 indexed serviceId, address indexed provider, uint256 price);
}

async fn cmd_register(
    name: &str,
    price: &str,
    token: &str,
    metadata_url: &str,
    registry: &str,
    config_path: &str,
) {
    let config = load_config_or_exit(config_path);

    eprintln!();
    eprintln!("  PayGate Registry");
    eprintln!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
    eprintln!("  Registering service...");
    eprintln!();

    // 1. Parse price to base units
    let price_base_units = parse_price_to_base_units(price).unwrap_or_else(|e| {
        eprintln!("  error: invalid price \"{price}\" \u{2014} {e}");
        std::process::exit(1);
    });

    // 2. Load private key
    let private_key = std::env::var(&config.tempo.private_key_env).unwrap_or_else(|_| {
        eprintln!("  error: {} not set", config.tempo.private_key_env);
        eprintln!(
            "    hint: export {}=<your-tempo-private-key>",
            config.tempo.private_key_env
        );
        std::process::exit(1);
    });

    // 3. Parse addresses
    let token_addr: Address = token.parse().unwrap_or_else(|_| {
        eprintln!("  error: invalid token address: {token}");
        std::process::exit(1);
    });

    let registry_addr: Address = registry.parse().unwrap_or_else(|_| {
        eprintln!("  error: invalid registry address: {registry}");
        eprintln!("    hint: pass --registry <address>");
        std::process::exit(1);
    });

    // 4. Derive provider address from private key
    let pk_bytes = private_key.strip_prefix("0x").unwrap_or(&private_key);
    let pk_decoded = hex::decode(pk_bytes).unwrap_or_else(|_| {
        eprintln!("  error: invalid private key hex");
        std::process::exit(1);
    });
    if pk_decoded.len() != 32 {
        eprintln!("  error: private key must be 32 bytes");
        std::process::exit(1);
    }
    let signing_key = k256::ecdsa::SigningKey::from_bytes(pk_decoded.as_slice().into())
        .unwrap_or_else(|_| {
            eprintln!("  error: invalid private key");
            std::process::exit(1);
        });
    let verifying_key = signing_key.verifying_key();
    let public_key_bytes = verifying_key.to_encoded_point(false);
    let provider_address = Address::from_slice(
        &keccak256(&public_key_bytes.as_bytes()[1..])[12..],
    );

    // 5. Encode the registerService call
    let call = registerServiceCall {
        name: name.to_string(),
        pricePerRequest: U256::from(price_base_units),
        acceptedToken: token_addr,
        metadataUri: metadata_url.to_string(),
    };
    let calldata = hex::encode(call.abi_encode());

    // 6. Get nonce via eth_getTransactionCount
    let http_client = reqwest::Client::new();
    let rpc_url = config.tempo.rpc_urls.first().unwrap_or_else(|| {
        eprintln!("  error: no RPC URL configured");
        std::process::exit(1);
    });

    let nonce = rpc_get_nonce(&http_client, rpc_url, &provider_address).await;

    // 7. Build EIP-1559 transaction
    let chain_id = if config.tempo.chain_id > 0 {
        config.tempo.chain_id
    } else {
        4217 // Tempo mainnet default
    };

    let gas_limit: u64 = 300_000;

    // Get gas price
    let gas_price = rpc_gas_price(&http_client, rpc_url).await;

    // Build raw tx with EIP-155 signing
    let tx_data = hex::decode(&calldata).unwrap();

    // RLP-encode and sign the transaction (legacy tx for compatibility)
    let raw_tx = sign_legacy_tx(
        &signing_key,
        nonce,
        gas_price,
        gas_limit,
        registry_addr,
        U256::ZERO,
        &tx_data,
        chain_id,
    );

    // 8. Send via eth_sendRawTransaction
    let tx_hash = rpc_send_raw_tx(&http_client, rpc_url, &raw_tx).await;

    let price_display = paygate_common::types::format_usd(price_base_units, TOKEN_DECIMALS);
    eprintln!("  Name:     {name}");
    eprintln!("  Price:    {price_display}/request");
    eprintln!("  Token:    {}", truncate_address(token));
    eprintln!("  Provider: {}", truncate_address(&format!("{provider_address}")));
    eprintln!();

    // 9. Wait for receipt
    eprintln!("  Transaction: {tx_hash}");
    let receipt = rpc_wait_for_receipt(&http_client, rpc_url, &tx_hash).await;

    // 10. Decode ServiceRegistered event from receipt logs
    let service_id = decode_service_registered(&receipt);

    match service_id {
        Some(id) => {
            eprintln!("  Service ID:  0x{}", hex::encode(id));
        }
        None => {
            // Check if tx reverted
            let status = receipt["status"].as_str().unwrap_or("0x0");
            if status == "0x0" {
                eprintln!();
                eprintln!("  error: registration transaction reverted");
                eprintln!("    hint: check the transaction on the explorer for details");
            } else {
                eprintln!("  Service ID:  (could not decode from logs)");
            }
        }
    }

    // Explorer link
    let explorer_base = if config.tempo.network == "mainnet" {
        "https://explore.tempo.xyz"
    } else {
        "https://explore.moderato.tempo.xyz"
    };
    eprintln!();
    eprintln!("  View on explorer:");
    eprintln!("    {explorer_base}/tx/{tx_hash}");
    eprintln!();
    eprintln!("  Your service is now discoverable on-chain!");
}

async fn rpc_get_nonce(client: &reqwest::Client, rpc_url: &str, address: &Address) -> u64 {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionCount",
        "params": [format!("{address}"), "latest"],
        "id": 1
    });
    let resp = client
        .post(rpc_url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .unwrap_or_else(|e| {
            eprintln!("  error: Tempo RPC unreachable: {e}");
            eprintln!("    hint: check your network and rpc_urls in paygate.toml");
            std::process::exit(1);
        });
    let json: serde_json::Value = resp.json().await.unwrap_or_default();
    let hex_str = json["result"].as_str().unwrap_or("0x0");
    u64::from_str_radix(hex_str.trim_start_matches("0x"), 16).unwrap_or(0)
}

async fn rpc_gas_price(client: &reqwest::Client, rpc_url: &str) -> u64 {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_gasPrice",
        "params": [],
        "id": 1
    });
    let resp = client
        .post(rpc_url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .unwrap_or_else(|_| {
            eprintln!("  error: failed to get gas price");
            std::process::exit(1);
        });
    let json: serde_json::Value = resp.json().await.unwrap_or_default();
    let hex_str = json["result"].as_str().unwrap_or("0x3B9ACA00"); // 1 gwei fallback
    u64::from_str_radix(hex_str.trim_start_matches("0x"), 16).unwrap_or(1_000_000_000)
}

async fn rpc_send_raw_tx(client: &reqwest::Client, rpc_url: &str, raw_tx: &[u8]) -> String {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_sendRawTransaction",
        "params": [format!("0x{}", hex::encode(raw_tx))],
        "id": 1
    });
    let resp = client
        .post(rpc_url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .unwrap_or_else(|e| {
            eprintln!("  error: failed to send transaction: {e}");
            std::process::exit(1);
        });
    let json: serde_json::Value = resp.json().await.unwrap_or_default();
    if let Some(error) = json.get("error") {
        let msg = error["message"].as_str().unwrap_or("unknown error");
        if msg.contains("insufficient") {
            eprintln!("  error: insufficient balance for gas");
            eprintln!("    hint: fund your wallet first");
        } else {
            eprintln!("  error: transaction failed: {msg}");
        }
        std::process::exit(1);
    }
    json["result"].as_str().unwrap_or("").to_string()
}

async fn rpc_wait_for_receipt(
    client: &reqwest::Client,
    rpc_url: &str,
    tx_hash: &str,
) -> serde_json::Value {
    for _ in 0..30 {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionReceipt",
            "params": [tx_hash],
            "id": 1
        });
        if let Ok(resp) = client
            .post(rpc_url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
        {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if !json["result"].is_null() {
                    return json["result"].clone();
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    eprintln!("  error: timed out waiting for transaction receipt");
    std::process::exit(1);
}

fn decode_service_registered(receipt: &serde_json::Value) -> Option<FixedBytes<32>> {
    let logs = receipt["logs"].as_array()?;
    let event_sig = ServiceRegistered::SIGNATURE_HASH;

    for log in logs {
        let topics = log["topics"].as_array()?;
        if topics.is_empty() {
            continue;
        }
        let topic0 = topics[0].as_str()?;
        let topic0_bytes: FixedBytes<32> = topic0.parse().ok()?;
        if topic0_bytes == event_sig {
            // serviceId is topic[1] (indexed)
            if let Some(service_id_hex) = topics.get(1).and_then(|t| t.as_str()) {
                return service_id_hex.parse::<FixedBytes<32>>().ok();
            }
        }
    }
    None
}

fn sign_legacy_tx(
    signing_key: &k256::ecdsa::SigningKey,
    nonce: u64,
    gas_price: u64,
    gas_limit: u64,
    to: Address,
    value: U256,
    data: &[u8],
    chain_id: u64,
) -> Vec<u8> {
    // RLP-encode the unsigned tx for signing (EIP-155)
    let mut unsigned = Vec::new();
    rlp_encode_u64(&mut unsigned, nonce);
    rlp_encode_u64(&mut unsigned, gas_price);
    rlp_encode_u64(&mut unsigned, gas_limit);
    rlp_encode_bytes(&mut unsigned, to.as_slice());
    rlp_encode_u256(&mut unsigned, value);
    rlp_encode_bytes(&mut unsigned, data);
    // EIP-155: append chain_id, 0, 0
    rlp_encode_u64(&mut unsigned, chain_id);
    rlp_encode_u64(&mut unsigned, 0);
    rlp_encode_u64(&mut unsigned, 0);

    let encoded_unsigned = rlp_encode_list(&unsigned);
    let msg_hash = keccak256(&encoded_unsigned);

    // Sign
    let (sig, recovery_id) = signing_key
        .sign_prehash_recoverable(msg_hash.as_slice())
        .unwrap_or_else(|e| {
            eprintln!("  error: signing failed: {e}");
            std::process::exit(1);
        });

    let v = recovery_id.to_byte() as u64 + chain_id * 2 + 35;
    let r_bytes = sig.r().to_bytes();
    let s_bytes = sig.s().to_bytes();

    // RLP-encode the signed tx
    let mut signed = Vec::new();
    rlp_encode_u64(&mut signed, nonce);
    rlp_encode_u64(&mut signed, gas_price);
    rlp_encode_u64(&mut signed, gas_limit);
    rlp_encode_bytes(&mut signed, to.as_slice());
    rlp_encode_u256(&mut signed, value);
    rlp_encode_bytes(&mut signed, data);
    rlp_encode_u64(&mut signed, v);
    rlp_encode_bytes(&mut signed, trim_leading_zeros(&r_bytes));
    rlp_encode_bytes(&mut signed, trim_leading_zeros(&s_bytes));

    rlp_encode_list(&signed)
}

fn trim_leading_zeros(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    &bytes[start..]
}

// Minimal RLP encoding helpers
fn rlp_encode_u64(buf: &mut Vec<u8>, val: u64) {
    if val == 0 {
        buf.push(0x80);
    } else if val < 128 {
        buf.push(val as u8);
    } else {
        let bytes = val.to_be_bytes();
        let start = bytes.iter().position(|&b| b != 0).unwrap_or(7);
        let len = 8 - start;
        buf.push(0x80 + len as u8);
        buf.extend_from_slice(&bytes[start..]);
    }
}

fn rlp_encode_u256(buf: &mut Vec<u8>, val: U256) {
    if val.is_zero() {
        buf.push(0x80);
    } else {
        let bytes: [u8; 32] = val.to_be_bytes();
        let start = bytes.iter().position(|&b| b != 0).unwrap_or(31);
        let trimmed = &bytes[start..];
        if trimmed.len() == 1 && trimmed[0] < 128 {
            buf.push(trimmed[0]);
        } else {
            buf.push(0x80 + trimmed.len() as u8);
            buf.extend_from_slice(trimmed);
        }
    }
}

fn rlp_encode_bytes(buf: &mut Vec<u8>, data: &[u8]) {
    if data.len() == 1 && data[0] < 128 {
        buf.push(data[0]);
    } else if data.len() < 56 {
        buf.push(0x80 + data.len() as u8);
        buf.extend_from_slice(data);
    } else {
        let len_bytes = data.len().to_be_bytes();
        let start = len_bytes.iter().position(|&b| b != 0).unwrap_or(7);
        let len_of_len = 8 - start;
        buf.push(0xb7 + len_of_len as u8);
        buf.extend_from_slice(&len_bytes[start..]);
        buf.extend_from_slice(data);
    }
}

fn rlp_encode_list(items: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    if items.len() < 56 {
        result.push(0xc0 + items.len() as u8);
    } else {
        let len_bytes = items.len().to_be_bytes();
        let start = len_bytes.iter().position(|&b| b != 0).unwrap_or(7);
        let len_of_len = 8 - start;
        result.push(0xf7 + len_of_len as u8);
        result.extend_from_slice(&len_bytes[start..]);
    }
    result.extend_from_slice(items);
    result
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

    /// Helper: create test state with upstream and optionally a mock RPC.
    /// Returns (AppState, upstream_addr).
    async fn test_state_with_upstream(
        upstream_addr: std::net::SocketAddr,
        webhook_url: Option<String>,
    ) -> AppState {
        let db_path = format!("/tmp/paygate_test_{}.db", uuid::Uuid::new_v4());
        let (db_reader, db_writer) = crate::db::init_db(&db_path).unwrap();

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

        let webhook_sender = webhook_url.map(|url| {
            crate::webhook::WebhookSender::new(reqwest::Client::new(), url, 5)
        });

        AppState {
            config: Arc::new(arc_swap::ArcSwap::new(Arc::new(config))),
            db_reader,
            db_writer,
            http_client: reqwest::Client::new(),
            rate_limiter: Arc::new(RateLimiter::new(100, 10)),
            webhook_sender,
            prometheus_handle: metrics_exporter_prometheus::PrometheusBuilder::new()
                .build_recorder()
                .handle(),
            started_at: std::time::Instant::now(),
        }
    }

    // T18: Health endpoint returns correct JSON for healthy state
    #[tokio::test]
    async fn test_health_endpoint_healthy() {
        // Start mock upstream
        let upstream_app = Router::new().fallback(|| async { "ok" });
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(upstream_listener, upstream_app).into_future());

        // Start mock RPC
        let rpc_app = Router::new().fallback(|| async {
            Json(json!({"jsonrpc":"2.0","result":"0x1","id":1}))
        });
        let rpc_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let rpc_addr = rpc_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(rpc_listener, rpc_app).into_future());

        let mut state = test_state_with_upstream(upstream_addr, None).await;
        // Override RPC URLs to point to our mock
        {
            let mut config = (*state.current_config()).clone();
            config.tempo.rpc_urls = vec![format!("http://{rpc_addr}")];
            state.config.store(Arc::new(config));
        }

        let admin_app = crate::admin::admin_router(state);

        let req = Request::builder()
            .method("GET")
            .uri("/paygate/health")
            .body(Body::empty())
            .unwrap();

        let resp = admin_app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap(),
        )
        .unwrap();
        assert_eq!(body["status"], "healthy");
        assert_eq!(body["db"], "ok");
        assert_eq!(body["tempo_rpc"], "connected");
        assert_eq!(body["upstream"], "reachable");
    }

    // T18: Health endpoint returns degraded when RPC is unreachable
    #[tokio::test]
    async fn test_health_endpoint_degraded() {
        // Start mock upstream
        let upstream_app = Router::new().fallback(|| async { "ok" });
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(upstream_listener, upstream_app).into_future());

        // No RPC server — rpc_urls points to unreachable addr
        let state = test_state_with_upstream(upstream_addr, None).await;

        let admin_app = crate::admin::admin_router(state);

        let req = Request::builder()
            .method("GET")
            .uri("/paygate/health")
            .body(Body::empty())
            .unwrap();

        let resp = admin_app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap(),
        )
        .unwrap();
        assert_eq!(body["status"], "degraded");
        assert_eq!(body["tempo_rpc"], "error");
    }

    // T19: Metrics endpoint returns Prometheus format
    #[tokio::test]
    async fn test_metrics_endpoint_prometheus_format() {
        let upstream_app = Router::new().fallback(|| async { "ok" });
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(upstream_listener, upstream_app).into_future());

        let state = test_state_with_upstream(upstream_addr, None).await;
        let admin_app = crate::admin::admin_router(state);

        let req = Request::builder()
            .method("GET")
            .uri("/paygate/metrics")
            .body(Body::empty())
            .unwrap();

        let resp = admin_app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let content_type = resp.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(
            content_type.contains("text/plain"),
            "metrics should return text/plain content type"
        );

        let body = String::from_utf8(
            axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();

        // Prometheus output is text-based, may be empty if no metrics recorded yet
        // but should at least be valid (no error)
        assert!(
            body.is_empty() || body.contains('#') || body.contains("paygate_"),
            "metrics should be empty or contain Prometheus-formatted lines"
        );
    }

    // Receipt endpoint: known tx_hash returns 200
    #[tokio::test]
    async fn test_receipt_endpoint_found() {
        let upstream_app = Router::new().fallback(|| async { "ok" });
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(upstream_listener, upstream_app).into_future());

        let state = test_state_with_upstream(upstream_addr, None).await;

        // Insert a payment record directly
        let record = paygate_common::types::PaymentRecord {
            id: "test_id".into(),
            tx_hash: "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            payer_address: "0x9E2b000000000000000000000000000000000001".into(),
            amount: 5000,
            token_address: "0x1234000000000000000000000000000000000001".into(),
            endpoint: "POST /v1/chat".into(),
            request_hash: None,
            quote_id: None,
            block_number: 100,
            verified_at: chrono::Utc::now().timestamp(),
            status: "verified".into(),
        };
        state.db_writer.insert_payment(record).await.unwrap();

        let admin_app = crate::admin::admin_router(state);

        let req = Request::builder()
            .method("GET")
            .uri("/paygate/receipts/0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .body(Body::empty())
            .unwrap();

        let resp = admin_app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap(),
        )
        .unwrap();
        assert_eq!(body["tx_hash"], "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(body["payer_address"], "0x9E2b000000000000000000000000000000000001");
        assert_eq!(body["amount"], 5000);
        assert_eq!(body["status"], "verified");
    }

    // Receipt endpoint: unknown tx_hash returns 404
    #[tokio::test]
    async fn test_receipt_endpoint_not_found() {
        let upstream_app = Router::new().fallback(|| async { "ok" });
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(upstream_listener, upstream_app).into_future());

        let state = test_state_with_upstream(upstream_addr, None).await;
        let admin_app = crate::admin::admin_router(state);

        let req = Request::builder()
            .method("GET")
            .uri("/paygate/receipts/0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
            .body(Body::empty())
            .unwrap();

        let resp = admin_app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap(),
        )
        .unwrap();
        assert_eq!(body["error"], "payment not found");
    }

    // Webhook delivery test: payment triggers webhook POST
    #[tokio::test]
    async fn test_webhook_delivery() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let received = Arc::new(AtomicBool::new(false));
        let received_clone = received.clone();

        // Start a webhook receiver server
        let webhook_app = Router::new().fallback(move || {
            let received = received_clone.clone();
            async move {
                received.store(true, Ordering::SeqCst);
                "ok"
            }
        });
        let webhook_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let webhook_addr = webhook_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(webhook_listener, webhook_app).into_future());

        let webhook_sender = crate::webhook::WebhookSender::new(
            reqwest::Client::new(),
            format!("http://{webhook_addr}/webhook"),
            5,
        );

        webhook_sender.notify_payment_verified(
            "0xabc123",
            "0x9E2b000000000000000000000000000000000001",
            5000,
            "POST /v1/chat",
        );

        // Give the async task time to deliver
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        assert!(
            received.load(Ordering::SeqCst),
            "webhook should have been delivered"
        );
    }

    // Webhook failure test: bad webhook URL doesn't block
    #[tokio::test]
    async fn test_webhook_failure_does_not_block() {
        let webhook_sender = crate::webhook::WebhookSender::new(
            reqwest::Client::new(),
            "http://127.0.0.1:1/nonexistent".into(), // will fail to connect
            1, // 1 second timeout
        );

        let start = std::time::Instant::now();
        webhook_sender.notify_payment_verified(
            "0xabc123",
            "0x9E2b000000000000000000000000000000000001",
            5000,
            "POST /v1/chat",
        );
        let elapsed = start.elapsed();

        // notify_payment_verified should return immediately (fire-and-forget)
        assert!(
            elapsed.as_millis() < 50,
            "webhook notification should be non-blocking, took {}ms",
            elapsed.as_millis()
        );
    }

    // T13: Wrong recipient — dedicated test via gateway handler
    #[tokio::test]
    async fn test_wrong_recipient_returns_error() {
        // This tests the gateway handler with a payment to wrong address.
        // The verifier will see the Transfer event going to a different address.
        // We need a mock RPC that returns a receipt with wrong `to`.
        let upstream_app = Router::new().fallback(|| async { "ok" });
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(upstream_listener, upstream_app).into_future());

        // Mock RPC: returns a receipt where Transfer `to` is wrong address
        let rpc_app = Router::new().fallback(|body: String| async move {
            let req: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let resp = match method {
                "eth_getTransactionReceipt" => {
                    // Transfer log to WRONG address (not provider)
                    json!({
                        "jsonrpc": "2.0",
                        "result": {
                            "blockNumber": "0x1",
                            "logs": [{
                                "address": "0x1234000000000000000000000000000000000001",
                                "topics": [
                                    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
                                    "0x0000000000000000000000009e2b000000000000000000000000000000000001",
                                    "0x000000000000000000000000dead000000000000000000000000000000000001"
                                ],
                                "data": "0x00000000000000000000000000000000000000000000000000000000000003e8"
                            }]
                        },
                        "id": 1
                    })
                }
                "eth_getBlockByNumber" => {
                    let ts = chrono::Utc::now().timestamp() as u64;
                    json!({"jsonrpc":"2.0","result":{"timestamp":format!("0x{ts:x}")},"id":1})
                }
                _ => json!({"jsonrpc":"2.0","error":{"code":-1},"id":1}),
            };
            Json(resp)
        });
        let rpc_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let rpc_addr = rpc_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(rpc_listener, rpc_app).into_future());

        let mut state = test_state_with_upstream(upstream_addr, None).await;
        {
            let mut config = (*state.current_config()).clone();
            config.tempo.rpc_urls = vec![format!("http://{rpc_addr}")];
            state.config.store(Arc::new(config));
        }

        let app = Router::new()
            .fallback(gateway_handler)
            .with_state(state);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat")
            .header("X-Payment-Tx", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .header("X-Payment-Payer", "0x9E2b000000000000000000000000000000000001")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        // Wrong recipient should result in an error (InvalidTransfer or similar)
        // The exact status depends on decode_transfer_events filtering — it should
        // return "no matching transfer" since provider address doesn't match
        assert_ne!(resp.status(), StatusCode::OK, "wrong recipient should not succeed");
    }

    // 402 flood rate limiter test
    #[tokio::test]
    async fn test_402_flood_rate_limiter() {
        let upstream_app = Router::new().fallback(|| async { "ok" });
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(upstream_listener, upstream_app).into_future());

        let state = test_state_with_upstream(upstream_addr, None).await;

        let app = Router::new()
            .fallback(gateway_handler)
            .with_state(state);

        // Send many requests without payment headers from same IP
        // The 402 flood limiter should eventually reject
        let mut got_429 = false;
        for _ in 0..1100 {
            let app_clone = app.clone();
            let req = Request::builder()
                .method("POST")
                .uri("/v1/chat")
                .header("x-forwarded-for", "1.2.3.4")
                .body(Body::empty())
                .unwrap();

            let resp = app_clone.oneshot(req).await.unwrap();
            if resp.status() == StatusCode::TOO_MANY_REQUESTS {
                got_429 = true;
                break;
            }
        }
        assert!(got_429, "402 flood limiter should eventually return 429");
    }

    // Register command: ABI encoding produces correct calldata
    #[test]
    fn test_register_service_abi_encoding() {
        use alloy_sol_types::SolCall;

        let call = registerServiceCall {
            name: "my-api".to_string(),
            pricePerRequest: U256::from(1000u64),
            acceptedToken: "0x20c0000000000000000000000000000000000000"
                .parse::<Address>()
                .unwrap(),
            metadataUri: "https://example.com/meta.json".to_string(),
        };

        let encoded = call.abi_encode();
        // First 4 bytes are the function selector
        let selector = &encoded[..4];
        // registerService(string,uint256,address,string) selector
        let expected_selector = &alloy_primitives::keccak256(
            b"registerService(string,uint256,address,string)"
        )[..4];
        assert_eq!(selector, expected_selector, "function selector mismatch");

        // Verify it round-trips
        let decoded = registerServiceCall::abi_decode(&encoded).unwrap();
        assert_eq!(decoded.name, "my-api");
        assert_eq!(decoded.pricePerRequest, U256::from(1000u64));
        assert_eq!(
            decoded.acceptedToken,
            "0x20c0000000000000000000000000000000000000"
                .parse::<Address>()
                .unwrap()
        );
        assert_eq!(decoded.metadataUri, "https://example.com/meta.json");
    }

    // Register command: ServiceRegistered event topic matches expected keccak256
    #[test]
    fn test_service_registered_event_signature() {
        use alloy_sol_types::SolEvent;

        let sig_hash = ServiceRegistered::SIGNATURE_HASH;
        let expected = alloy_primitives::keccak256(
            b"ServiceRegistered(bytes32,address,uint256)"
        );
        assert_eq!(sig_hash, expected, "ServiceRegistered event signature mismatch");
    }
}
