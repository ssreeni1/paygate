use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use alloy_primitives::{Address, B256};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use serde_json::json;
use tracing::{error, info, warn};

use crate::config::parse_price_to_base_units;
use crate::db::FullSessionRecord;
use crate::server::AppState;
use crate::verifier;
use paygate_common::hash;
use paygate_common::mpp;
use paygate_common::types::*;

type HmacSha256 = Hmac<Sha256>;

pub struct SessionDeduction {
    pub session_id: String,
    pub payer_address: String,
    pub amount_deducted: u64,
    pub remaining_balance: u64,
}

#[derive(Debug)]
pub enum SessionError {
    MissingHeaders,
    SessionNotFound,
    SessionExpired,
    InsufficientBalance { balance: u64, rate: u64 },
    InvalidSignature,
    StaleTimestamp,
    DeductionFailed,
}

// ─── POST /paygate/sessions/nonce ──────────────────────────────────────────

pub async fn handle_nonce(State(state): State<AppState>, req: Request) -> Response {
    let payer = match req.headers().get(mpp::HEADER_PAYMENT_PAYER).and_then(|v| v.to_str().ok()) {
        Some(p) => p.to_string(),
        None => {
            return (StatusCode::PAYMENT_REQUIRED, Json(json!({
                "error": "missing_payer",
                "message": "X-Payment-Payer header is required"
            }))).into_response();
        }
    };

    // Validate address format
    if !payer.starts_with("0x") || payer.len() != 42 || hex::decode(&payer[2..]).is_err() {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "error": "invalid_payer",
            "message": "X-Payment-Payer must be a valid 0x-prefixed Ethereum address"
        }))).into_response();
    }

    let config = state.current_config();

    // Check concurrent session limit
    let active = match state.db_reader.count_active_sessions_for_payer(&payer) {
        Ok(n) => n,
        Err(e) => {
            warn!(error = %e, "failed to count active sessions");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "internal error"}))).into_response();
        }
    };

    if active >= config.sessions.max_concurrent_per_payer as u64 {
        return (StatusCode::TOO_MANY_REQUESTS, Json(json!({
            "error": "max_concurrent_sessions",
            "message": "Maximum concurrent sessions exceeded",
            "limit": config.sessions.max_concurrent_per_payer
        }))).into_response();
    }

    // Generate nonce
    let nonce = format!("nonce_{}", uuid::Uuid::new_v4());
    let now = chrono::Utc::now();
    let expires_at = now.timestamp() + 300; // 5 minutes

    if let Err(e) = state.db_writer.create_session_nonce(payer, nonce.clone(), expires_at).await {
        warn!(error = %e, "failed to create session nonce");
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "internal error"}))).into_response();
    }

    let expires_at_iso = chrono::DateTime::from_timestamp(expires_at, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default();

    (StatusCode::OK, Json(json!({
        "nonce": nonce,
        "expiresAt": expires_at_iso
    }))).into_response()
}

// ─── POST /paygate/sessions ────────────────────────────────────────────────

