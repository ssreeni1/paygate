use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use paygate_common::types::VerificationResult;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info, warn};

use crate::db::DbReader;
use crate::mpp;
use crate::npm_resolver::{self, ResolveError};
use crate::server::AppState;
use crate::verifier;

// ─── Internal API auth ──────────────────────────────────────────────────────

fn verify_internal_auth(headers: &HeaderMap) -> Result<(), (StatusCode, &'static str)> {
    let secret = std::env::var("PAYGATE_INTERNAL_SECRET").unwrap_or_default();
    if secret.is_empty() {
        return Ok(());
    }
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if auth == format!("Bearer {secret}") {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, "Invalid internal API secret"))
    }
}

/// Log warning if internal secret is not configured. Call once at startup.
pub fn check_internal_secret() {
    let secret = std::env::var("PAYGATE_INTERNAL_SECRET").unwrap_or_default();
    if secret.is_empty() {
        warn!("PAYGATE_INTERNAL_SECRET is not set — internal API is open to all requests. Set this in production.");
    }
}

// ─── Constants ──────────────────────────────────────────────────────────────

const MAX_TIP_AMOUNT_USD: f64 = 100.0;
const MIN_TIP_AMOUNT_USD: f64 = 0.01;
const MAX_REASON_LEN: usize = 500;
const MAX_EVIDENCE_LEN: usize = 1000;
const MAX_BATCH_SIZE: usize = 50;

// ─── Request / Response types ───────────────────────────────────────────────

#[derive(Deserialize, Serialize, Clone)]
pub struct TipRequest {
    pub target: String,
    #[serde(alias = "amount")]
    pub amount_usd: f64,
    pub reason: String,
    pub evidence: Option<String>,
    pub sender_name: Option<String>,
}

#[derive(Serialize)]
pub struct TipResponse {
    pub receipt_url: String,
    pub recipient: String,
    pub resolved_github: String,
    pub status: String,
    pub tx_hash: Option<String>,
    pub tip_id: String,
}

#[derive(Deserialize, Serialize)]
pub struct TipBatchRequest {
    pub tips: Vec<TipRequest>,
    pub sender_name: Option<String>,
}

#[derive(Serialize)]
pub struct TipBatchResponse {
    pub results: Vec<TipBatchResult>,
    pub summary: TipBatchSummary,
}

