use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use paygate_common::mpp::*;
use paygate_common::types::*;
use crate::server::AppState;
use serde_json::json;

/// Payment headers extracted from a request.
pub struct PaymentHeaders {
    pub tx_hash: String,
    pub payer_address: String,
    pub quote_id: Option<String>,
}

/// Check if a request has payment headers (X-Payment-Tx).
pub fn has_payment_headers(headers: &HeaderMap) -> bool {
    headers.contains_key(HEADER_PAYMENT_TX)
}

/// Extract payment headers from a request.
pub fn extract_payment_headers(headers: &HeaderMap) -> Option<PaymentHeaders> {
    let tx_hash = headers.get(HEADER_PAYMENT_TX)?.to_str().ok()?.to_string();
    let payer_address = headers.get(HEADER_PAYMENT_PAYER)?.to_str().ok()?.to_string();
    let quote_id = headers
        .get(HEADER_PAYMENT_QUOTE_ID)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    Some(PaymentHeaders {
        tx_hash,
        payer_address,
        quote_id,
    })
}

/// Generate a 402 Payment Required response for an endpoint.
pub async fn payment_required_response(state: &AppState, endpoint: &str) -> Response {
    let config = state.current_config();
    let price = config.price_for_endpoint(endpoint);
    let quote_id = format!("qt_{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let now = chrono::Utc::now();
    let expires_at = now + chrono::Duration::seconds(config.pricing.quote_ttl_seconds as i64);

    // Store quote in DB
    let accepted_token: alloy_primitives::Address =
        config.tempo.accepted_token.parse().unwrap_or_default();
    let quote = Quote {
        id: quote_id.clone(),
        endpoint: endpoint.to_string(),
        price,
        token: accepted_token,
        created_at: now.timestamp(),
        expires_at: expires_at.timestamp(),
    };
    let _ = state.db_writer.insert_quote(quote).await;

    let amount_str = format_amount(price, TOKEN_DECIMALS);
    let provider_addr = &config.provider.address;

    let body = json!({
        "error": "payment_required",
        "message": format!("Send {amount_str} USDC to {provider_addr} on Tempo, then retry with X-Payment-Tx header."),
        "help_url": "https://docs.paygate.dev/quickstart#paying",
        "pricing": {
            "amount": amount_str,
            "amount_base_units": price,
            "decimals": TOKEN_DECIMALS,
            "token": config.tempo.accepted_token,
            "recipient": provider_addr,
            "quote_id": quote_id,
            "quote_expires_at": expires_at.to_rfc3339(),
            "methods": ["direct", "session"]
        }
    });

    let mut response = (StatusCode::PAYMENT_REQUIRED, Json(body)).into_response();
    let h = response.headers_mut();

    let _ = h.insert(HEADER_PAYMENT_REQUIRED, HeaderValue::from_static("true"));
    if let Ok(v) = HeaderValue::from_str(&price.to_string()) {
        let _ = h.insert(HEADER_PAYMENT_AMOUNT, v);
    }
    if let Ok(v) = HeaderValue::from_str(&TOKEN_DECIMALS.to_string()) {
        let _ = h.insert(HEADER_PAYMENT_DECIMALS, v);
    }
    if let Ok(v) = HeaderValue::from_str(&config.tempo.accepted_token) {
        let _ = h.insert(HEADER_PAYMENT_TOKEN, v);
    }
    if let Ok(v) = HeaderValue::from_str(provider_addr) {
        let _ = h.insert(HEADER_PAYMENT_RECIPIENT, v);
    }
    if let Ok(v) = HeaderValue::from_str(&format!("tempo-{}", config.tempo.network)) {
        let _ = h.insert(HEADER_PAYMENT_NETWORK, v);
    }
    if let Ok(v) = HeaderValue::from_str(&config.tempo.chain_id.to_string()) {
        let _ = h.insert(HEADER_PAYMENT_CHAIN_ID, v);
    }
    if let Ok(v) = HeaderValue::from_str(&quote_id) {
        let _ = h.insert(HEADER_PAYMENT_QUOTE_ID_RESP, v);
    }
    if let Ok(v) = HeaderValue::from_str(&expires_at.timestamp().to_string()) {
        let _ = h.insert(HEADER_PAYMENT_QUOTE_EXPIRES, v);
    }
    let _ = h.insert(
        HEADER_PAYMENT_METHODS,
        HeaderValue::from_static("direct,session"),
    );

    response
}

// Test 12: 402 response format
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use crate::rate_limit::RateLimiter;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_402_response_format() {
        let db_path = format!("/tmp/paygate_test_{}.db", uuid::Uuid::new_v4());
        let (db_reader, db_writer) = crate::db::init_db(&db_path).unwrap();
        let config = Config {
            gateway: GatewayConfig {
                listen: "127.0.0.1:0".into(),
                admin_listen: "127.0.0.1:0".into(),
                upstream: "http://localhost:9999".into(),
                upstream_timeout_seconds: 30,
                max_response_body_bytes: 10_485_760,
            },
            tempo: TempoConfig {
                network: "testnet".into(),
                rpc_urls: vec!["http://localhost:1".into()],
                failover_timeout_ms: 2000,
                rpc_pool_max_idle: 10,
                rpc_timeout_ms: 5000,
                chain_id: 12345,
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
                endpoints: HashMap::new(),
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
            prometheus_handle: metrics_exporter_prometheus::PrometheusBuilder::new()
                .build_recorder()
                .handle(),
            started_at: std::time::Instant::now(),
        };

        let resp = payment_required_response(&state, "POST /v1/test").await;
        assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);

        let headers = resp.headers();
        assert_eq!(headers.get(HEADER_PAYMENT_REQUIRED).unwrap(), "true");
        assert_eq!(headers.get(HEADER_PAYMENT_AMOUNT).unwrap(), "1000");
        assert_eq!(headers.get(HEADER_PAYMENT_DECIMALS).unwrap(), "6");
        assert!(headers.get(HEADER_PAYMENT_QUOTE_ID_RESP).is_some());
        assert!(headers.get(HEADER_PAYMENT_QUOTE_EXPIRES).is_some());
        assert_eq!(headers.get(HEADER_PAYMENT_METHODS).unwrap(), "direct,session");

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "payment_required");
        assert!(json["help_url"].as_str().unwrap().contains("paygate.dev"));
        assert!(json["pricing"]["quote_id"].as_str().unwrap().starts_with("qt_"));
        assert_eq!(json["pricing"]["amount_base_units"], 1000);
        assert_eq!(json["pricing"]["decimals"], 6);
    }
}
