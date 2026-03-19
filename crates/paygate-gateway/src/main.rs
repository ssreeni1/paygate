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
use paygate_common::types::VerificationResult;
use proxy::ProxyError;
use server::AppState;
use serde_json::json;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("paygate=info".parse().unwrap()),
        )
        .init();

    let config_path = std::path::Path::new("paygate.toml");
    let config = match config::Config::load(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("  hint: run `paygate init` to create paygate.toml");
            std::process::exit(1);
        }
    };

    let (db_reader, db_writer) = match db::init_db("paygate.db") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: failed to initialize database: {e}");
            std::process::exit(1);
        }
    };

    let http_client = reqwest::Client::builder()
        .pool_max_idle_per_host(config.tempo.rpc_pool_max_idle)
        .build()
        .expect("failed to create HTTP client");

    let rate_limiter = Arc::new(rate_limit::RateLimiter::new(
        config.rate_limiting.requests_per_second,
        config.rate_limiting.per_payer_per_second,
    ));

    let webhook_sender = if !config.webhooks.payment_verified_url.is_empty() {
        Some(webhook::WebhookSender::new(
            http_client.clone(),
            config.webhooks.payment_verified_url.clone(),
            config.webhooks.timeout_seconds,
        ))
    } else {
        None
    };

    let listen = config.gateway.listen.clone();
    let retention_days = config.storage.request_log_retention_days;

    let state = AppState {
        config: Arc::new(arc_swap::ArcSwap::new(Arc::new(config))),
        db_reader: db_reader.clone(),
        db_writer,
        http_client,
        rate_limiter,
        webhook_sender,
    };

    tokio::spawn(db::cleanup_task(db_reader, retention_days));

    let app = Router::new()
        .fallback(gateway_handler)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit::rate_limit_middleware,
        ))
        .with_state(state);

    info!("PayGate v0.1.0");
    info!("Proxy: {listen}");
    info!("Ready. Accepting payments.");

    let listener = tokio::net::TcpListener::bind(&listen).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

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
