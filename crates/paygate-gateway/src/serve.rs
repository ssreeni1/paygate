use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, Request, StatusCode};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum::Router;
use crate::admin;
use crate::config::{Config, ConfigError};
use crate::db;
use crate::metrics;
use crate::mpp;
use crate::proxy::ProxyError;
use crate::rate_limit;
use crate::server::AppState;
use crate::sessions;
use crate::sponsor;
use crate::verifier;
use crate::webhook;
use paygate_common::types::VerificationResult;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

// ─── serve ───────────────────────────────────────────────────────────────────

pub(crate) async fn cmd_serve(config_path: &str) {
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
    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods([axum::http::Method::GET, axum::http::Method::POST, axum::http::Method::OPTIONS])
        .allow_headers(tower_http::cors::Any);

    let mut gateway_app = Router::new()
        .route("/paygate/sessions/nonce", axum::routing::post(sessions::handle_nonce))
        .route("/paygate/sessions", axum::routing::post(sessions::handle_create_session)
            .get(sessions::handle_get_sessions))
        .merge(admin::receipt_route())
        .merge(admin::transactions_route())
        .fallback(gateway_handler)
        .layer(cors)
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

pub(crate) async fn gateway_handler(State(state): State<AppState>, req: Request<Body>) -> Response {
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
        return match crate::proxy::forward_request(&state, req, "", 0, &endpoint).await {
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

    // Session auth: HMAC-based
    if parts.headers.contains_key("x-payment-session") {
        let request_hash = paygate_common::hash::request_hash(&method, &path, &body_bytes);
        match sessions::verify_and_deduct(&state, &parts.headers, &request_hash, &endpoint).await {
            Ok(deduction) => {
                let req = Request::from_parts(parts, Body::from(body_bytes));
                return match crate::proxy::forward_request(&state, req, "", deduction.amount_deducted, &endpoint).await {
                    Ok(mut resp) => {
                        // no_charge_on_5xx refund
                        if resp.status().is_server_error() && config.is_no_charge_on_5xx(&endpoint) {
                            let _ = state.db_writer.refund_session_balance(&deduction.session_id, deduction.amount_deducted).await;
                            resp.headers_mut().insert("X-Payment-Refunded", HeaderValue::from_static("true"));
                        }

                        // Dynamic pricing adjustment (session auth only)
                        let final_cost = if config.pricing.dynamic.enabled {
                            if let Some(token_count_header) = resp.headers().get(&config.pricing.dynamic.header_source) {
                                if let Ok(token_count_str) = token_count_header.to_str() {
                                    if let Ok(token_count) = token_count_str.parse::<u64>() {
                                        let dynamic_cost = config.pricing.dynamic.compute_cost(token_count);

                                        if dynamic_cost > deduction.amount_deducted {
                                            let diff = dynamic_cost - deduction.amount_deducted;
                                            let _ = sessions::deduct_additional(&state, &deduction.session_id, diff).await;
                                        } else if dynamic_cost < deduction.amount_deducted {
                                            let diff = deduction.amount_deducted - dynamic_cost;
                                            let _ = state.db_writer.refund_session_balance(&deduction.session_id, diff).await;
                                        }

                                        // Update X-Payment-Cost header to reflect actual cost
                                        let cost_decimal = format!("{:.6}", dynamic_cost as f64 / 1_000_000.0);
                                        if let Ok(v) = HeaderValue::from_str(&cost_decimal) {
                                            resp.headers_mut().insert("X-Payment-Cost", v);
                                        }

                                        dynamic_cost
                                    } else { deduction.amount_deducted }
                                } else { deduction.amount_deducted }
                            } else { deduction.amount_deducted }
                        } else { deduction.amount_deducted };

                        let _ = state.db_writer.log_request(
                            None,
                            Some(deduction.session_id),
                            endpoint,
                            deduction.payer_address,
                            final_cost,
                            Some(resp.status().as_u16() as i32),
                            None,
                        ).await;
                        resp
                    }
                    Err(ProxyError::Timeout) => (
                        StatusCode::GATEWAY_TIMEOUT,
                        Json(json!({"error": "upstream timeout"})),
                    ).into_response(),
                    Err(ProxyError::PayloadTooLarge) => (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({"error": "response too large"})),
                    ).into_response(),
                    Err(e) => (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({"error": format!("upstream error: {e}")})),
                    ).into_response(),
                };
            }
            Err(sessions::SessionError::InsufficientBalance { balance, rate }) => {
                return (StatusCode::PAYMENT_REQUIRED, Json(json!({
                    "error": "insufficient_session_balance",
                    "message": "Session balance too low",
                    "balance": balance,
                    "rate_per_request": rate,
                }))).into_response();
            }
            Err(sessions::SessionError::InvalidSignature) | Err(sessions::SessionError::StaleTimestamp) => {
                return (StatusCode::FORBIDDEN, Json(json!({"error": "invalid_session_auth"}))).into_response();
            }
            Err(sessions::SessionError::SessionNotFound) | Err(sessions::SessionError::SessionExpired) => {
                return (StatusCode::PAYMENT_REQUIRED, Json(json!({"error": "session_expired_or_not_found"}))).into_response();
            }
            Err(_) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "session_error"}))).into_response();
            }
        }
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
            match crate::proxy::forward_request(&state, req, &payment.tx_hash, proof.amount, &endpoint)
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

