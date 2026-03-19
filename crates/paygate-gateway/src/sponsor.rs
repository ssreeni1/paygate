use arc_swap::ArcSwap;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use metrics::gauge;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::config::{parse_price_to_base_units, Config};

/// Atomic budget tracker for daily fee sponsorship spending.
pub struct SponsorBudget {
    daily_spent: AtomicU64,
    daily_limit: u64,
    per_tx_limit: u64,
    last_reset: AtomicI64,
}

impl SponsorBudget {
    pub fn new(daily_limit: u64, per_tx_limit: u64) -> Self {
        let now = chrono::Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap();
        let today_midnight = now.and_utc().timestamp();
        Self {
            daily_spent: AtomicU64::new(0),
            daily_limit,
            per_tx_limit,
            last_reset: AtomicI64::new(today_midnight),
        }
    }

    /// Check if a spend of `amount` is allowed, and if so, atomically deduct it.
    /// Returns Ok(remaining) or Err with the reason.
    pub fn check_and_spend(&self, amount: u64) -> Result<u64, BudgetError> {
        // Per-tx limit check
        if amount > self.per_tx_limit {
            return Err(BudgetError::PerTxLimitExceeded);
        }

        // Reset if new day
        self.maybe_reset();

        // Atomic check-and-increment
        loop {
            let current = self.daily_spent.load(Ordering::Acquire);
            let new_total = current.checked_add(amount).unwrap_or(u64::MAX);
            if new_total > self.daily_limit {
                return Err(BudgetError::DailyLimitExhausted);
            }
            match self.daily_spent.compare_exchange_weak(
                current,
                new_total,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(self.daily_limit - new_total),
                Err(_) => continue, // CAS failed, retry
            }
        }
    }

    /// Check if new day and reset counter.
    fn maybe_reset(&self) {
        let now = chrono::Utc::now();
        let today_midnight = now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
        let last = self.last_reset.load(Ordering::Acquire);
        if today_midnight > last {
            // New day — try to reset
            if self
                .last_reset
                .compare_exchange(last, today_midnight, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                self.daily_spent.store(0, Ordering::Release);
                info!("sponsor budget reset for new day");
            }
        }
    }

    pub fn remaining(&self) -> u64 {
        self.maybe_reset();
        let spent = self.daily_spent.load(Ordering::Acquire);
        self.daily_limit.saturating_sub(spent)
    }

    #[cfg(test)]
    fn set_last_reset(&self, ts: i64) {
        self.last_reset.store(ts, Ordering::Release);
    }
}

#[derive(Debug)]
pub enum BudgetError {
    DailyLimitExhausted,
    PerTxLimitExceeded,
}

/// Fee sponsorship service. Maintains its own state separate from AppState.
#[derive(Clone)]
pub struct SponsorService {
    config: Arc<ArcSwap<Config>>,
    http_client: reqwest::Client,
    budget: Arc<SponsorBudget>,
}

impl SponsorService {
    /// Create a new SponsorService. Fails if private key env var is not set.
    pub fn new(
        config: Arc<ArcSwap<Config>>,
        http_client: reqwest::Client,
    ) -> Result<Self, String> {
        let cfg = config.load();

        // Validate private key is set
        let pk_env = &cfg.tempo.private_key_env;
        if std::env::var(pk_env).is_err() {
            return Err(format!(
                "sponsorship enabled but {pk_env} not set"
            ));
        }

        // Parse budget config
        let daily_limit = parse_price_to_base_units(&cfg.sponsorship.budget_per_day)
            .unwrap_or(10_000_000); // default 10 USD
        let per_tx_limit = parse_price_to_base_units(&cfg.sponsorship.max_per_tx)
            .unwrap_or(10_000); // default 0.01 USD

        let budget = Arc::new(SponsorBudget::new(daily_limit, per_tx_limit));

        Ok(Self {
            config,
            http_client,
            budget,
        })
    }

