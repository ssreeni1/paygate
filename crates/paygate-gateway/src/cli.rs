use axum::middleware;
use axum::Router;
use crate::config;
use crate::config::parse_price_to_base_units;
use crate::db;
use crate::helpers::*;
use crate::rate_limit;
use crate::serve::{gateway_handler, check_rpc_connectivity};
use crate::server;
use crate::sessions;
use paygate_common::types::{format_amount, format_usd, TOKEN_DECIMALS};
use std::path::Path;
use std::sync::Arc;

// ─── init ────────────────────────────────────────────────────────────────────

pub(crate) fn cmd_init(force: bool) {
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

// ─── status ──────────────────────────────────────────────────────────────────

pub(crate) async fn cmd_status(config_path: &str) {
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

pub(crate) fn cmd_pricing(config_path: &str, html: bool) {
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

// ─── revenue ─────────────────────────────────────────────────────────────────

pub(crate) fn cmd_revenue(config_path: &str) {
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

// ─── wallet ──────────────────────────────────────────────────────────────────

pub(crate) async fn cmd_wallet(config_path: &str) {
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

pub(crate) async fn cmd_test(is_demo: bool) {
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
            no_charge_on_5xx: Default::default(),
        },
        rate_limiting: Default::default(),
        security: Default::default(),
        webhooks: Default::default(),
        storage: Default::default(),
        governance: Default::default(),
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
        spend_accumulator: Arc::new(sessions::SpendAccumulator::new()),
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

pub(crate) fn cmd_sessions(config_path: &str) {
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

pub(crate) async fn cmd_register(
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

#[cfg(test)]
mod tests {
    use super::*;

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
