use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use serde::Deserialize;
use serde_json::json;

use crate::server::AppState;
use paygate_common::types::{format_amount, format_usd, TOKEN_DECIMALS};

pub fn admin_router(state: AppState) -> axum::Router {
    axum::Router::new()
        .route("/paygate/health", get(health_handler))
        .route("/paygate/metrics", get(metrics_handler))
        .route("/paygate/receipts/{tx_hash}", get(receipt_handler))
        .with_state(state)
}

/// Receipt route for the main gateway router (public port).
pub fn receipt_route() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/paygate/receipts/{tx_hash}", get(receipt_handler))
}

/// Transactions route for the main gateway router (public port).
pub fn transactions_route() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/paygate/transactions", get(transactions_handler))
}

async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let config = state.current_config();

    // Check DB
    let db_status = match state.db_reader.active_quote_count() {
        Ok(_) => "ok",
        Err(_) => "error",
    };

    // Check Tempo RPC
    let rpc_status = check_rpc(&state.http_client, &config.tempo.rpc_urls).await;

    // Check upstream
    let upstream_status = check_upstream(&state.http_client, &config.gateway.upstream).await;

    // Count active sessions
    let active_sessions = state
        .db_reader
        .active_session_count()
        .unwrap_or(0);

    // Overall status
    let overall = if db_status == "ok"
        && rpc_status == "connected"
        && upstream_status == "reachable"
    {
        "healthy"
    } else {
        "degraded"
    };

    let status_code = if overall == "healthy" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status_code,
        Json(json!({
            "status": overall,
            "tempo_rpc": rpc_status,
            "upstream": upstream_status,
            "active_sessions": active_sessions,
            "db": db_status,
        })),
    )
}

async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    let output = state.prometheus_handle.render();
    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
        output,
    )
}

async fn receipt_handler(
    State(state): State<AppState>,
    Path(tx_hash): Path<String>,
) -> impl IntoResponse {
    // Validate tx_hash format
    if !tx_hash.starts_with("0x") || tx_hash.len() != 66 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid tx_hash format"})),
        );
    }

    match state.db_reader.get_payment(&tx_hash) {
        Ok(Some(payment)) => (
            StatusCode::OK,
            Json(json!({
                "tx_hash": payment.tx_hash,
                "payer_address": payment.payer_address,
                "amount": payment.amount,
                "verified_at": payment.verified_at,
                "status": payment.status,
            })),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "payment not found"})),
        ),
        Err(e) => {
            tracing::error!(tx_hash = %tx_hash, error = %e, "receipt lookup failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal error"})),
            )
        }
    }
}

#[derive(Deserialize)]
struct TransactionsQuery {
    #[serde(default = "default_limit")]
    limit: u32,
    #[serde(default)]
    offset: u32,
}

fn default_limit() -> u32 {
    20
}

