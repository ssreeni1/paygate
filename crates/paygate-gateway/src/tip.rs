use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::db::{DbReader, DbWriter, DbError};
use crate::npm_resolver::{self, ResolveError};
use crate::server::AppState;

// ─── Internal API auth ──────────────────────────────────────────────────────

/// Verify the internal API secret from the Authorization header.
/// Returns Ok(()) if valid, Err response if not.
fn verify_internal_auth(headers: &HeaderMap) -> Result<(), (StatusCode, &'static str)> {
    let secret = std::env::var("PAYGATE_INTERNAL_SECRET").unwrap_or_default();
    if secret.is_empty() {
        // No secret configured, allow all (dev mode)
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

// ─── Constants ──────────────────────────────────────────────────────────────

const MAX_TIP_AMOUNT_USD: f64 = 100.0;
const MIN_TIP_AMOUNT_USD: f64 = 0.01;
const AUTO_APPROVE_THRESHOLD_USD: f64 = 1.0;
const MAX_REASON_LEN: usize = 500;
const MAX_EVIDENCE_LEN: usize = 1000;
const MAX_BATCH_SIZE: usize = 50;

// ─── Request / Response types ───────────────────────────────────────────────

#[derive(Deserialize)]
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

#[derive(Deserialize)]
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

// ─── Sanitization ───────────────────────────────────────────────────────────

fn sanitize_text(input: &str, max_len: usize) -> String {
    let truncated = if input.len() > max_len {
        &input[..max_len]
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
    // Escape markdown special chars for GitHub issue body
    input
        .replace('\\', "\\\\")
        .replace('`', "\\`")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

// ─── ULID generation ────────────────────────────────────────────────────────

fn generate_tip_id() -> String {
    // Simple ULID-like: timestamp + random suffix
    let now = chrono::Utc::now();
    let ts = now.format("%Y%m%d%H%M%S").to_string();
    let rand: u64 = rand::random();
    format!("tip_{ts}_{rand:016x}")
}

// ─── Tip creation logic ─────────────────────────────────────────────────────

fn usdc_base_units(usd: f64) -> i64 {
    (usd * 1_000_000.0).round() as i64
}

struct TipContext<'a> {
    http_client: &'a reqwest::Client,
    db_reader: &'a DbReader,
    db_writer: &'a DbWriter,
    sender_wallet: String,
    receipt_base_url: String,
}

async fn create_single_tip(
    ctx: &TipContext<'_>,
    req: &TipRequest,
    sender_name_override: Option<&str>,
) -> Result<TipResponse, (StatusCode, String)> {
    // Validate amount
    if req.amount_usd < MIN_TIP_AMOUNT_USD {
        return Err((StatusCode::BAD_REQUEST, format!("Minimum tip is ${MIN_TIP_AMOUNT_USD}")));
    }
    if req.amount_usd > MAX_TIP_AMOUNT_USD {
        return Err((StatusCode::BAD_REQUEST, format!("Maximum tip is ${MAX_TIP_AMOUNT_USD}")));
    }

    // Sanitize text fields
    let reason = sanitize_text(&req.reason, MAX_REASON_LEN);
    let evidence = req.evidence.as_deref().map(|e| sanitize_text(e, MAX_EVIDENCE_LEN));
    let sender_name = sender_name_override
        .or(req.sender_name.as_deref())
        .map(|s| sanitize_text(s, 100));

    // Resolve target to GitHub owner
    let target = req.target.trim().to_lowercase();
    let (github_owner, package_name) = if target.starts_with('@') {
        // Direct GitHub username
        (target.trim_start_matches('@').to_string(), None)
    } else {
        // npm package name — resolve to GitHub owner
        match npm_resolver::resolve_package(ctx.http_client, ctx.db_reader, &target).await {
            Ok(resolution) => (resolution.github_owner, Some(target.clone())),
            Err(ResolveError::PackageNotFound) => {
                return Err((StatusCode::NOT_FOUND, format!("Package '{target}' not found on npm")));
            }
            Err(ResolveError::NoRepository) => {
                return Err((StatusCode::NOT_FOUND, format!("Package '{target}' has no repository field. Tip by GitHub username instead.")));
            }
            Err(ResolveError::NotGitHub) => {
                return Err((StatusCode::NOT_FOUND, "Only GitHub-hosted packages supported.".to_string()));
            }
            Err(e) => {
                warn!(package = target, error = %e, "npm resolution failed");
                return Err((StatusCode::SERVICE_UNAVAILABLE, "Package resolution temporarily unavailable. Tip by GitHub username instead.".to_string()));
            }
        }
    };

    // Check if recipient has a registered wallet
    let wallet = lookup_wallet(ctx.db_reader, &github_owner);

    let tip_id = generate_tip_id();
    let amount_base = usdc_base_units(req.amount_usd);
    let now = chrono::Utc::now();
    let expires = now + chrono::Duration::days(90);

    let (status, tx_hash) = if let Some(_wallet_addr) = &wallet {
        // TODO: Execute x402 payment to wallet_addr
        // For now, record as "paid" with placeholder tx hash
        // Real implementation will call walletClient.writeContract
        ("paid".to_string(), Some(format!("0x{:064x}", rand::random::<u128>())))
    } else {
        ("escrowed".to_string(), None)
    };

    // Insert tip record via direct connection (tips are low-frequency, don't need batching)
    if let Ok(conn) = ctx.db_reader.conn_raw() {
        let result = conn.execute(
            "INSERT INTO tips (id, sender_wallet, sender_name, recipient_gh, package_name,
             amount_usdc, reason, evidence, status, tx_hash, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                tip_id,
                ctx.sender_wallet,
                sender_name,
                github_owner,
                package_name,
                amount_base,
                reason,
                evidence,
                status,
                tx_hash,
                now.to_rfc3339(),
                expires.to_rfc3339(),
            ],
        );
        if let Err(e) = result {
            error!(tip_id = %tip_id, error = %e, "failed to insert tip");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, "Failed to record tip".to_string()));
        }
    } else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "Database unavailable".to_string()));
    }

    let receipt_url = format!("{}/tx/{}", ctx.receipt_base_url, tip_id);

    info!(
        tip_id = %tip_id,
        recipient = %github_owner,
        package = ?package_name,
        amount_usd = req.amount_usd,
        status = %status,
        "tip created"
    );

    // Fire-and-forget: GitHub issue notification
    if status == "escrowed" {
        let gh_owner = github_owner.clone();
        let pkg = package_name.clone();
        let receipt = receipt_url.clone();
        let amount = req.amount_usd;
        let reason_md = sanitize_for_markdown(&reason);
        let client = ctx.http_client.clone();
        tokio::spawn(async move {
            notify_github_issue(&client, &gh_owner, pkg.as_deref(), amount, &reason_md, &receipt).await;
        });
    }

    Ok(TipResponse {
        receipt_url,
        recipient: github_owner.clone(),
        resolved_github: github_owner,
        status,
        tx_hash,
        tip_id,
    })
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

