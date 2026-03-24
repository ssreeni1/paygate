use axum::body::Body;
use axum::http::{HeaderValue, Request, Response};
use paygate_common::mpp;
use paygate_common::types::{format_amount, TOKEN_DECIMALS};
use std::time::Instant;
use thiserror::Error;
use tracing::warn;

use crate::metrics;
use crate::server::AppState;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("upstream timeout")]
    Timeout,
    #[error("upstream connection error: {0}")]
    Connection(String),
    #[error("response body too large")]
    PayloadTooLarge,
    #[error("request error: {0}")]
    Request(String),
}

/// Forward a request to the upstream API.
/// Strips X-Payment-* headers, adds receipt headers on response.
pub async fn forward_request(
    state: &AppState,
    request: Request<Body>,
    tx_hash: &str,
    amount_charged: u64,
    endpoint: &str,
) -> Result<Response<Body>, ProxyError> {
    let config = state.current_config();
    let start = Instant::now();

    let (mut parts, body) = request.into_parts();

    // 1. Strip all X-Payment-* headers
    let payment_headers: Vec<_> = parts
        .headers
        .keys()
        .filter(|name| mpp::is_payment_header(name.as_str()))
        .cloned()
        .collect();
    for name in payment_headers {
        parts.headers.remove(&name);
    }

    // 2. Build upstream URL
    let upstream = config.gateway.upstream.trim_end_matches('/');
    let path = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let upstream_url = format!("{upstream}{path}");

    // 3. Read request body
    let body_bytes = axum::body::to_bytes(body, config.security.max_request_body_bytes)
        .await
        .map_err(|e| ProxyError::Request(format!("failed to read request body: {e}")))?;

    // 4. Build and send upstream request
    let timeout = std::time::Duration::from_secs(config.gateway.upstream_timeout_seconds);
    let mut req_builder = state
        .http_client
        .request(parts.method.clone(), &upstream_url)
        .timeout(timeout);

    for (name, value) in &parts.headers {
        // Skip hop-by-hop headers and Host (reqwest sets Host from the URL)
        if name == "host" || name == "transfer-encoding" || name == "connection" {
            continue;
        }
        req_builder = req_builder.header(name.clone(), value.clone());
    }
    req_builder = req_builder.body(body_bytes.to_vec());

    tracing::info!(url = %upstream_url, method = %parts.method, "proxying request to upstream");
    let upstream_resp = match req_builder.send().await {
        Ok(r) => {
            tracing::info!(status = %r.status(), "upstream responded");
            r
        }
        Err(e) if e.is_timeout() => {
            metrics::record_upstream_duration(endpoint, 504, start.elapsed().as_secs_f64());
            return Err(ProxyError::Timeout);
        }
        Err(e) => {
            tracing::error!(error = %e, "upstream connection error");
            metrics::record_upstream_duration(endpoint, 502, start.elapsed().as_secs_f64());
            return Err(ProxyError::Connection(e.to_string()));
        }
    };

    let status = upstream_resp.status();
    let resp_headers = upstream_resp.headers().clone();

    // 5. Check response body size
    let resp_bytes = upstream_resp
        .bytes()
        .await
        .map_err(|e| ProxyError::Connection(format!("failed to read response: {e}")))?;

    if resp_bytes.len() > config.gateway.max_response_body_bytes {
        warn!(
            endpoint = endpoint,
            size = resp_bytes.len(),
            "upstream response too large"
        );
        return Err(ProxyError::PayloadTooLarge);
    }

    metrics::record_upstream_duration(endpoint, status.as_u16(), start.elapsed().as_secs_f64());

    // 6. Build response with receipt headers
    let mut response = Response::builder().status(status);
    for (name, value) in &resp_headers {
        // Skip hop-by-hop headers that don't apply to the rewritten response
        if name == "transfer-encoding" || name == "connection" || name == "content-length" {
            continue;
        }
        response = response.header(name, value);
    }

    if !tx_hash.is_empty() {
        if let Ok(v) = HeaderValue::from_str(tx_hash) {
            response = response.header(mpp::HEADER_PAYMENT_RECEIPT, v);
        }
        let cost_str = format_amount(amount_charged, TOKEN_DECIMALS);
        if let Ok(v) = HeaderValue::from_str(&cost_str) {
            response = response.header(mpp::HEADER_PAYMENT_COST, v);
        }
    }

    response
        .body(Body::from(resp_bytes))
        .map_err(|e| ProxyError::Request(e.to_string()))
}