pub async fn handle_create_session(State(state): State<AppState>, req: Request) -> Response {
    let tx_hash = match req.headers().get(mpp::HEADER_PAYMENT_TX).and_then(|v| v.to_str().ok()) {
        Some(t) => t.to_string(),
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error": "missing_tx",
                "message": "X-Payment-Tx header is required"
            }))).into_response();
        }
    };

    let payer = match req.headers().get(mpp::HEADER_PAYMENT_PAYER).and_then(|v| v.to_str().ok()) {
        Some(p) => p.to_string(),
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error": "missing_payer",
                "message": "X-Payment-Payer header is required"
            }))).into_response();
        }
    };

    // Parse body for nonce
    let body_bytes = match axum::body::to_bytes(req.into_body(), 4096).await {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid body"}))).into_response();
        }
    };

    let body_json: serde_json::Value = match serde_json::from_slice(&body_bytes) {
        Ok(v) => v,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid JSON body"}))).into_response();
        }
    };

    let nonce = match body_json.get("nonce").and_then(|n| n.as_str()) {
        Some(n) => n.to_string(),
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error": "missing_nonce",
                "message": "Request body must include 'nonce' field"
            }))).into_response();
        }
    };

    // Look up nonce — wait briefly for DB writer to flush
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let nonce_record = match state.db_reader.get_session_nonce(&nonce) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error": "invalid_nonce",
                "message": "Nonce not found or expired"
            }))).into_response();
        }
        Err(e) => {
            warn!(error = %e, "nonce lookup failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "internal error"}))).into_response();
        }
    };

    let now = chrono::Utc::now().timestamp();

    if nonce_record.expires_at <= now {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "error": "invalid_nonce",
            "message": "Nonce not found or expired"
        }))).into_response();
    }

    if nonce_record.consumed {
        return (StatusCode::CONFLICT, Json(json!({
            "error": "nonce_consumed",
            "message": "Nonce already used"
        }))).into_response();
    }

    if nonce_record.payer_address.to_lowercase() != payer.to_lowercase() {
        return (StatusCode::FORBIDDEN, Json(json!({
            "error": "payer_mismatch",
            "message": "Payer address does not match nonce"
        }))).into_response();
    }

    // Re-check concurrent session limit (also checked at nonce time, but re-check here
    // to prevent bypass via parallel nonce minting)
    let config = state.current_config();
    let active = state.db_reader.count_active_sessions_for_payer(&payer).unwrap_or(0);
    if active >= config.sessions.max_concurrent_per_payer as u64 {
        return (StatusCode::TOO_MANY_REQUESTS, Json(json!({
            "error": "max_concurrent_sessions",
            "message": "Maximum concurrent sessions exceeded",
            "limit": config.sessions.max_concurrent_per_payer
        }))).into_response();
    }

    // Verify on-chain deposit

    let receipt_val = match verifier::rpc_call(
        &state.http_client,
        &config.tempo.rpc_urls,
        config.tempo.rpc_timeout_ms,
        "eth_getTransactionReceipt",
        serde_json::json!([&tx_hash]),
    ).await {
        Ok(v) if v.is_null() => {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error": "tx_not_found",
                "message": "Transaction not yet indexed"
            }))).into_response();
        }
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({
                "error": "rpc_error",
                "message": format!("RPC error: {e}")
            }))).into_response();
        }
    };

    let logs = receipt_val.get("logs")
        .and_then(|l| l.as_array())
        .cloned()
        .unwrap_or_default();

    // Decode transfer
    let provider_address: Address = match config.provider.address.parse() {
        Ok(a) => a,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "invalid provider config"}))).into_response();
        }
    };
    let accepted_token: Address = match config.tempo.accepted_token.parse() {
        Ok(a) => a,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "invalid token config"}))).into_response();
        }
    };

    let transfer = match verifier::decode_transfer_events(&logs, &provider_address, &accepted_token) {
        Ok(t) => t,
        Err(vr) => {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error": "invalid_deposit",
                "message": format!("Deposit verification failed: {}", vr.step_name())
            }))).into_response();
        }
    };

    // Verify payer matches
    let expected_payer: Address = match payer.parse() {
        Ok(a) => a,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid payer address"}))).into_response();
        }
    };
    if transfer.from != expected_payer {
        return (StatusCode::FORBIDDEN, Json(json!({
            "error": "payer_mismatch",
            "message": "Transaction sender does not match X-Payment-Payer"
        }))).into_response();
    }

    // Guard against amounts that would overflow SQLite's signed INTEGER
    if transfer.amount > i64::MAX as u64 {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "error": "invalid_deposit",
            "message": "Deposit amount exceeds maximum"
        }))).into_response();
    }

    // Verify minimum deposit
    let min_deposit = parse_price_to_base_units(&config.sessions.minimum_deposit).unwrap_or(50_000);
    if transfer.amount < min_deposit {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "error": "insufficient_deposit",
            "message": format!("Minimum deposit is {} USDC", format_amount(min_deposit, TOKEN_DECIMALS))
        }))).into_response();
    }

    // Verify memo = keccak256("paygate-session" || nonce)
    let on_chain_memo = match verifier::decode_memo_from_logs(&logs) {
        Ok(m) => m,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error": "invalid_memo",
                "message": "No TransferWithMemo event found"
            }))).into_response();
        }
    };

    let expected_memo = hash::session_deposit_memo(&nonce);
    if on_chain_memo != expected_memo {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "error": "memo_mismatch",
            "message": "Deposit memo does not match expected session nonce memo"
        }))).into_response();
    }

    // Check replay
    if let Ok(true) = state.db_reader.is_tx_consumed(&tx_hash) {
        return (StatusCode::CONFLICT, Json(json!({
            "error": "replay_detected",
            "message": "Transaction already used"
        }))).into_response();
    }

    // Generate session credentials
    let session_id = format!("sess_{}", hex::encode(rand::random::<[u8; 32]>()));
    let session_secret = format!("ssec_{}", hex::encode(rand::random::<[u8; 32]>()));

    let rate_per_request = parse_price_to_base_units(&config.pricing.default_price).unwrap_or(1000);
    let expires_at = now + (config.sessions.max_duration_hours as i64 * 3600);

    let session = FullSessionRecord {
        id: session_id.clone(),
        secret: session_secret.clone(),
        payer_address: payer.clone(),
        deposit_tx: tx_hash.clone(),
        nonce: nonce.clone(),
        deposit_amount: transfer.amount,
        balance: transfer.amount,
        rate_per_request,
        requests_made: 0,
        created_at: now,
        expires_at,
        status: "active".to_string(),
    };

    let payment = PaymentRecord {
        id: uuid::Uuid::new_v4().to_string(),
        tx_hash: tx_hash.clone(),
        payer_address: payer.clone(),
        amount: transfer.amount,
        token_address: config.tempo.accepted_token.clone(),
        endpoint: "session_deposit".to_string(),
        request_hash: None,
        quote_id: None,
        block_number: 0,
        verified_at: now,
        status: "verified".to_string(),
    };

    if let Err(e) = state.db_writer.create_session(session, payment).await {
        warn!(error = %e, "failed to create session");
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "failed to create session"}))).into_response();
    }

    let balance_str = format_amount(transfer.amount, TOKEN_DECIMALS);
    let rate_str = format_amount(rate_per_request, TOKEN_DECIMALS);
    let expires_iso = chrono::DateTime::from_timestamp(expires_at, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default();

    info!(session_id = %session_id, payer = %payer, deposit = transfer.amount, "session created");

    (StatusCode::CREATED, Json(json!({
        "sessionId": session_id,
        "sessionSecret": session_secret,
        "balance": balance_str,
        "ratePerRequest": rate_str,
        "expiresAt": expires_iso
    }))).into_response()
}