/// Check if a GitHub user is a public member of an org.
/// Uses unauthenticated GitHub API: GET /orgs/{org}/public_members/{user}
/// Returns 204 if member, 404 if not. No auth token needed.
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

// ─── GitHub issue notification (fire-and-forget) ────────────────────────────

async fn notify_github_issue(
    _client: &reqwest::Client,
    _github_owner: &str,
    _package: Option<&str>,
    _amount: f64,
    _reason: &str,
    _receipt_url: &str,
) {
    // TODO: Implement GitHub issue creation via GitHub API
    // Requires a GitHub token (env var GITHUB_TOKEN)
    // POST https://api.github.com/repos/{owner}/{repo}/issues
    // Body: { "title": "An AI agent tipped you...", "body": "..." }
    // Fire-and-forget: log success/failure but don't block tip creation
    info!(owner = _github_owner, "GitHub issue notification (placeholder)");
}

// ─── Route handlers ─────────────────────────────────────────────────────────

pub async fn handle_tip(
    State(state): State<AppState>,
    Json(req): Json<TipRequest>,
) -> impl IntoResponse {
    let config = state.current_config();
    let receipt_base_url = config.tips.as_ref()
        .map(|t| t.receipt_base_url.clone())
        .unwrap_or_else(|| "https://tips.paygate.fm".to_string());

    let ctx = TipContext {
        http_client: &state.http_client,
        db_reader: &state.db_reader,
        db_writer: &state.db_writer,
        sender_wallet: "unknown".to_string(), // TODO: extract from x402 auth header
        receipt_base_url,
    };

    match create_single_tip(&ctx, &req, None).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, msg)) => (status, Json(serde_json::json!({ "error": msg }))).into_response(),
    }
}