async fn transactions_handler(
    State(state): State<AppState>,
    Query(params): Query<TransactionsQuery>,
) -> impl IntoResponse {
    let limit = params.limit.min(100);
    let offset = params.offset;

    let transactions = match state.db_reader.recent_transactions(limit, offset) {
        Ok(txs) => txs,
        Err(e) => {
            tracing::error!(error = %e, "recent_transactions query failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("Access-Control-Allow-Origin", "https://ssreeni1.github.io")],
                Json(json!({"error": "internal error"})),
            );
        }
    };

    let (total, total_revenue) = match state.db_reader.transaction_stats() {
        Ok(stats) => stats,
        Err(e) => {
            tracing::error!(error = %e, "transaction_stats query failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("Access-Control-Allow-Origin", "https://ssreeni1.github.io")],
                Json(json!({"error": "internal error"})),
            );
        }
    };

    let tx_json: Vec<serde_json::Value> = transactions
        .iter()
        .map(|tx| {
            let amount_formatted = format!(
                "{}.{:06}",
                tx.amount / 1_000_000,
                tx.amount % 1_000_000
            );
            let verified_at_iso = chrono::DateTime::from_timestamp(tx.verified_at, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default();
            let explorer_url = format!(
                "https://explore.moderato.tempo.xyz/tx/{}",
                tx.tx_hash
            );
            json!({
                "tx_hash": tx.tx_hash,
                "payer_address": tx.payer_address,
                "amount": tx.amount,
                "amount_formatted": amount_formatted,
                "endpoint": tx.endpoint,
                "verified_at": tx.verified_at,
                "verified_at_iso": verified_at_iso,
                "status": tx.status,
                "explorer_url": explorer_url,
            })
        })
        .collect();

    let revenue_dollars = total_revenue / 1_000_000;
    let revenue_cents = (total_revenue % 1_000_000) / 10_000;
    let total_revenue_formatted = format!("${revenue_dollars}.{revenue_cents:02}");

    (
        StatusCode::OK,
        [("Access-Control-Allow-Origin", "https://ssreeni1.github.io")],
        Json(json!({
            "transactions": tx_json,
            "total": total,
            "total_revenue": total_revenue,
            "total_revenue_formatted": total_revenue_formatted,
        })),
    )
}

async fn check_rpc(client: &reqwest::Client, rpc_urls: &[String]) -> &'static str {
    for url in rpc_urls {
        let body = json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        });
        match client
            .post(url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => return "connected",
            _ => continue,
        }
    }
    "error"
}