// ─── verify_and_deduct — called from gateway_handler ────────────────────────

pub async fn verify_and_deduct(
    state: &AppState,
    headers: &HeaderMap,
    request_hash: &B256,
    endpoint: &str,
) -> Result<SessionDeduction, SessionError> {
    // 1. Extract headers
    let session_id = headers
        .get(mpp::HEADER_PAYMENT_SESSION)
        .and_then(|v| v.to_str().ok())
        .ok_or(SessionError::MissingHeaders)?;

    let sig_header = headers
        .get(mpp::HEADER_PAYMENT_SESSION_SIG)
        .and_then(|v| v.to_str().ok())
        .ok_or(SessionError::MissingHeaders)?;

    let timestamp_str = headers
        .get(mpp::HEADER_PAYMENT_TIMESTAMP)
        .and_then(|v| v.to_str().ok())
        .ok_or(SessionError::MissingHeaders)?;

    // 2. Look up session
    let session = state.db_reader.get_session(session_id)
        .map_err(|_| SessionError::DeductionFailed)?
        .ok_or(SessionError::SessionNotFound)?;

    if session.status != "active" {
        return Err(SessionError::SessionExpired);
    }

    let now = chrono::Utc::now().timestamp();
    if session.expires_at <= now {
        return Err(SessionError::SessionExpired);
    }

    // 3. Verify timestamp freshness
    let ts: i64 = timestamp_str.parse().map_err(|_| SessionError::StaleTimestamp)?;
    if (now - ts).unsigned_abs() > 60 {
        return Err(SessionError::StaleTimestamp);
    }

    // 4. HMAC verification (constant-time via hmac crate)
    // Strip ssec_ prefix and hex-decode to get raw key bytes (must match TS SDK)
    let raw_secret = session.secret.strip_prefix("ssec_").unwrap_or(&session.secret);
    let key_bytes = hex::decode(raw_secret).map_err(|_| SessionError::InvalidSignature)?;
    let rh_hex = format!("0x{}", hex::encode(request_hash.as_slice()));
    let mut mac = HmacSha256::new_from_slice(&key_bytes)
        .map_err(|_| SessionError::InvalidSignature)?;
    mac.update(rh_hex.as_bytes());
    mac.update(timestamp_str.as_bytes());

    let sig_hex = sig_header.strip_prefix("0x").unwrap_or(sig_header);
    let sig_bytes = hex::decode(sig_hex).map_err(|_| SessionError::InvalidSignature)?;
    mac.verify_slice(&sig_bytes).map_err(|_| SessionError::InvalidSignature)?;

    // 5. Determine rate for this endpoint
    let config = state.current_config();
    let rate = config.price_for_endpoint(endpoint);

    // 6. Atomically deduct
    let deducted = state.db_writer.deduct_session_balance(&session.id, rate)
        .await
        .map_err(|_| SessionError::DeductionFailed)?;

    if !deducted {
        return Err(SessionError::InsufficientBalance {
            balance: session.balance,
            rate,
        });
    }

    Ok(SessionDeduction {
        session_id: session.id.clone(),
        payer_address: session.payer_address.clone(),
        amount_deducted: rate,
        remaining_balance: session.balance.saturating_sub(rate),
    })
}