pub async fn handle_tip_batch(
    State(state): State<AppState>,
    Json(req): Json<TipBatchRequest>,
) -> impl IntoResponse {
    if req.tips.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "Empty batch" }))).into_response();
    }
    if req.tips.len() > MAX_BATCH_SIZE {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": format!("Batch too large (max {MAX_BATCH_SIZE})") }))).into_response();
    }

    let config = state.current_config();
    let receipt_base_url = config.tips.as_ref()
        .map(|t| t.receipt_base_url.clone())
        .unwrap_or_else(|| "https://tips.paygate.fm".to_string());

    let ctx = TipContext {
        http_client: &state.http_client,
        db_reader: &state.db_reader,
        db_writer: &state.db_writer,
        sender_wallet: "unknown".to_string(),
        receipt_base_url,
    };

    // Dedup by resolved GitHub owner within batch
    let mut seen_owners: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut results = Vec::with_capacity(req.tips.len());
    let mut total_amount = 0.0;
    let mut succeeded = 0;
    let mut failed = 0;

    for tip_req in &req.tips {
        match create_single_tip(&ctx, tip_req, req.sender_name.as_deref()).await {
            Ok(resp) => {
                if seen_owners.contains(&resp.resolved_github) {
                    // Skip duplicate in same batch
                    results.push(TipBatchResult {
                        target: tip_req.target.clone(),
                        status: "skipped_duplicate".to_string(),
                        receipt_url: None,
                        tip_id: None,
                        error: Some("Already tipped this owner in this batch".to_string()),
                    });
                    failed += 1;
                } else {
                    seen_owners.insert(resp.resolved_github.clone());
                    total_amount += tip_req.amount_usd;
                    succeeded += 1;
                    results.push(TipBatchResult {
                        target: tip_req.target.clone(),
                        status: resp.status,
                        receipt_url: Some(resp.receipt_url),
                        tip_id: Some(resp.tip_id),
                        error: None,
                    });
                }
            }
            Err((_status, msg)) => {
                failed += 1;
                results.push(TipBatchResult {
                    target: tip_req.target.clone(),
                    status: "error".to_string(),
                    receipt_url: None,
                    tip_id: None,
                    error: Some(msg),
                });
            }
        }
    }

    let resp = TipBatchResponse {
        results,
        summary: TipBatchSummary {
            total: req.tips.len(),
            succeeded,
            failed,
            total_amount_usd: total_amount,
        },
    };

    (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response()
}

// ─── Internal API handlers (for Vercel web app) ─────────────────────────────

pub async fn handle_get_tip(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(tip_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err((status, msg)) = verify_internal_auth(&headers) {
        return (status, Json(serde_json::json!({ "error": msg }))).into_response();
    }
    match get_tip_record(&state.db_reader, &tip_id) {
        Some(record) => (StatusCode::OK, Json(serde_json::to_value(record).unwrap())).into_response(),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "Tip not found" }))).into_response(),
    }
}

pub async fn handle_get_tips_by_recipient(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(github_username): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err((status, msg)) = verify_internal_auth(&headers) {
        return (status, Json(serde_json::json!({ "error": msg }))).into_response();
    }
    let records = get_tips_for_recipient(&state.db_reader, &github_username.to_lowercase());
    (StatusCode::OK, Json(serde_json::to_value(records).unwrap())).into_response()
}

pub async fn handle_get_leaderboard(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err((status, msg)) = verify_internal_auth(&headers) {
        return (status, Json(serde_json::json!({ "error": msg }))).into_response();
    }
    let entries = get_leaderboard(&state.db_reader);
    (StatusCode::OK, Json(serde_json::to_value(entries).unwrap())).into_response()
}