// Test 14: Header sanitization
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use crate::rate_limit::RateLimiter;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_header_sanitization() {
        // Start a mock upstream that echoes back received headers
        let upstream_app = axum::Router::new().fallback(
            |req: Request<Body>| async move {
                let has_payment = req
                    .headers()
                    .keys()
                    .any(|k| mpp::is_payment_header(k.as_str()));
                if has_payment {
                    "FAIL: payment headers leaked"
                } else {
                    "OK"
                }
            },
        );
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
            pricing: Default::default(),
            rate_limiting: Default::default(),
            security: Default::default(),
            webhooks: Default::default(),
            storage: Default::default(),
            governance: Default::default(),
        };

        let state = AppState {
            config: Arc::new(arc_swap::ArcSwap::new(Arc::new(config))),
            db_reader,
            db_writer,
            http_client: reqwest::Client::new(),
            rate_limiter: Arc::new(RateLimiter::new(100, 10)),
            webhook_sender: None,
            prometheus_handle: metrics_exporter_prometheus::PrometheusBuilder::new()
                .build_recorder()
                .handle(),
            started_at: std::time::Instant::now(),
            spend_accumulator: Arc::new(crate::sessions::SpendAccumulator::new()),
        };

        // Build a request WITH X-Payment-* headers
        let req = Request::builder()
            .method("GET")
            .uri("/v1/test")
            .header("X-Payment-Tx", "0xabc")
            .header("X-Payment-Payer", "0x123")
            .header("X-Payment-Quote-Id", "qt_xyz")
            .header("X-Normal-Header", "keep-me")
            .body(Body::empty())
            .unwrap();

        let resp = forward_request(&state, req, "0xabc", 1000, "GET /v1/test")
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"OK", "payment headers should be stripped");
    }

    fn test_state_with_upstream(upstream_addr: std::net::SocketAddr) -> AppState {
        let db_path = format!("/tmp/paygate_test_{}.db", uuid::Uuid::new_v4());
        let (db_reader, db_writer) = crate::db::init_db(&db_path).unwrap();
        let config = Config {
            gateway: GatewayConfig {
                listen: "127.0.0.1:0".into(),
                admin_listen: "127.0.0.1:0".into(),
                upstream: format!("http://{upstream_addr}"),
                upstream_timeout_seconds: 5,
                max_response_body_bytes: 1024, // small limit for testing
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
            governance: Default::default(),
        };
        AppState {
            config: Arc::new(arc_swap::ArcSwap::new(Arc::new(config))),
            db_reader,
            db_writer,
            http_client: reqwest::Client::new(),
            rate_limiter: Arc::new(RateLimiter::new(100, 10)),
            webhook_sender: None,
            prometheus_handle: metrics_exporter_prometheus::PrometheusBuilder::new()
                .build_recorder()
                .handle(),
            started_at: std::time::Instant::now(),
            spend_accumulator: Arc::new(crate::sessions::SpendAccumulator::new()),
        }
    }

    // T15: Upstream 5xx returns 502 + receipt headers
    #[tokio::test]
    async fn test_upstream_5xx_returns_response_with_receipt() {
        let upstream_app = axum::Router::new().fallback(|| async {
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "server error")
        });
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(upstream_listener, upstream_app).into_future());

        let state = test_state_with_upstream(upstream_addr);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat")
            .body(Body::empty())
            .unwrap();

        let resp = forward_request(&state, req, "0xabc123", 5000, "POST /v1/chat")
            .await
            .unwrap();

        // Upstream 5xx is forwarded through (not converted to 502 by proxy)
        assert_eq!(resp.status().as_u16(), 500);
        // Receipt headers should still be present
        assert!(resp.headers().get(mpp::HEADER_PAYMENT_RECEIPT).is_some());
        assert_eq!(
            resp.headers().get(mpp::HEADER_PAYMENT_RECEIPT).unwrap().to_str().unwrap(),
            "0xabc123"
        );
        // X-Payment-Cost should be present
        assert!(resp.headers().get(mpp::HEADER_PAYMENT_COST).is_some());
    }

    // T15 variant: X-Payment-Cost header has correct amount
    #[tokio::test]
    async fn test_x_payment_cost_header_correct_amount() {
        let upstream_app = axum::Router::new().fallback(|| async { "ok" });
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(upstream_listener, upstream_app).into_future());

        let state = test_state_with_upstream(upstream_addr);

        let req = Request::builder()
            .method("GET")
            .uri("/v1/test")
            .body(Body::empty())
            .unwrap();

        let resp = forward_request(&state, req, "0xdef456", 5000, "GET /v1/test")
            .await
            .unwrap();

        assert_eq!(resp.status().as_u16(), 200);
        let cost = resp.headers().get(mpp::HEADER_PAYMENT_COST).unwrap().to_str().unwrap();
        assert_eq!(cost, "0.005000", "cost should be 5000 base units formatted as 0.005000");
    }

    // T29: Upstream response body too large returns PayloadTooLarge
    #[tokio::test]
    async fn test_upstream_response_too_large() {
        // Return a response larger than the 1024-byte limit
        let upstream_app = axum::Router::new().fallback(|| async {
            "x".repeat(2048)
        });
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(upstream_listener, upstream_app).into_future());

        let state = test_state_with_upstream(upstream_addr);

        let req = Request::builder()
            .method("GET")
            .uri("/v1/large")
            .body(Body::empty())
            .unwrap();

        let result = forward_request(&state, req, "0xabc", 1000, "GET /v1/large").await;
        assert!(
            matches!(result, Err(ProxyError::PayloadTooLarge)),
            "should return PayloadTooLarge for oversized response"
        );
    }

    // No receipt headers when tx_hash is empty (free endpoint)
    #[tokio::test]
    async fn test_no_receipt_headers_for_free_endpoint() {
        let upstream_app = axum::Router::new().fallback(|| async { "free" });
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(upstream_listener, upstream_app).into_future());

        let state = test_state_with_upstream(upstream_addr);

        let req = Request::builder()
            .method("GET")
            .uri("/v1/free")
            .body(Body::empty())
            .unwrap();

        // Empty tx_hash = free endpoint
        let resp = forward_request(&state, req, "", 0, "GET /v1/free")
            .await
            .unwrap();

        assert_eq!(resp.status().as_u16(), 200);
        assert!(resp.headers().get(mpp::HEADER_PAYMENT_RECEIPT).is_none());
        assert!(resp.headers().get(mpp::HEADER_PAYMENT_COST).is_none());
    }
}