#[derive(Serialize)]
pub struct TipBatchResult {
    pub target: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct TipBatchSummary {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub total_amount_usd: f64,
}

// ─── Internal API types (for Vercel web app) ────────────────────────────────

#[derive(Serialize)]
pub struct TipRecord {
    pub id: String,
    pub sender_wallet: String,
    pub sender_name: Option<String>,
    pub recipient_gh: String,
    pub package_name: Option<String>,
    pub amount_usdc: i64,
    pub reason: String,
    pub evidence: Option<String>,
    pub status: String,
    pub tx_hash: Option<String>,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct LeaderboardEntry {
    pub github_username: String,
    pub total_amount: i64,
    pub tip_count: i64,
    pub agent_count: i64,
}

// ─── Sanitization (FIX: UTF-8 safe truncation) ─────────────────────────────

fn sanitize_text(input: &str, max_len: usize) -> String {
    // Truncate at character boundary, not byte boundary
    let truncated = if input.chars().count() > max_len {
        match input.char_indices().nth(max_len) {
            Some((byte_idx, _)) => &input[..byte_idx],
            None => input,
        }
    } else {
        input
    };
    // Escape special characters (& must be first to avoid double-encoding)
    truncated
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn sanitize_for_markdown(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('`', "\\`")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

// ─── Tip ID generation (FIX: use UUID v4) ───────────────────────────────────

fn generate_tip_id() -> String {
    format!("tip_{}", uuid::Uuid::new_v4())
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn usdc_base_units(usd: f64) -> i64 {
    (usd * 1_000_000.0).round() as i64
}

/// Validate wallet address format: 0x + 40 hex chars
fn is_valid_wallet_address(addr: &str) -> bool {
    addr.len() == 42
        && addr.starts_with("0x")
        && addr[2..].chars().all(|c| c.is_ascii_hexdigit())
}

// ─── Resolve target to GitHub owner ─────────────────────────────────────────

struct ResolvedTarget {
    github_owner: String,
    package_name: Option<String>,
}

async fn resolve_target(
    http_client: &reqwest::Client,
    db_reader: &DbReader,
    target: &str,
) -> Result<ResolvedTarget, (StatusCode, String)> {
    let target = target.trim().to_lowercase();
    if target.starts_with('@') {
        Ok(ResolvedTarget {
            github_owner: target.trim_start_matches('@').to_string(),
            package_name: None,
        })
    } else {
        match npm_resolver::resolve_package(http_client, db_reader, &target).await {
            Ok(r) => Ok(ResolvedTarget {
                github_owner: r.github_owner,
                package_name: Some(target),
            }),
            Err(ResolveError::PackageNotFound) => {
                Err((StatusCode::NOT_FOUND, format!("Package '{target}' not found on npm")))
            }
            Err(ResolveError::NoRepository) => {
                Err((StatusCode::NOT_FOUND, format!("Package '{target}' has no repository field. Tip by GitHub username instead.")))
            }
            Err(ResolveError::NotGitHub) => {
                Err((StatusCode::NOT_FOUND, "Only GitHub-hosted packages supported.".to_string()))
            }
            Err(e) => {
                warn!(package = target, error = %e, "npm resolution failed");
                Err((StatusCode::SERVICE_UNAVAILABLE, "Package resolution temporarily unavailable. Tip by GitHub username instead.".to_string()))
            }
        }
    }
}

// ─── Wallet registry lookup ─────────────────────────────────────────────────

fn lookup_wallet(db_reader: &DbReader, github_username: &str) -> Option<String> {
    let conn = db_reader.conn_raw().ok()?;
    conn.query_row(
        "SELECT wallet_address FROM tip_registry WHERE github_username = ?",
        params![github_username.to_lowercase()],
        |row| row.get(0),
    )
    .ok()
}

// ─── GitHub org membership check ────────────────────────────────────────────

async fn is_org_member(http_client: &reqwest::Client, org: &str, username: &str) -> bool {
    let url = format!("https://api.github.com/orgs/{org}/public_members/{username}");
    match http_client
        .get(&url)
        .header("User-Agent", "agent-tips/0.6")
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(resp) => resp.status().as_u16() == 204,
        Err(_) => false,
    }
}

// ─── GitHub issue notification ──────────────────────────────────────────────

async fn notify_github_issue(
    client: &reqwest::Client,
    github_owner: &str,
    repo: Option<&str>,
    amount: f64,
    reason: &str,
    receipt_url: &str,
) {
    let github_token = std::env::var("GITHUB_TOKEN").unwrap_or_default();
    if github_token.is_empty() {
        info!(owner = github_owner, "GitHub notification skipped (no GITHUB_TOKEN)");
        return;
    }

    // Use the repo if known, otherwise try owner/owner (common for personal repos)
    let repo_path = if let Some(r) = repo {
        format!("{github_owner}/{r}")
    } else {
        format!("{github_owner}/{github_owner}")
    };

    let title = format!("An AI agent tipped you ${amount:.2} for your open source work");
    let body = format!(
        "An AI agent sent you a **${amount:.2} USDC** tip.\n\n\
         **Reason:** {reason}\n\n\
         **Claim your tip:** [{receipt_url}]({receipt_url})\n\n\
         ---\n\
         *This tip was sent via [Agent Tips](https://tips.paygate.fm). \
         If you don't have a wallet, the tip is held in escrow for 90 days.*"
    );

    let url = format!("https://api.github.com/repos/{repo_path}/issues");
    let result = client
        .post(&url)
        .header("Authorization", format!("Bearer {github_token}"))
        .header("User-Agent", "agent-tips/0.6")
        .header("Accept", "application/vnd.github+json")
        .json(&json!({
            "title": title,
            "body": body,
            "labels": ["agent-tip"]
        }))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await;

    match result {
        Ok(resp) if resp.status().is_success() => {
            info!(repo = repo_path, "GitHub issue created for tip notification");
            crate::metrics::record_tip_notification("success");
        }
        Ok(resp) => {
            let status = resp.status();
            warn!(repo = repo_path, status = %status, "GitHub issue creation failed");
            crate::metrics::record_tip_notification("error");
        }
        Err(e) => {
            warn!(repo = repo_path, error = %e, "GitHub issue request failed");
            crate::metrics::record_tip_notification("error");
        }
    }
}

// ─── Tip creation (with real payment verification) ──────────────────────────

/// Create a single tip. Requires verified payment via the MPP 402 flow.
///
/// Flow:
/// 1. POST /paygate/tip with tip details → 402 with price = tip amount
/// 2. Agent pays on-chain, retries with X-Payment-Tx header
/// 3. Gateway verifies payment, creates tip with real tx_hash
async fn create_tip_record(
    state: &AppState,
    req: &TipRequest,
    sender_wallet: &str,
    tx_hash: &str,
    receipt_base_url: &str,
    sender_name_override: Option<&str>,
) -> Result<TipResponse, (StatusCode, String)> {
    // Sanitize text fields
    let reason = sanitize_text(&req.reason, MAX_REASON_LEN);
    let evidence = req.evidence.as_deref().map(|e| sanitize_text(e, MAX_EVIDENCE_LEN));
    let sender_name = sender_name_override
        .or(req.sender_name.as_deref())
        .map(|s| sanitize_text(s, 100));

    // Resolve target
    let resolved = resolve_target(&state.http_client, &state.db_reader, &req.target).await?;

    let tip_id = generate_tip_id();
    let amount_base = usdc_base_units(req.amount_usd);
    let now = chrono::Utc::now();
    let expires = now + chrono::Duration::days(90);

    // All tips are "escrowed" — funds are in the gateway's wallet.
    // The claim flow triggers the outbound transfer.
    let status = "escrowed";

    // Insert via DbWriter channel (FIX: no more raw conn writes)
    let insert_result = state.db_writer.insert_tip(
        tip_id.clone(),
        sender_wallet.to_string(),
        sender_name.clone(),
        resolved.github_owner.clone(),
        resolved.package_name.clone(),
        amount_base,
        reason.clone(),
        evidence,
        status.to_string(),
        Some(tx_hash.to_string()),
        now.to_rfc3339(),
        expires.to_rfc3339(),
    ).await;

    if let Err(e) = insert_result {
        error!(tip_id = %tip_id, error = %e, "failed to insert tip");
        return Err((StatusCode::INTERNAL_SERVER_ERROR, "Failed to record tip".to_string()));
    }

    let receipt_url = format!("{receipt_base_url}/tx/{tip_id}");

    // Metrics
    crate::metrics::record_tip_created(&resolved.github_owner);

    info!(
        tip_id = %tip_id,
        recipient = %resolved.github_owner,
        package = ?resolved.package_name,
        amount_usd = req.amount_usd,
        tx_hash = %tx_hash,
        "tip created (verified payment)"
    );

    // Fire-and-forget: GitHub issue notification
    let gh_owner = resolved.github_owner.clone();
    let pkg = resolved.package_name.clone();
    let receipt = receipt_url.clone();
    let amount = req.amount_usd;
    let reason_md = sanitize_for_markdown(&reason);
    let client = state.http_client.clone();
    let repo = state.db_reader.conn_raw().ok().and_then(|conn| {
        conn.query_row(
            "SELECT github_repo FROM npm_cache WHERE github_owner = ? LIMIT 1",
            params![&gh_owner],
            |row| row.get::<_, String>(0),
        ).ok()
    });
    tokio::spawn(async move {
        notify_github_issue(&client, &gh_owner, repo.as_deref().or(pkg.as_deref()), amount, &reason_md, &receipt).await;
    });

    Ok(TipResponse {
        receipt_url,
        recipient: resolved.github_owner.clone(),
        resolved_github: resolved.github_owner,
        status: status.to_string(),
        tx_hash: Some(tx_hash.to_string()),
        tip_id,
    })
}

// ─── Route handlers ─────────────────────────────────────────────────────────

/// POST /paygate/tip — create a tip with MPP payment verification.
///
/// First call (no payment headers): returns 402 with tip amount as price.
/// Second call (with X-Payment-Tx): verifies payment, creates tip.
pub async fn handle_tip(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // Parse body manually so we keep the raw bytes for request hash
    let req: TipRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("Invalid request: {e}") }))).into_response(),
    };

    if req.amount_usd < MIN_TIP_AMOUNT_USD {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("Minimum tip is ${MIN_TIP_AMOUNT_USD}") }))).into_response();
    }
    if req.amount_usd > MAX_TIP_AMOUNT_USD {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("Maximum tip is ${MAX_TIP_AMOUNT_USD}") }))).into_response();
    }

    let config = state.current_config();
    let receipt_base_url = config.tips.as_ref()
        .map(|t| t.receipt_base_url.clone())
        .unwrap_or_else(|| "https://tips.paygate.fm".to_string());

    let payment = mpp::extract_payment_headers(&headers);

    match payment {
        None => {
            let endpoint = format!("POST /paygate/tip:{}", req.target);
            mpp::payment_required_response_with_price(
                &state,
                &endpoint,
                usdc_base_units(req.amount_usd) as u64,
            ).await
        }
        Some(payment_headers) => {
            let endpoint = format!("POST /paygate/tip:{}", req.target);
            // Use raw body bytes for request hash (must match what client sent)
            let request_hash = paygate_common::hash::request_hash("POST", "/paygate/tip", &body);

            let result = verifier::verify_payment(
                &state,
                &payment_headers.tx_hash,
                &payment_headers.payer_address,
                payment_headers.quote_id.as_deref(),
                &endpoint,
                &request_hash,
            ).await;

            match result {
                VerificationResult::Valid(_proof) => {
                    match create_tip_record(
                        &state,
                        &req,
                        &payment_headers.payer_address,
                        &payment_headers.tx_hash,
                        &receipt_base_url,
                        None,
                    ).await {
                        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
                        Err((status, msg)) => (status, Json(json!({ "error": msg }))).into_response(),
                    }
                }
                VerificationResult::TxNotFound => {
                    (StatusCode::BAD_REQUEST, Json(json!({ "error": "Transaction not found on chain. It may still be pending." }))).into_response()
                }
                VerificationResult::InsufficientAmount { expected, actual } => {
                    (StatusCode::PAYMENT_REQUIRED, Json(json!({
                        "error": "Insufficient payment",
                        "expected": expected,
                        "actual": actual,
                    }))).into_response()
                }
                VerificationResult::ReplayDetected => {
                    (StatusCode::CONFLICT, Json(json!({ "error": "This transaction has already been used" }))).into_response()
                }
                other => {
                    let msg = format!("{other:?}");
                    warn!(error = %msg, "tip payment verification failed");
                    (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("Payment verification failed: {msg}") }))).into_response()
                }
            }
        }
    }
}