/// Deduct additional amount from session balance (used for dynamic pricing adjustment).
pub async fn deduct_additional(state: &AppState, session_id: &str, amount: u64) -> Result<bool, crate::db::DbError> {
    state.db_writer.deduct_session_balance(session_id, amount).await
}

// ─── GET /paygate/sessions ───────────────────────────────────────────────

pub async fn handle_get_sessions(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let payer = match params.get("payer") {
        Some(p) if !p.is_empty() => p,
        _ => {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error": "payer_required",
                "message": "Query parameter 'payer' is required"
            }))).into_response();
        }
    };

    let sessions = match state.db_reader.list_sessions_for_payer(payer) {
        Ok(s) => s,
        Err(e) => {
            error!(error = %e, payer = %payer, "failed to list sessions");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "internal error"}))).into_response();
        }
    };

    let active: Vec<_> = sessions.into_iter().filter(|s| s.status == "active").collect();

    let session_json: Vec<serde_json::Value> = active.iter().map(|s| {
        json!({
            "sessionId": s.id,
            "balance": format!("{:.6}", s.balance as f64 / 1_000_000.0),
            "ratePerRequest": format!("{:.6}", s.rate_per_request as f64 / 1_000_000.0),
            "requestsMade": s.requests_made,
            "expiresAt": chrono::DateTime::from_timestamp(s.expires_at, 0)
                .map(|d| d.to_rfc3339()).unwrap_or_default(),
            "status": s.status,
        })
    }).collect();

    Json(json!({
        "sessions": session_json,
    })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use crate::rate_limit::RateLimiter;
    use rusqlite::{params, Connection};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn test_config() -> Config {
        Config {
            gateway: GatewayConfig {
                listen: "127.0.0.1:0".into(),
                admin_listen: "127.0.0.1:0".into(),
                upstream: "http://localhost:9999".into(),
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
            sessions: SessionsConfig {
                max_concurrent_per_payer: 5,
                minimum_deposit: "0.05".into(),
                max_duration_hours: 24,
                ..Default::default()
            },
            pricing: PricingConfig {
                default_price: "0.001".into(),
                quote_ttl_seconds: 300,
                endpoints: HashMap::new(),
                no_charge_on_5xx: vec!["POST /v1/summarize".into()],
                ..Default::default()
            },
            rate_limiting: Default::default(),
            security: Default::default(),
            webhooks: Default::default(),
            storage: Default::default(),
        }
    }

    async fn test_state() -> (AppState, String) {
        let db_path = format!("/tmp/paygate_sess_test_{}.db", uuid::Uuid::new_v4());
        let (db_reader, db_writer) = crate::db::init_db(&db_path).unwrap();
        let state = AppState {
            config: Arc::new(arc_swap::ArcSwap::new(Arc::new(test_config()))),
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

    fn insert_session(path: &str, id: &str, secret: &str, payer: &str, balance: i64, expires_at: i64) {
        let conn = Connection::open(path).unwrap();
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO sessions (id, secret, payer_address, deposit_tx, nonce,
             deposit_amount, balance, rate_per_request, requests_made,
             created_at, expires_at, status)
             VALUES (?, ?, ?, 'tx_dep', 'nonce_test', 50000, ?, 1000, 0, ?, ?, 'active')",
            params![id, secret, payer, balance, now, expires_at],
        ).unwrap();
    }

    fn insert_sessions_for_payer(path: &str, payer: &str, count: u32) {
        let conn = Connection::open(path).unwrap();
        let now = chrono::Utc::now().timestamp();
        let future = now + 86400;
        for i in 0..count {
            conn.execute(
                "INSERT INTO sessions (id, secret, payer_address, deposit_tx, nonce,
                 deposit_amount, balance, rate_per_request, requests_made,
                 created_at, expires_at, status)
                 VALUES (?, 'sec', ?, ?, ?, 50000, 50000, 1000, 0, ?, ?, 'active')",
                params![format!("sess_{i}"), payer, format!("tx_dep_{i}"), format!("nonce_{i}"), now, future],
            ).unwrap();
        }
    }

    // Test 1: Nonce generation happy path
    #[tokio::test]
    async fn test_nonce_happy_path() {
        let (state, _db) = test_state().await;
        let app = axum::Router::new()
            .route("/paygate/sessions/nonce", axum::routing::post(handle_nonce))
            .with_state(state);

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/paygate/sessions/nonce")
            .header("X-Payment-Payer", "0x9E2b000000000000000000000000000000000001")
            .body(Body::empty())
            .unwrap();

        let resp = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap(),
        ).unwrap();
        assert!(body["nonce"].as_str().unwrap().starts_with("nonce_"));
        assert!(body["expiresAt"].as_str().is_some());
    }

    // Test 2: Max concurrent sessions exceeded
    #[tokio::test]
    async fn test_max_concurrent_exceeded() {
        let (state, db_path) = test_state().await;
        let payer = "0x9E2b000000000000000000000000000000000001";
        insert_sessions_for_payer(&db_path, payer, 5);

        let app = axum::Router::new()
            .route("/paygate/sessions/nonce", axum::routing::post(handle_nonce))
            .with_state(state);

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/paygate/sessions/nonce")
            .header("X-Payment-Payer", payer)
            .body(Body::empty())
            .unwrap();

        let resp = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap(),
        ).unwrap();
        assert_eq!(body["error"], "max_concurrent_sessions");
    }

    // Test 4: Create session with invalid nonce
    #[tokio::test]
    async fn test_create_session_invalid_nonce() {
        let (state, _db) = test_state().await;
        let app = axum::Router::new()
            .route("/paygate/sessions", axum::routing::post(handle_create_session))
            .with_state(state);

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/paygate/sessions")
            .header("X-Payment-Tx", "0xabc123")
            .header("X-Payment-Payer", "0x9E2b000000000000000000000000000000000001")
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"nonce": "nonce_nonexistent"}"#))
            .unwrap();

        let resp = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap(),
        ).unwrap();
        assert_eq!(body["error"], "invalid_nonce");
    }

    // Test 5: HMAC verification happy path
    #[tokio::test]
    async fn test_hmac_verification_happy_path() {
        let (state, db_path) = test_state().await;

        let session_secret = "ssec_deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
        let session_id = "sess_test_hmac";
        let payer = "0x9E2b000000000000000000000000000000000001";
        let future = chrono::Utc::now().timestamp() + 86400;
        insert_session(&db_path, session_id, session_secret, payer, 50000, future);

        let rh = B256::repeat_byte(0xAB);
        let ts = chrono::Utc::now().timestamp().to_string();

        // Compute valid HMAC (must match production: strip ssec_ prefix, hex-decode key)
        let raw_secret = session_secret.strip_prefix("ssec_").unwrap();
        let key_bytes = hex::decode(raw_secret).unwrap();
        let rh_hex = format!("0x{}", hex::encode(rh.as_slice()));
        let mut mac = HmacSha256::new_from_slice(&key_bytes).unwrap();
        mac.update(rh_hex.as_bytes());
        mac.update(ts.as_bytes());
        let sig = hex::encode(mac.finalize().into_bytes());

        let mut headers = HeaderMap::new();
        headers.insert("x-payment-session", session_id.parse().unwrap());
        headers.insert("x-payment-session-sig", sig.parse().unwrap());
        headers.insert("x-payment-timestamp", ts.parse().unwrap());

        let result = verify_and_deduct(&state, &headers, &rh, "POST /v1/chat").await;
        assert!(result.is_ok(), "HMAC verification should succeed");

        let deduction = result.unwrap();
        assert_eq!(deduction.session_id, session_id);
        assert_eq!(deduction.payer_address, payer);
        assert_eq!(deduction.amount_deducted, 1000); // default price
    }

    // Test 6: Invalid HMAC
    #[tokio::test]
    async fn test_invalid_hmac() {
        let (state, db_path) = test_state().await;

        let session_id = "sess_test_bad_hmac";
        let session_secret = "ssec_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let payer = "0x9E2b000000000000000000000000000000000001";
        let future = chrono::Utc::now().timestamp() + 86400;
        insert_session(&db_path, session_id, session_secret, payer, 50000, future);

        let rh = B256::repeat_byte(0xCC);
        let ts = chrono::Utc::now().timestamp().to_string();

        let mut headers = HeaderMap::new();
        headers.insert("x-payment-session", session_id.parse().unwrap());
        headers.insert("x-payment-session-sig", "bad_signature".parse().unwrap());
        headers.insert("x-payment-timestamp", ts.parse().unwrap());

        let result = verify_and_deduct(&state, &headers, &rh, "POST /v1/chat").await;
        assert!(matches!(result, Err(SessionError::InvalidSignature)));
    }

    // Test 7: Stale timestamp
    #[tokio::test]
    async fn test_stale_timestamp() {
        let (state, db_path) = test_state().await;

        let session_id = "sess_test_stale";
        let session_secret = "ssec_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let payer = "0x9E2b000000000000000000000000000000000001";
        let future = chrono::Utc::now().timestamp() + 86400;
        insert_session(&db_path, session_id, session_secret, payer, 50000, future);

        let rh = B256::repeat_byte(0xDD);
        let old_ts = (chrono::Utc::now().timestamp() - 120).to_string(); // 2 minutes ago

        let rh_hex = format!("0x{}", hex::encode(rh.as_slice()));
        let mut mac = HmacSha256::new_from_slice(session_secret.as_bytes()).unwrap();
        mac.update(rh_hex.as_bytes());
        mac.update(old_ts.as_bytes());
        let sig = hex::encode(mac.finalize().into_bytes());

        let mut headers = HeaderMap::new();
        headers.insert("x-payment-session", session_id.parse().unwrap());
        headers.insert("x-payment-session-sig", sig.parse().unwrap());
        headers.insert("x-payment-timestamp", old_ts.parse().unwrap());

        let result = verify_and_deduct(&state, &headers, &rh, "POST /v1/chat").await;
        assert!(matches!(result, Err(SessionError::StaleTimestamp)));
    }

    // Test 8: Insufficient balance
    #[tokio::test]
    async fn test_insufficient_balance() {
        let (state, db_path) = test_state().await;

        let session_id = "sess_test_low_bal";
        let session_secret = "ssec_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let payer = "0x9E2b000000000000000000000000000000000001";
        let future = chrono::Utc::now().timestamp() + 86400;
        // Balance 500, rate 1000 → insufficient
        insert_session(&db_path, session_id, session_secret, payer, 500, future);

        let rh = B256::repeat_byte(0xEE);
        let ts = chrono::Utc::now().timestamp().to_string();

        let raw_secret = session_secret.strip_prefix("ssec_").unwrap();
        let key_bytes = hex::decode(raw_secret).unwrap();
        let rh_hex = format!("0x{}", hex::encode(rh.as_slice()));
        let mut mac = HmacSha256::new_from_slice(&key_bytes).unwrap();
        mac.update(rh_hex.as_bytes());
        mac.update(ts.as_bytes());
        let sig = hex::encode(mac.finalize().into_bytes());

        let mut headers = HeaderMap::new();
        headers.insert("x-payment-session", session_id.parse().unwrap());
        headers.insert("x-payment-session-sig", sig.parse().unwrap());
        headers.insert("x-payment-timestamp", ts.parse().unwrap());

        let result = verify_and_deduct(&state, &headers, &rh, "POST /v1/chat").await;
        assert!(matches!(result, Err(SessionError::InsufficientBalance { .. })));
    }

    // Test 9: Session expired
    #[tokio::test]
    async fn test_session_expired() {
        let (state, db_path) = test_state().await;

        let session_id = "sess_test_expired";
        let session_secret = "ssec_dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
        let payer = "0x9E2b000000000000000000000000000000000001";
        let past = chrono::Utc::now().timestamp() - 3600; // expired 1 hour ago
        insert_session(&db_path, session_id, session_secret, payer, 50000, past);

        let rh = B256::repeat_byte(0xFF);
        let ts = chrono::Utc::now().timestamp().to_string();

        let rh_hex = format!("0x{}", hex::encode(rh.as_slice()));
        let mut mac = HmacSha256::new_from_slice(session_secret.as_bytes()).unwrap();
        mac.update(rh_hex.as_bytes());
        mac.update(ts.as_bytes());
        let sig = hex::encode(mac.finalize().into_bytes());

        let mut headers = HeaderMap::new();
        headers.insert("x-payment-session", session_id.parse().unwrap());
        headers.insert("x-payment-session-sig", sig.parse().unwrap());
        headers.insert("x-payment-timestamp", ts.parse().unwrap());

        let result = verify_and_deduct(&state, &headers, &rh, "POST /v1/chat").await;
        assert!(matches!(result, Err(SessionError::SessionExpired)));
    }

    // Test 10: no_charge_on_5xx — verify config helper
    #[test]
    fn test_no_charge_on_5xx_config() {
        let config = test_config();
        assert!(config.is_no_charge_on_5xx("POST /v1/summarize"));
        assert!(!config.is_no_charge_on_5xx("POST /v1/search"));
    }

    // Test 11: compute_cost unit test
    #[test]
    fn test_compute_cost() {
        use crate::config::DynamicPricingConfig;

        let dpc = DynamicPricingConfig {
            enabled: true,
            formula: "token".into(),
            base_cost_per_token: "0.00001".into(),
            spread_per_token: "0.000005".into(),
            header_source: "X-Token-Count".into(),
        };

        // 100 tokens * 0.000015/token = 0.0015 USD = 1500 base units
        assert_eq!(dpc.compute_cost(100), 1500);

        // 5000 tokens * 0.000015/token = 0.075 USD = 75000 base units
        assert_eq!(dpc.compute_cost(5000), 75000);

        // 0 tokens = 0
        assert_eq!(dpc.compute_cost(0), 0);

        // 1 token = 15 base units
        assert_eq!(dpc.compute_cost(1), 15);
    }

    // Test 12: compute_cost with unparseable values falls back to 0
    #[test]
    fn test_compute_cost_invalid_config() {
        use crate::config::DynamicPricingConfig;

        let dpc = DynamicPricingConfig {
            enabled: true,
            formula: "token".into(),
            base_cost_per_token: "not_a_number".into(),
            spread_per_token: "".into(),
            header_source: "X-Token-Count".into(),
        };

        assert_eq!(dpc.compute_cost(100), 0);
    }

    // Test 13: deduct_additional helper
    #[tokio::test]
    async fn test_deduct_additional() {
        let (state, db_path) = test_state().await;

        let session_id = "sess_test_deduct_add";
        let secret = "ssec_eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let payer = "0x9E2b000000000000000000000000000000000001";
        let future = chrono::Utc::now().timestamp() + 86400;
        insert_session(&db_path, session_id, secret, payer, 50000, future);

        // Deduct additional 5000
        let result = deduct_additional(&state, session_id, 5000).await;
        assert!(result.is_ok());
        assert!(result.unwrap()); // should succeed

        // Verify balance decreased
        let session = state.db_reader.get_session(session_id).unwrap().unwrap();
        assert_eq!(session.balance, 45000);
    }

    // Test 14: list_sessions_for_payer
    #[test]
    fn test_list_sessions_for_payer() {
        let db_path = format!("/tmp/paygate_sess_test_{}.db", uuid::Uuid::new_v4());
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(include_str!("../../../schema.sql")).unwrap();

        let payer = "0x9E2b000000000000000000000000000000000001";
        let other = "0x1111000000000000000000000000000000000002";
        let now = chrono::Utc::now().timestamp();
        let future = now + 86400;

        // Insert 2 sessions for our payer, 1 for another
        conn.execute(
            "INSERT INTO sessions (id, secret, payer_address, deposit_tx, nonce,
             deposit_amount, balance, rate_per_request, requests_made,
             created_at, expires_at, status)
             VALUES ('sess_a', 'sec', ?1, 'tx1', 'n1', 50000, 30000, 1000, 5, ?2, ?3, 'active')",
            params![payer, now, future],
        ).unwrap();
        conn.execute(
            "INSERT INTO sessions (id, secret, payer_address, deposit_tx, nonce,
             deposit_amount, balance, rate_per_request, requests_made,
             created_at, expires_at, status)
             VALUES ('sess_b', 'sec', ?1, 'tx2', 'n2', 50000, 20000, 1000, 10, ?2, ?3, 'active')",
            params![payer, now, future],
        ).unwrap();
        conn.execute(
            "INSERT INTO sessions (id, secret, payer_address, deposit_tx, nonce,
             deposit_amount, balance, rate_per_request, requests_made,
             created_at, expires_at, status)
             VALUES ('sess_c', 'sec', ?1, 'tx3', 'n3', 50000, 40000, 1000, 2, ?2, ?3, 'active')",
            params![other, now, future],
        ).unwrap();
        drop(conn);

        let reader = crate::db::DbReader::new(&db_path);
        let sessions = reader.list_sessions_for_payer(payer).unwrap();
        assert_eq!(sessions.len(), 2);
        // Should not include the other payer's session
        assert!(sessions.iter().all(|s| s.payer_address == payer));

        std::fs::remove_file(&db_path).ok();
    }
}