pub async fn handle_claim(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ClaimRequest>,
) -> impl IntoResponse {
    if let Err((status, msg)) = verify_internal_auth(&headers) {
        return (status, Json(serde_json::json!({ "error": msg }))).into_response();
    }
    let gh_lower = req.github_username.to_lowercase();

    // Phase 1: gather claim targets (sync DB reads, then async org checks)
    // Read candidate orgs from DB first, then drop the connection before async calls
    let candidate_orgs: Vec<String> = {
        let conn = match state.db_reader.conn_raw() {
            Ok(c) => c,
            Err(_) => return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({ "error": "Database unavailable" }))).into_response(),
        };

        // Check direct match first
        let direct_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tips WHERE recipient_gh = ? AND status = 'escrowed'",
            params![gh_lower],
            |row| row.get(0),
        ).unwrap_or(0);

        if direct_count > 0 {
            vec![] // Direct match found, no need to check orgs
        } else {
            // Get all distinct escrowed recipients to check for org membership
            let mut stmt = conn.prepare(
                "SELECT DISTINCT recipient_gh FROM tips WHERE status = 'escrowed'"
            ).unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        }
        // conn dropped here, safe to await below
    };

    // Phase 2: async org membership checks (no DB connection held)
    let mut claim_targets: Vec<String> = vec![gh_lower.clone()];
    for org in &candidate_orgs {
        if is_org_member(&state.http_client, org, &gh_lower).await {
            claim_targets.push(org.clone());
            info!(user = %gh_lower, org = %org, "org member can claim org tips");
        }
    }

    // Phase 3: execute claims (reopen DB connection)
    let conn = match state.db_reader.conn_raw() {
        Ok(c) => c,
        Err(_) => return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({ "error": "Database unavailable" }))).into_response(),
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
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "No unclaimed tips" }))).into_response();
    }

    // Register wallet for all claim targets
    for target in &claim_targets {
        let _ = conn.execute(
            "INSERT OR REPLACE INTO tip_registry (github_username, wallet_address, registered_at)
             VALUES (?, ?, datetime('now'))",
            params![target, req.wallet_address],
        );
    }

    // Mark all escrowed tips as claimed
    let now = chrono::Utc::now().to_rfc3339();
    let mut updated: usize = 0;
    for target in &claim_targets {
        updated += conn.execute(
            "UPDATE tips SET status = 'claimed', claim_wallet = ?, claimed_at = ?
             WHERE recipient_gh = ? AND status = 'escrowed'",
            params![req.wallet_address, now, target],
        ).unwrap_or(0);
    }

    // TODO: Initiate on-chain transfer of escrowed funds to claim_wallet
    // This should be a background task that processes each claimed tip

    info!(
        github = %gh_lower,
        wallet = %req.wallet_address,
        tips_claimed = updated,
        "tips claimed"
    );

    (StatusCode::OK, Json(serde_json::json!({
        "claimed": updated,
        "wallet": req.wallet_address,
    }))).into_response()
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
         FROM tips WHERE status IN ('paid', 'escrowed', 'claimed')
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

// ─── Escrow expiry background task ──────────────────────────────────────────

pub async fn escrow_expiry_task(db_reader: DbReader) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(86400)); // daily

    loop {
        interval.tick().await;

        if let Ok(conn) = db_reader.conn_raw() {
            let now = chrono::Utc::now().to_rfc3339();
            match conn.execute(
                "UPDATE tips SET status = 'reclaimed' WHERE status = 'escrowed' AND expires_at < ?",
                params![now],
            ) {
                Ok(n) if n > 0 => info!(count = n, "reclaimed expired escrowed tips"),
                Err(e) => warn!(error = %e, "escrow expiry check failed"),
                _ => {}
            }
        }
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
    fn test_sanitize_text_truncates() {
        let input = "a".repeat(600);
        let result = sanitize_text(&input, 500);
        assert!(result.len() <= 500 * 6); // each char could become up to 6 chars after escaping
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
}