/// POST /paygate/tip/batch — batch tip with payment verification.
///
/// Batch tips require payment for the total amount. The 402 response includes
/// the sum of all tip amounts. Dedup happens BEFORE payment.
pub async fn handle_tip_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let req: TipBatchRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("Invalid request: {e}") }))).into_response(),
    };

    if req.tips.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Empty batch" }))).into_response();
    }
    if req.tips.len() > MAX_BATCH_SIZE {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("Batch too large (max {MAX_BATCH_SIZE})") }))).into_response();
    }

    for tip in &req.tips {
        if tip.amount_usd < MIN_TIP_AMOUNT_USD || tip.amount_usd > MAX_TIP_AMOUNT_USD {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error": format!("Tip amount must be between ${MIN_TIP_AMOUNT_USD} and ${MAX_TIP_AMOUNT_USD}")
            }))).into_response();
        }
    }

    // Phase 1: Resolve all targets and dedup BEFORE payment
    let mut resolved_tips: Vec<(TipRequest, ResolvedTarget)> = Vec::new();
    let mut seen_owners: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut skipped: Vec<TipBatchResult> = Vec::new();

    for tip in &req.tips {
        match resolve_target(&state.http_client, &state.db_reader, &tip.target).await {
            Ok(resolved) => {
                if seen_owners.contains(&resolved.github_owner) {
                    skipped.push(TipBatchResult {
                        target: tip.target.clone(),
                        status: "skipped_duplicate".to_string(),
                        receipt_url: None,
                        tip_id: None,
                        error: Some("Already tipping this owner in this batch".to_string()),
                    });
                } else {
                    seen_owners.insert(resolved.github_owner.clone());
                    resolved_tips.push((tip.clone(), resolved));
                }
            }
            Err((_status, msg)) => {
                skipped.push(TipBatchResult {
                    target: tip.target.clone(),
                    status: "error".to_string(),
                    receipt_url: None,
                    tip_id: None,
                    error: Some(msg),
                });
            }
        }
    }

    // Calculate total amount for payment
    let total_amount: f64 = resolved_tips.iter().map(|(t, _)| t.amount_usd).sum();
    let total_base_units = usdc_base_units(total_amount) as u64;

    // Phase 2: Check payment
    let payment = mpp::extract_payment_headers(&headers);

    match payment {
        None => {
            // Return 402 with total batch amount as price
            let endpoint = "POST /paygate/tip/batch";
            mpp::payment_required_response_with_price(&state, endpoint, total_base_units).await
        }
        Some(payment_headers) => {
            // Use raw body bytes for request hash
            let request_hash = paygate_common::hash::request_hash("POST", "/paygate/tip/batch", &body);

            let result = verifier::verify_payment(
                &state,
                &payment_headers.tx_hash,
                &payment_headers.payer_address,
                payment_headers.quote_id.as_deref(),
                "POST /paygate/tip/batch",
                &request_hash,
            ).await;

            match result {
                VerificationResult::Valid(_proof) => {
                    let config = state.current_config();
                    let receipt_base_url = config.tips.as_ref()
                        .map(|t| t.receipt_base_url.clone())
                        .unwrap_or_else(|| "https://tips.paygate.fm".to_string());

                    // Phase 3: Create all tips (payment already verified)
                    let mut results: Vec<TipBatchResult> = skipped;
                    let mut succeeded = 0;
                    let mut succeeded_amount = 0.0;

                    for (tip_req, resolved) in &resolved_tips {
                        let tip_id = generate_tip_id();
                        let amount_base = usdc_base_units(tip_req.amount_usd);
                        let now = chrono::Utc::now();
                        let expires = now + chrono::Duration::days(90);
                        let reason = sanitize_text(&tip_req.reason, MAX_REASON_LEN);
                        let evidence = tip_req.evidence.as_deref().map(|e| sanitize_text(e, MAX_EVIDENCE_LEN));
                        let sender_name = req.sender_name.as_deref()
                            .or(tip_req.sender_name.as_deref())
                            .map(|s| sanitize_text(s, 100));

                        let insert_result = state.db_writer.insert_tip(
                            tip_id.clone(),
                            payment_headers.payer_address.clone(),
                            sender_name,
                            resolved.github_owner.clone(),
                            resolved.package_name.clone(),
                            amount_base,
                            reason,
                            evidence,
                            "escrowed".to_string(),
                            Some(payment_headers.tx_hash.clone()),
                            now.to_rfc3339(),
                            expires.to_rfc3339(),
                        ).await;

                        match insert_result {
                            Ok(()) => {
                                let receipt_url = format!("{receipt_base_url}/tx/{tip_id}");
                                crate::metrics::record_tip_created(&resolved.github_owner);
                                succeeded += 1;
                                succeeded_amount += tip_req.amount_usd;
                                results.push(TipBatchResult {
                                    target: tip_req.target.clone(),
                                    status: "escrowed".to_string(),
                                    receipt_url: Some(receipt_url),
                                    tip_id: Some(tip_id),
                                    error: None,
                                });
                            }
                            Err(e) => {
                                error!(error = %e, "batch tip insert failed");
                                results.push(TipBatchResult {
                                    target: tip_req.target.clone(),
                                    status: "error".to_string(),
                                    receipt_url: None,
                                    tip_id: None,
                                    error: Some("Failed to record tip".to_string()),
                                });
                            }
                        }
                    }

                    let resp = TipBatchResponse {
                        summary: TipBatchSummary {
                            total: req.tips.len(),
                            succeeded,
                            failed: req.tips.len() - succeeded,
                            total_amount_usd: succeeded_amount,
                        },
                        results,
                    };

                    (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response()
                }
                other => {
                    let msg = format!("{other:?}");
                    (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("Payment verification failed: {msg}") }))).into_response()
                }
            }
        }
    }
}