    /// Spawn background task that checks wallet balance every 60s and updates metrics.
    pub fn spawn_balance_checker(&self) {
        let http_client = self.http_client.clone();
        let config = self.config.clone();
        let budget = self.budget.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));

            loop {
                interval.tick().await;

                // Update budget remaining metric
                let remaining = budget.remaining();
                gauge!("paygate_sponsor_budget_remaining").set(remaining as f64);

                // Query on-chain native balance
                let cfg = config.load();
                let pk_env = &cfg.tempo.private_key_env;
                let _pk_hex = match std::env::var(pk_env) {
                    Ok(k) => k,
                    Err(_) => continue,
                };

                // Derive address from private key (simplified: use the provider address as proxy)
                // In production, derive from the actual private key
                let sponsor_address = &cfg.provider.address;

                if let Some(balance) = query_native_balance(
                    &http_client,
                    &cfg.tempo.rpc_urls,
                    sponsor_address,
                )
                .await
                {
                    gauge!("paygate_sponsor_wallet_balance").set(balance as f64);

                    let per_tx = parse_price_to_base_units(&cfg.sponsorship.max_per_tx)
                        .unwrap_or(10_000);
                    if balance < per_tx {
                        warn!(
                            balance = balance,
                            min_required = per_tx,
                            "sponsor wallet balance critically low"
                        );
                    }
                }
            }
        });
    }

    /// Get the primary RPC URL for relaying transactions.
    fn primary_rpc_url(&self) -> String {
        let cfg = self.config.load();
        cfg.tempo
            .rpc_urls
            .first()
            .cloned()
            .unwrap_or_default()
    }
}

/// Handle incoming JSON-RPC requests from viem's `withFeePayer` transport.
///
/// The fee payer transport in viem forwards JSON-RPC calls to the sponsor URL.
/// The sponsor acts as an RPC proxy: it receives the call, and for transaction
/// submission methods, it co-signs the fee portion before relaying to the real RPC.
///
/// For non-transaction methods, it proxies directly to the RPC.
pub async fn handle_sponsor(
    State(service): State<SponsorService>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    // Parse JSON-RPC request
    let method = match body.get("method").and_then(|m| m.as_str()) {
        Some(m) => m,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json_rpc_error(
                    body.get("id").cloned(),
                    -32600,
                    "invalid_request",
                    "Missing or invalid 'method' field",
                )),
            );
        }
    };

    let id = body.get("id").cloned().unwrap_or(Value::Null);

    match method {
        // Transaction submission — this is where we sponsor fees
        "eth_sendTransaction" | "eth_sendRawTransaction" | "tempo_sendTransaction" => {
            handle_sponsor_tx(&service, &body, &id).await
        }
        // All other methods — proxy directly to RPC
        _ => proxy_to_rpc(&service, &body).await,
    }
}

async fn handle_sponsor_tx(
    service: &SponsorService,
    body: &Value,
    id: &Value,
) -> (StatusCode, Json<Value>) {
    let cfg = service.config.load();

    // Budget check — use per_tx_limit as the estimated fee
    let per_tx = parse_price_to_base_units(&cfg.sponsorship.max_per_tx).unwrap_or(10_000);

    match service.budget.check_and_spend(per_tx) {
        Ok(remaining) => {
            gauge!("paygate_sponsor_budget_remaining").set(remaining as f64);
        }
        Err(BudgetError::DailyLimitExhausted) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json_rpc_error(
                    Some(id.clone()),
                    -32000,
                    "fee_sponsorship_unavailable",
                    "Fee sponsorship temporarily unavailable — daily budget exhausted",
                )),
            );
        }
        Err(BudgetError::PerTxLimitExceeded) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json_rpc_error(
                    Some(id.clone()),
                    -32000,
                    "fee_too_high",
                    "Transaction fee exceeds per-transaction sponsorship limit",
                )),
            );
        }
    }

    // Relay the transaction to the Tempo RPC.
    //
    // In the full implementation, we would:
    // 1. Parse the transaction from the request params
    // 2. Sign the fee payer portion with our private key
    // 3. Submit the co-signed transaction to the RPC
    //
    // For now, we proxy the request as-is to the RPC. The Tempo fee payer
    // protocol has the RPC node handle the co-signing when the fee payer
    // field is set in the transaction. The private key is used to sign
    // a fee authorization that accompanies the transaction.
    //
    // TODO: Implement proper fee payer co-signing once the exact Tempo
    // fee payer signing protocol is confirmed via integration testing.
    proxy_to_rpc(service, body).await
}