pub(crate) async fn check_rpc_connectivity(client: &reqwest::Client, rpc_urls: &[String]) -> bool {
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

// Test 13: Free endpoint bypasses payment
#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin;
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
                no_charge_on_5xx: Vec::new(),
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
    pub(crate) async fn test_state_with_upstream(
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
                no_charge_on_5xx: Vec::new(),
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

    // ─── Transaction Explorer Tests (T1-T6) ─────────────────────────────

    fn insert_test_payment(conn: &rusqlite::Connection, tx_hash: &str, amount: i64, verified_at: i64) {
        conn.execute(
            "INSERT INTO payments (id, tx_hash, payer_address, amount, token_address, endpoint,
             request_hash, quote_id, block_number, verified_at, status)
             VALUES (?1, ?2, '0xpayer', ?3, '0xtoken', 'POST /v1/chat', NULL, NULL, 1, ?4, 'verified')",
            rusqlite::params![
                format!("pay_{tx_hash}"),
                tx_hash,
                amount,
                verified_at,
            ],
        ).unwrap();
    }

    #[test]
    fn test_recent_transactions_ordered() {
        let db_path = format!("/tmp/paygate_tx_test_{}.db", uuid::Uuid::new_v4());
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(include_str!("../../../schema.sql")).unwrap();

        insert_test_payment(&conn, "0xaaa", 1000, 100);
        insert_test_payment(&conn, "0xbbb", 2000, 300);
        insert_test_payment(&conn, "0xccc", 3000, 200);
        drop(conn);

        let reader = crate::db::DbReader::new(&db_path);
        let txs = reader.recent_transactions(10, 0).unwrap();

        assert_eq!(txs.len(), 3);
        // Should be ordered by verified_at DESC: 300, 200, 100
        assert_eq!(txs[0].tx_hash, "0xbbb");
        assert_eq!(txs[1].tx_hash, "0xccc");
        assert_eq!(txs[2].tx_hash, "0xaaa");

        std::fs::remove_file(&db_path).ok();
    }

    #[test]
    fn test_recent_transactions_empty_db() {
        let db_path = format!("/tmp/paygate_tx_test_{}.db", uuid::Uuid::new_v4());
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(include_str!("../../../schema.sql")).unwrap();
        drop(conn);

        let reader = crate::db::DbReader::new(&db_path);
        let txs = reader.recent_transactions(10, 0).unwrap();
        assert!(txs.is_empty());

        std::fs::remove_file(&db_path).ok();
    }

    #[test]
    fn test_transaction_stats_correct() {
        let db_path = format!("/tmp/paygate_tx_test_{}.db", uuid::Uuid::new_v4());
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(include_str!("../../../schema.sql")).unwrap();

        insert_test_payment(&conn, "0xd1", 1000, 100);
        insert_test_payment(&conn, "0xd2", 2000, 200);
        insert_test_payment(&conn, "0xd3", 5000, 300);
        drop(conn);

        let reader = crate::db::DbReader::new(&db_path);
        let (count, revenue) = reader.transaction_stats().unwrap();
        assert_eq!(count, 3);
        assert_eq!(revenue, 8000); // 1000 + 2000 + 5000

        std::fs::remove_file(&db_path).ok();
    }

    #[tokio::test]
    async fn test_transactions_endpoint_json() {
        let db_path = format!("/tmp/paygate_tx_test_{}.db", uuid::Uuid::new_v4());
        let (db_reader, db_writer) = crate::db::init_db(&db_path).unwrap();

        // Insert a payment directly via SQL
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            insert_test_payment(&conn, "0xe1", 5000, 1000);
        }

        let prometheus_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
            .build_recorder()
            .handle();

        let upstream_app = Router::new().fallback(|| async { "ok" });
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(upstream_listener, upstream_app).into_future());

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
            pricing: Default::default(),
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

        let app = admin::transactions_route().with_state(state);

        let req = axum::http::Request::builder()
            .uri("/paygate/transactions")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert!(json["transactions"].is_array());
        assert_eq!(json["transactions"].as_array().unwrap().len(), 1);
        assert_eq!(json["total"], 1);
        assert_eq!(json["transactions"][0]["tx_hash"], "0xe1");
        assert_eq!(json["transactions"][0]["amount"], 5000);

        std::fs::remove_file(&db_path).ok();
    }

    #[tokio::test]
    async fn test_transactions_limit_param() {
        let db_path = format!("/tmp/paygate_tx_test_{}.db", uuid::Uuid::new_v4());
        let (db_reader, db_writer) = crate::db::init_db(&db_path).unwrap();

        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            insert_test_payment(&conn, "0xf1", 1000, 100);
            insert_test_payment(&conn, "0xf2", 2000, 200);
            insert_test_payment(&conn, "0xf3", 3000, 300);
        }

        let prometheus_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
            .build_recorder()
            .handle();

        let upstream_app = Router::new().fallback(|| async { "ok" });
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(upstream_listener, upstream_app).into_future());

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
            pricing: Default::default(),
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

        let app = admin::transactions_route().with_state(state);

        let req = axum::http::Request::builder()
            .uri("/paygate/transactions?limit=2")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let txs = json["transactions"].as_array().unwrap();
        assert_eq!(txs.len(), 2);
        // Total should still be 3
        assert_eq!(json["total"], 3);

        std::fs::remove_file(&db_path).ok();
    }

    #[tokio::test]
    async fn test_transactions_cors_headers() {
        let db_path = format!("/tmp/paygate_tx_test_{}.db", uuid::Uuid::new_v4());
        let (db_reader, db_writer) = crate::db::init_db(&db_path).unwrap();

        let prometheus_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
            .build_recorder()
            .handle();

        let upstream_app = Router::new().fallback(|| async { "ok" });
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(upstream_listener, upstream_app).into_future());

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
            pricing: Default::default(),
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

        let app = admin::transactions_route().with_state(state);

        let req = axum::http::Request::builder()
            .uri("/paygate/transactions")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let cors = resp.headers().get("access-control-allow-origin");
        assert!(cors.is_some(), "CORS header missing");
        assert_eq!(
            cors.unwrap().to_str().unwrap(),
            "https://ssreeni1.github.io"
        );

        std::fs::remove_file(&db_path).ok();
    }
}
