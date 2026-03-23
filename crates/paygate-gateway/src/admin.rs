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