async fn check_upstream(client: &reqwest::Client, upstream: &str) -> &'static str {
    match client
        .head(upstream)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
    {
        Ok(_) => "reachable",
        Err(_) => "unreachable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use crate::config::*;
    use crate::rate_limit::RateLimiter;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tower::ServiceExt;

    async fn make_test_state(rpc_url: &str, upstream_url: &str) -> (AppState, String) {
        let db_path = format!("/tmp/paygate_admin_test_{}.db", uuid::Uuid::new_v4());
        let (db_reader, db_writer) = crate::db::init_db(&db_path).unwrap();
        let config = Config {
            gateway: GatewayConfig {
                listen: "127.0.0.1:0".into(),
                admin_listen: "127.0.0.1:0".into(),
                upstream: upstream_url.to_string(),
                upstream_timeout_seconds: 5,
                max_response_body_bytes: 10_485_760,
            },
            tempo: TempoConfig {
                network: "testnet".into(),
                rpc_urls: vec![rpc_url.to_string()],
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
                    m.insert("POST /v1/chat/completions".into(), "0.005".into());
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
            prometheus_handle: metrics_exporter_prometheus::PrometheusBuilder::new()
                .build_recorder()
                .handle(),
            started_at: std::time::Instant::now(),
        };
        (state, db_path)
    }

    /// Start a mock RPC server that responds to eth_blockNumber.
    async fn start_mock_rpc() -> String {
        let app = axum::Router::new().fallback(|body: String| async move {
            let req: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let resp = match method {
                "eth_blockNumber" => serde_json::json!({"jsonrpc":"2.0","result":"0x1","id":1}),
                _ => serde_json::json!({"jsonrpc":"2.0","result":null,"id":1}),
            };
            axum::Json(resp)
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());
        format!("http://{addr}")
    }

    /// Start a mock upstream that responds to HEAD.
    async fn start_mock_upstream() -> String {
        let app = axum::Router::new().fallback(|| async { "ok" });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());
        format!("http://{addr}")
    }

    fn insert_test_payment(path: &str, id: &str, tx_hash: &str, amount: i64, verified_at: i64) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute(
            "INSERT INTO payments (id, tx_hash, payer_address, amount, token_address, endpoint,
                                   block_number, verified_at, status)
             VALUES (?, ?, '0x9E2b000000000000000000000000000000000001', ?,
                     '0x1234000000000000000000000000000000000001', 'POST /v1/chat/completions',
                     100, ?, 'verified')",
            rusqlite::params![id, tx_hash, amount, verified_at],
        ).unwrap();
    }

    // 1. Health returns valid JSON structure when all deps are reachable
    #[tokio::test]
    async fn test_health_returns_json_structure() {
        let rpc_url = start_mock_rpc().await;
        let upstream_url = start_mock_upstream().await;
        let (state, db_path) = make_test_state(&rpc_url, &upstream_url).await;

        let app = admin_router(state);
        let req = Request::builder()
            .method("GET")
            .uri("/paygate/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "healthy");
        assert!(json.get("db").is_some());
        assert!(json.get("tempo_rpc").is_some());
        assert!(json.get("upstream").is_some());

        let _ = std::fs::remove_file(&db_path);
    }

    // 2. Health returns degraded when RPC is down
    #[tokio::test]
    async fn test_health_degraded_when_rpc_down() {
        let upstream_url = start_mock_upstream().await;
        let (state, db_path) = make_test_state("http://127.0.0.1:1", &upstream_url).await;

        let app = admin_router(state);
        let req = Request::builder()
            .method("GET")
            .uri("/paygate/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "degraded");
        assert_eq!(json["tempo_rpc"], "error");

        let _ = std::fs::remove_file(&db_path);
    }

    // 3. Metrics returns text/plain
    #[tokio::test]
    async fn test_metrics_returns_text_plain() {
        let (state, db_path) = make_test_state("http://127.0.0.1:1", "http://127.0.0.1:1").await;

        let app = admin_router(state);
        let req = Request::builder()
            .method("GET")
            .uri("/paygate/metrics")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(ct.contains("text/plain"), "content-type should be text/plain, got {ct}");

        let _ = std::fs::remove_file(&db_path);
    }

    // 4. Transactions returns JSON array with inserted payments
    #[tokio::test]
    async fn test_transactions_returns_json_array() {
        let (state, db_path) = make_test_state("http://127.0.0.1:1", "http://127.0.0.1:1").await;
        insert_test_payment(&db_path, "p1", "0xaaa1", 1000, 100);
        insert_test_payment(&db_path, "p2", "0xaaa2", 2000, 200);

        let app = transactions_route().with_state(state);
        let req = Request::builder()
            .method("GET")
            .uri("/paygate/transactions")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["transactions"].as_array().unwrap().len(), 2);
        assert_eq!(json["total"], 2);

        let _ = std::fs::remove_file(&db_path);
    }

    // 5. Transactions respects limit parameter
    #[tokio::test]
    async fn test_transactions_limit_parameter() {
        let (state, db_path) = make_test_state("http://127.0.0.1:1", "http://127.0.0.1:1").await;
        for i in 0..5 {
            insert_test_payment(
                &db_path,
                &format!("pl{i}"),
                &format!("0xll{i:02}"),
                1000,
                100 + i,
            );
        }

        let app = transactions_route().with_state(state);
        let req = Request::builder()
            .method("GET")
            .uri("/paygate/transactions?limit=2")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["transactions"].as_array().unwrap().len(), 2);
        assert_eq!(json["total"], 5);

        let _ = std::fs::remove_file(&db_path);
    }

    // 6. Transactions on empty DB
    #[tokio::test]
    async fn test_transactions_empty_db() {
        let (state, db_path) = make_test_state("http://127.0.0.1:1", "http://127.0.0.1:1").await;

        let app = transactions_route().with_state(state);
        let req = Request::builder()
            .method("GET")
            .uri("/paygate/transactions")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["transactions"].as_array().unwrap().is_empty());
        assert_eq!(json["total"], 0);

        let _ = std::fs::remove_file(&db_path);
    }

    // 7. Receipt returns 404 for unknown tx hash
    #[tokio::test]
    async fn test_receipt_404_for_unknown_tx() {
        let (state, db_path) = make_test_state("http://127.0.0.1:1", "http://127.0.0.1:1").await;

        let app = receipt_route().with_state(state);
        // Need a valid-format tx hash (0x + 64 hex chars)
        let req = Request::builder()
            .method("GET")
            .uri("/paygate/receipts/0x0000000000000000000000000000000000000000000000000000000000000000")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "payment not found");

        let _ = std::fs::remove_file(&db_path);
    }
}