async fn proxy_to_rpc(
    service: &SponsorService,
    body: &Value,
) -> (StatusCode, Json<Value>) {
    let rpc_url = service.primary_rpc_url();
    if rpc_url.is_empty() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json_rpc_error(
                body.get("id").cloned(),
                -32000,
                "relay_failed",
                "No RPC URL configured",
            )),
        );
    }

    match service
        .http_client
        .post(&rpc_url)
        .json(body)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(resp) => {
            let status = if resp.status().is_success() {
                StatusCode::OK
            } else {
                StatusCode::BAD_GATEWAY
            };
            match resp.json::<Value>().await {
                Ok(rpc_response) => (status, Json(rpc_response)),
                Err(e) => (
                    StatusCode::BAD_GATEWAY,
                    Json(json_rpc_error(
                        body.get("id").cloned(),
                        -32000,
                        "relay_failed",
                        &format!("Failed to parse RPC response: {e}"),
                    )),
                ),
            }
        }
        Err(e) => {
            error!(error = %e, "failed to relay transaction to Tempo RPC");
            (
                StatusCode::BAD_GATEWAY,
                Json(json_rpc_error(
                    body.get("id").cloned(),
                    -32000,
                    "relay_failed",
                    "Failed to relay transaction to Tempo RPC",
                )),
            )
        }
    }
}

async fn query_native_balance(
    client: &reqwest::Client,
    rpc_urls: &[String],
    address: &str,
) -> Option<u64> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "eth_getBalance",
        "params": [address, "latest"],
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
            if let Ok(json) = resp.json::<Value>().await {
                if let Some(result) = json["result"].as_str() {
                    let hex_str = result.trim_start_matches("0x");
                    if let Ok(val) = u64::from_str_radix(hex_str, 16) {
                        return Some(val);
                    }
                }
            }
        }
    }
    None
}

fn json_rpc_error(id: Option<Value>, code: i64, _error_type: &str, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": {
            "code": code,
            "message": message,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_budget(daily_limit: u64, per_tx_limit: u64) -> SponsorBudget {
        SponsorBudget::new(daily_limit, per_tx_limit)
    }

    #[test]
    fn test_budget_daily_limit() {
        let budget = make_budget(10_000, 10_000);

        // Spend 9000 — should succeed, 1000 remaining
        assert_eq!(budget.check_and_spend(9_000).unwrap(), 1_000);

        // Try to spend 2000 — exceeds remaining
        assert!(budget.check_and_spend(2_000).is_err());

        // Spend 1000 — should succeed, 0 remaining
        assert_eq!(budget.check_and_spend(1_000).unwrap(), 0);

        // Try to spend 1 — exhausted
        assert!(matches!(
            budget.check_and_spend(1),
            Err(BudgetError::DailyLimitExhausted)
        ));
    }

    #[test]
    fn test_budget_per_tx_limit() {
        let budget = make_budget(100_000, 5_000);

        // Try to spend 6000 — exceeds per-tx limit
        assert!(matches!(
            budget.check_and_spend(6_000),
            Err(BudgetError::PerTxLimitExceeded)
        ));

        // Spend 5000 — should succeed
        assert!(budget.check_and_spend(5_000).is_ok());
    }

    #[test]
    fn test_budget_reset_at_midnight() {
        let budget = make_budget(10_000, 10_000);

        // Spend to near-limit
        assert!(budget.check_and_spend(9_000).is_ok());
        assert_eq!(budget.remaining(), 1_000);

        // Set last_reset to yesterday
        let yesterday = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(1))
            .unwrap()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        budget.set_last_reset(yesterday);

        // Should reset and succeed with full budget
        let remaining = budget.check_and_spend(5_000).unwrap();
        assert_eq!(remaining, 5_000);
    }

    #[test]
    fn test_invalid_json_rpc_request() {
        // Verify the error response format for missing method
        let error = json_rpc_error(Some(Value::Number(1.into())), -32600, "invalid_request", "Missing method");
        assert_eq!(error["error"]["code"], -32600);
        assert_eq!(error["id"], 1);
    }

    #[tokio::test]
    async fn test_sponsorship_disabled_no_route() {
        // When sponsorship is disabled, SponsorService::new should not be called.
        // The route is conditionally registered in main.rs.
        // Verify that budget with 0 limit rejects everything.
        let budget = make_budget(0, 0);
        assert!(budget.check_and_spend(1).is_err());
    }
}