// ─── Internal API handlers (for Vercel web app) ─────────────────────────────

pub async fn handle_get_tip(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(tip_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err((status, msg)) = verify_internal_auth(&headers) {
        return (status, Json(json!({ "error": msg }))).into_response();
    }
    match get_tip_record(&state.db_reader, &tip_id) {
        Some(record) => (StatusCode::OK, Json(serde_json::to_value(record).unwrap())).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({ "error": "Tip not found" }))).into_response(),
    }
}

pub async fn handle_get_tips_by_recipient(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(github_username): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err((status, msg)) = verify_internal_auth(&headers) {
        return (status, Json(json!({ "error": msg }))).into_response();
    }
    let records = get_tips_for_recipient(&state.db_reader, &github_username.to_lowercase());
    (StatusCode::OK, Json(serde_json::to_value(records).unwrap())).into_response()
}

pub async fn handle_get_leaderboard(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err((status, msg)) = verify_internal_auth(&headers) {
        return (status, Json(json!({ "error": msg }))).into_response();
    }
    let entries = get_leaderboard(&state.db_reader);
    (StatusCode::OK, Json(serde_json::to_value(entries).unwrap())).into_response()
}

/// POST /paygate/internal/claim — claim escrowed tips (FIX: atomic + wallet validation)
pub async fn handle_claim(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ClaimRequest>,
) -> impl IntoResponse {
    if let Err((status, msg)) = verify_internal_auth(&headers) {
        return (status, Json(json!({ "error": msg }))).into_response();
    }

    // Validate wallet address format
    if !is_valid_wallet_address(&req.wallet_address) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Invalid wallet address. Expected format: 0x followed by 40 hex characters." }))).into_response();
    }

    let gh_lower = req.github_username.to_lowercase();

    // Phase 1: gather claim targets (sync DB reads, then async org checks)
    let candidate_orgs: Vec<String> = {
        let conn = match state.db_reader.conn_raw() {
            Ok(c) => c,
            Err(_) => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "Database unavailable" }))).into_response(),
        };

        let direct_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tips WHERE recipient_gh = ? AND status = 'escrowed'",
            params![gh_lower],
            |row| row.get(0),
        ).unwrap_or(0);

        if direct_count > 0 {
            vec![]
        } else {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT recipient_gh FROM tips WHERE status = 'escrowed'"
            ).unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        }
    };

    // Phase 2: async org membership checks
    let mut claim_targets: Vec<String> = vec![gh_lower.clone()];
    for org in &candidate_orgs {
        if is_org_member(&state.http_client, org, &gh_lower).await {
            claim_targets.push(org.clone());
            info!(user = %gh_lower, org = %org, "org member can claim org tips");
        }
    }

    // Phase 3: atomic claim (FIX: wrap in SAVEPOINT)
    let conn = match state.db_reader.conn_raw() {
        Ok(c) => c,
        Err(_) => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "Database unavailable" }))).into_response(),
    };

    // Count total claimable tips
    let mut count: i64 = 0;
    for target in &claim_targets {
        count += conn.query_row(
            "SELECT COUNT(*) FROM tips WHERE recipient_gh = ? AND status = 'escrowed'",
            params![target],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0);
    }

    if count == 0 {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "No unclaimed tips" }))).into_response();
    }

    // Atomic claim via SAVEPOINT
    let claim_result = (|| -> Result<usize, String> {
        conn.execute_batch("SAVEPOINT claim_tips").map_err(|e| e.to_string())?;

        // Register wallet for all claim targets
        for target in &claim_targets {
            conn.execute(
                "INSERT OR REPLACE INTO tip_registry (github_username, wallet_address, registered_at)
                 VALUES (?, ?, datetime('now'))",
                params![target, req.wallet_address],
            ).map_err(|e| {
                let _ = conn.execute_batch("ROLLBACK TO claim_tips");
                e.to_string()
            })?;
        }

        // Mark all escrowed tips as claimed
        let now = chrono::Utc::now().to_rfc3339();
        let mut updated: usize = 0;
        for target in &claim_targets {
            updated += conn.execute(
                "UPDATE tips SET status = 'claimed', claim_wallet = ?, claimed_at = ?
                 WHERE recipient_gh = ? AND status = 'escrowed'",
                params![req.wallet_address, now, target],
            ).map_err(|e| {
                let _ = conn.execute_batch("ROLLBACK TO claim_tips");
                e.to_string()
            })?;
        }

        conn.execute_batch("RELEASE claim_tips").map_err(|e| e.to_string())?;
        Ok(updated)
    })();

    match claim_result {
        Ok(updated) if updated > 0 => {
            crate::metrics::record_tips_claimed(updated as u64);

            // Calculate total amount to transfer
            let total_amount: i64 = {
                let mut total = 0i64;
                for target in &claim_targets {
                    total += conn.query_row(
                        "SELECT COALESCE(SUM(amount_usdc), 0) FROM tips WHERE recipient_gh = ? AND status = 'claimed' AND claim_wallet = ?",
                        params![target, req.wallet_address],
                        |row| row.get::<_, i64>(0),
                    ).unwrap_or(0);
                }
                total
            };

            // Send USDC from gateway wallet to claim wallet (on-chain payout)
            let payout_tx = if total_amount > 0 {
                match crate::payout::transfer_usdc(&state, &req.wallet_address, total_amount as u64).await {
                    Ok(tx_hash) => {
                        info!(
                            github = %gh_lower,
                            wallet = %req.wallet_address,
                            amount = total_amount,
                            tx_hash = %tx_hash,
                            "payout sent"
                        );
                        Some(tx_hash)
                    }
                    Err(e) => {
                        // Payout failed but tips are still marked claimed.
                        // A background retry can handle this later.
                        warn!(
                            github = %gh_lower,
                            wallet = %req.wallet_address,
                            amount = total_amount,
                            error = %e,
                            "payout failed — tips claimed but transfer pending"
                        );
                        None
                    }
                }
            } else {
                None
            };

            info!(github = %gh_lower, wallet = %req.wallet_address, tips_claimed = updated, "tips claimed");
            (StatusCode::OK, Json(json!({
                "claimed": updated,
                "wallet": req.wallet_address,
                "payout_tx": payout_tx,
                "amount_usdc": total_amount,
            }))).into_response()
        }
        Ok(_) => {
            (StatusCode::NOT_FOUND, Json(json!({ "error": "No unclaimed tips" }))).into_response()
        }
        Err(e) => {
            error!(error = %e, "claim transaction failed");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "Claim failed" }))).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct ClaimRequest {
    pub github_username: String,
    pub wallet_address: String,
}

// ─── DB query helpers ───────────────────────────────────────────────────────

fn get_tip_record(db_reader: &DbReader, tip_id: &str) -> Option<TipRecord> {
    let conn = db_reader.conn_raw().ok()?;
    conn.query_row(
        "SELECT id, sender_wallet, sender_name, recipient_gh, package_name,
                amount_usdc, reason, evidence, status, tx_hash, created_at
         FROM tips WHERE id = ?",
        params![tip_id],
        |row| {
            Ok(TipRecord {
                id: row.get(0)?,
                sender_wallet: row.get(1)?,
                sender_name: row.get(2)?,
                recipient_gh: row.get(3)?,
                package_name: row.get(4)?,
                amount_usdc: row.get(5)?,
                reason: row.get(6)?,
                evidence: row.get(7)?,
                status: row.get(8)?,
                tx_hash: row.get(9)?,
                created_at: row.get(10)?,
            })
        },
    )
    .ok()
}

fn get_tips_for_recipient(db_reader: &DbReader, github_username: &str) -> Vec<TipRecord> {
    let conn = match db_reader.conn_raw() {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let mut stmt = conn.prepare(
        "SELECT id, sender_wallet, sender_name, recipient_gh, package_name,
                amount_usdc, reason, evidence, status, tx_hash, created_at
         FROM tips WHERE recipient_gh = ? ORDER BY created_at DESC LIMIT 100",
    ).unwrap();
    let rows = stmt.query_map(params![github_username], |row| {
        Ok(TipRecord {
            id: row.get(0)?,
            sender_wallet: row.get(1)?,
            sender_name: row.get(2)?,
            recipient_gh: row.get(3)?,
            package_name: row.get(4)?,
            amount_usdc: row.get(5)?,
            reason: row.get(6)?,
            evidence: row.get(7)?,
            status: row.get(8)?,
            tx_hash: row.get(9)?,
            created_at: row.get(10)?,
        })
    }).unwrap();
    rows.filter_map(|r| r.ok()).collect()
}

fn get_leaderboard(db_reader: &DbReader) -> Vec<LeaderboardEntry> {
    let conn = match db_reader.conn_raw() {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let mut stmt = conn.prepare(
        "SELECT recipient_gh, SUM(amount_usdc), COUNT(*), COUNT(DISTINCT sender_wallet)
         FROM tips WHERE status IN ('escrowed', 'claimed')
         GROUP BY recipient_gh ORDER BY SUM(amount_usdc) DESC LIMIT 50",
    ).unwrap();
    let rows = stmt.query_map([], |row| {
        Ok(LeaderboardEntry {
            github_username: row.get(0)?,
            total_amount: row.get(1)?,
            tip_count: row.get(2)?,
            agent_count: row.get(3)?,
        })
    }).unwrap();
    rows.filter_map(|r| r.ok()).collect()
}

// ─── Escrow expiry background task (FIX: run on startup + daily) ────────────

pub async fn escrow_expiry_task(db_reader: DbReader) {
    // Run immediately on startup
    run_escrow_expiry(&db_reader);

    // Then run daily
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(86400));
    loop {
        interval.tick().await;
        run_escrow_expiry(&db_reader);
    }
}

fn run_escrow_expiry(db_reader: &DbReader) {
    if let Ok(conn) = db_reader.conn_raw() {
        let now = chrono::Utc::now().to_rfc3339();
        match conn.execute(
            "UPDATE tips SET status = 'reclaimed' WHERE status = 'escrowed' AND expires_at < ?",
            params![now],
        ) {
            Ok(n) if n > 0 => {
                info!(count = n, "reclaimed expired escrowed tips");
                crate::metrics::record_tips_expired(n as u64);
            }
            Err(e) => warn!(error = %e, "escrow expiry check failed"),
            _ => {}
        }

        // Also clean up old npm_cache entries (> 7 days old)
        let _ = conn.execute(
            "DELETE FROM npm_cache WHERE resolved_at < datetime('now', '-7 days')",
            [],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_text_strips_html() {
        let input = "<script>alert('xss')</script>";
        let result = sanitize_text(input, 500);
        assert!(!result.contains("<script>"));
        assert!(result.contains("&lt;script&gt;"));
    }

    #[test]
    fn test_sanitize_text_truncates_safely() {
        let input = "a".repeat(600);
        let result = sanitize_text(&input, 500);
        // After truncation (500 chars) and escaping, length should be reasonable
        assert!(result.len() <= 500);
    }

    #[test]
    fn test_sanitize_text_utf8_safe() {
        // Multi-byte characters: each emoji is 4 bytes
        let input = "🎉🎊🎈🎁🎀🎂🎄🎃🎆🎇";
        let result = sanitize_text(input, 5);
        // Should truncate to 5 characters (emojis), not panic on byte boundary
        assert_eq!(result.chars().count(), 5);
        assert!(result.starts_with("🎉🎊🎈🎁🎀"));
    }

    #[test]
    fn test_usdc_base_units() {
        assert_eq!(usdc_base_units(0.50), 500_000);
        assert_eq!(usdc_base_units(1.0), 1_000_000);
        assert_eq!(usdc_base_units(0.01), 10_000);
    }

    #[test]
    fn test_generate_tip_id_format() {
        let id = generate_tip_id();
        assert!(id.starts_with("tip_"));
        assert!(id.len() > 20);
    }

    #[test]
    fn test_sanitize_for_markdown() {
        let input = "Used `chalk.green()` for *bold* output";
        let result = sanitize_for_markdown(input);
        assert!(result.contains("\\`chalk.green()\\`"));
        assert!(result.contains("\\*bold\\*"));
    }

    #[test]
    fn test_is_valid_wallet_address() {
        assert!(is_valid_wallet_address("0x742d35Cc6634C0532925a3b844Bc9e7595f3fAE6"));
        assert!(!is_valid_wallet_address("0x742d")); // too short
        assert!(!is_valid_wallet_address("742d35Cc6634C0532925a3b844Bc9e7595f3fAE6")); // no 0x
        assert!(!is_valid_wallet_address("0xZZZd35Cc6634C0532925a3b844Bc9e7595f3fAE6")); // invalid hex
        assert!(!is_valid_wallet_address("")); // empty
    }
}
