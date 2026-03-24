# Build Brief: Pane Governance — Spend Governance + Agent Identity

Stream 2 of Wave 3. Gateway-side spend limits and agent identity tracking.

## Scope

1. `[governance]` config section with daily/monthly limits
2. `SpendAccumulator` in-memory tracker with UTC day/month reset
3. Spend limit enforcement in `verify_and_deduct()` (after HMAC, before balance deduction)
4. `X-Payment-Agent` header extraction and storage
5. `ALTER TABLE` migrations for `agent_name` on `sessions` and `request_log`
6. `GET /paygate/spend` endpoint with HMAC authentication (no balance deduction)
7. DB queries: `daily_spend_for_payer`, `monthly_spend_for_payer`, `daily_spend_for_agent`
8. `SessionError::SpendLimitExceeded` variant returning 402
9. ArcSwap config reload interaction with SpendAccumulator
10. At least 10 tests

## Files Modified

| File | Changes |
|------|---------|
| `crates/paygate-gateway/src/config.rs` | Add `GovernanceConfig` struct, add `governance` field to `Config`, validation, defaults |
| `crates/paygate-gateway/src/db.rs` | ALTER TABLE migrations, 3 new DbReader queries, `agent_name` on `InsertRequestLog` and `CreateSession` |
| `crates/paygate-gateway/src/sessions.rs` | `SpendAccumulator`, spend check in `verify_and_deduct()`, `SpendLimitExceeded` error variant, `handle_get_spend()` |
| `crates/paygate-gateway/src/serve.rs` | Extract `X-Payment-Agent` header, pass to `log_request()` and session creation, route `/paygate/spend`, handle `SpendLimitExceeded` |
| `crates/paygate-gateway/src/server.rs` | Add `spend_accumulator` field to `AppState` |
| `crates/paygate-common/src/mpp.rs` | Add `HEADER_PAYMENT_AGENT` constant |
| `schema.sql` | Add `agent_name` columns (documentation only; runtime migration via ALTER TABLE) |

---

## 1. Config: `GovernanceConfig`

### File: `crates/paygate-gateway/src/config.rs`

#### New struct

```rust
#[derive(Debug, Clone, Deserialize, Default)]
pub struct GovernanceConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_daily_limit")]
    pub default_daily_limit: String,     // USDC decimal, e.g. "10.00"
    #[serde(default = "default_monthly_limit")]
    pub default_monthly_limit: String,   // USDC decimal, e.g. "100.00"
}

fn default_daily_limit() -> String { "10.00".to_string() }
fn default_monthly_limit() -> String { "100.00".to_string() }
```

#### Add to Config struct

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    // ... existing fields ...
    #[serde(default)]
    pub governance: GovernanceConfig,
}
```

#### Validation

Add to `Config::validate()`:

```rust
// Governance limits
if self.governance.enabled {
    validate_price(&self.governance.default_daily_limit, "governance.default_daily_limit")?;
    validate_price(&self.governance.default_monthly_limit, "governance.default_monthly_limit")?;
}
```

#### Helper methods

```rust
impl GovernanceConfig {
    /// Daily limit in base units (6 decimals). Returns u64::MAX if governance disabled.
    pub fn daily_limit_base_units(&self) -> u64 {
        if !self.enabled {
            return u64::MAX;
        }
        parse_price_to_base_units(&self.default_daily_limit).unwrap_or(u64::MAX)
    }

    /// Monthly limit in base units (6 decimals). Returns u64::MAX if governance disabled.
    pub fn monthly_limit_base_units(&self) -> u64 {
        if !self.enabled {
            return u64::MAX;
        }
        parse_price_to_base_units(&self.default_monthly_limit).unwrap_or(u64::MAX)
    }
}
```

#### TOML example

```toml
[governance]
enabled = true
default_daily_limit = "10.00"
default_monthly_limit = "100.00"
```

---

## 2. Header constant

### File: `crates/paygate-common/src/mpp.rs`

```rust
pub const HEADER_PAYMENT_AGENT: &str = "X-Payment-Agent";
```

Also update `is_payment_header()` to include this header so it gets stripped before proxying to upstream.

---

## 3. Schema migration

### File: `crates/paygate-gateway/src/db.rs` (in `init_db`)

After `conn.execute_batch(include_str!("../../../schema.sql"))`, add:

```rust
// Migration: add agent_name columns (idempotent — catch "duplicate column" error)
for stmt in &[
    "ALTER TABLE sessions ADD COLUMN agent_name TEXT DEFAULT ''",
    "ALTER TABLE request_log ADD COLUMN agent_name TEXT DEFAULT ''",
] {
    match conn.execute(stmt, []) {
        Ok(_) => info!("Migration applied: {}", stmt),
        Err(e) if e.to_string().contains("duplicate column name") => {
            // Column already exists — safe to ignore
        }
        Err(e) => {
            error!("Migration failed: {}: {}", stmt, e);
            return Err(DbError::Sqlite(e));
        }
    }
}
```

### Index for spend queries

```rust
// Index for spend tracking queries
conn.execute_batch(
    "CREATE INDEX IF NOT EXISTS idx_request_log_payer_created ON request_log(payer_address, created_at);
     CREATE INDEX IF NOT EXISTS idx_request_log_agent ON request_log(agent_name, created_at);"
)?;
```

---

## 4. DB queries

### File: `crates/paygate-gateway/src/db.rs` — new methods on `DbReader`

```rust
/// Total spend for a payer today (UTC day). Returns base units.
pub fn daily_spend_for_payer(&self, payer: &str) -> Result<u64, DbError> {
    let conn = self.conn()?;
    let today_start = utc_day_start();
    let total: i64 = conn.query_row(
        "SELECT COALESCE(SUM(amount_charged), 0) FROM request_log
         WHERE payer_address = ? AND created_at >= ?",
        params![payer, today_start],
        |row| row.get(0),
    )?;
    Ok(total as u64)
}

/// Total spend for a payer this month (UTC month). Returns base units.
pub fn monthly_spend_for_payer(&self, payer: &str) -> Result<u64, DbError> {
    let conn = self.conn()?;
    let month_start = utc_month_start();
    let total: i64 = conn.query_row(
        "SELECT COALESCE(SUM(amount_charged), 0) FROM request_log
         WHERE payer_address = ? AND created_at >= ?",
        params![payer, month_start],
        |row| row.get(0),
    )?;
    Ok(total as u64)
}

/// Total spend for a specific agent of a payer today (UTC day). Returns base units.
pub fn daily_spend_for_agent(&self, payer: &str, agent: &str) -> Result<u64, DbError> {
    let conn = self.conn()?;
    let today_start = utc_day_start();
    let total: i64 = conn.query_row(
        "SELECT COALESCE(SUM(amount_charged), 0) FROM request_log
         WHERE payer_address = ? AND agent_name = ? AND created_at >= ?",
        params![payer, agent, today_start],
        |row| row.get(0),
    )?;
    Ok(total as u64)
}
```

### Timestamp helpers (module-level in db.rs)

```rust
/// Start of current UTC day as unix timestamp.
fn utc_day_start() -> i64 {
    let now = chrono::Utc::now();
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp()
}

/// Start of current UTC month as unix timestamp.
fn utc_month_start() -> i64 {
    let now = chrono::Utc::now();
    chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp()
}
```

---

## 5. WriteCommand changes for agent_name

### File: `crates/paygate-gateway/src/db.rs`

#### Update `WriteCommand::InsertRequestLog`

```rust
InsertRequestLog {
    payment_id: Option<String>,
    session_id: Option<String>,
    endpoint: String,
    payer_address: String,
    amount_charged: BaseUnits,
    upstream_status: Option<i32>,
    upstream_latency_ms: Option<i64>,
    agent_name: String,               // NEW
},
```

#### Update `WriteCommand::CreateSession`

The `FullSessionRecord` struct gets a new field:

```rust
pub struct FullSessionRecord {
    // ... existing fields ...
    pub agent_name: String,   // NEW — default ""
}
```

#### Update flush_batch InsertRequestLog handler

```rust
WriteCommand::InsertRequestLog {
    payment_id,
    session_id,
    endpoint,
    payer_address,
    amount_charged,
    upstream_status,
    upstream_latency_ms,
    agent_name,
} => {
    let now = chrono::Utc::now().timestamp();
    let _ = conn.execute(
        "INSERT INTO request_log (payment_id, session_id, endpoint, payer_address,
         amount_charged, upstream_status, upstream_latency_ms, agent_name, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            payment_id,
            session_id,
            endpoint,
            payer_address,
            amount_charged as i64,
            upstream_status,
            upstream_latency_ms,
            agent_name,
            now,
        ],
    );
}
```

#### Update flush_batch CreateSession handler

Add `agent_name` to the INSERT INTO sessions:

```rust
conn.execute(
    "INSERT INTO sessions (id, secret, payer_address, deposit_tx, nonce,
     deposit_amount, balance, rate_per_request, requests_made,
     created_at, expires_at, status, agent_name)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    params![
        record.id, record.secret, record.payer_address,
        record.deposit_tx, record.nonce,
        record.deposit_amount as i64, record.balance as i64,
        record.rate_per_request as i64, record.requests_made as i64,
        record.created_at, record.expires_at, record.status,
        record.agent_name,
    ],
)
```

#### Update `DbWriter::log_request` signature

```rust
pub async fn log_request(
    &self,
    payment_id: Option<String>,
    session_id: Option<String>,
    endpoint: String,
    payer_address: String,
    amount_charged: BaseUnits,
    upstream_status: Option<i32>,
    upstream_latency_ms: Option<i64>,
    agent_name: String,               // NEW
) -> Result<(), DbError> {
    self.tx
        .try_send(WriteCommand::InsertRequestLog {
            payment_id,
            session_id,
            endpoint,
            payer_address,
            amount_charged,
            upstream_status,
            upstream_latency_ms,
            agent_name,
        })
        .map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => DbError::Backpressure,
            mpsc::error::TrySendError::Closed(_) => DbError::ChannelClosed,
        })?;
    Ok(())
}
```

---

## 6. SpendAccumulator

### File: `crates/paygate-gateway/src/sessions.rs`

```rust
use std::collections::HashMap;
use std::sync::Mutex;

/// Key for spend tracking: (payer_address_lowercase, agent_name).
/// An empty agent_name tracks payer-level totals.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct SpendKey {
    pub payer: String,       // lowercased
    pub agent: String,       // "" for payer-level
}

/// Running totals for a single (payer, agent) pair.
#[derive(Debug, Clone)]
pub struct Accumulator {
    pub daily_total: u64,
    pub monthly_total: u64,
    pub current_day: u32,    // day-of-year (1-366) for reset detection
    pub current_month: u32,  // month (1-12) for reset detection
    pub current_year: i32,   // year for reset detection
}

impl Accumulator {
    fn new_now() -> Self {
        let now = chrono::Utc::now();
        Self {
            daily_total: 0,
            monthly_total: 0,
            current_day: now.ordinal(),
            current_month: now.month(),
            current_year: now.year(),
        }
    }

    /// Check if the day/month has rolled over and reset if needed.
    /// Called on every access (check-on-access pattern).
    fn maybe_reset(&mut self) {
        let now = chrono::Utc::now();
        let today = now.ordinal();
        let this_month = now.month();
        let this_year = now.year();

        // Year changed => reset both
        if this_year != self.current_year {
            self.daily_total = 0;
            self.monthly_total = 0;
            self.current_year = this_year;
            self.current_day = today;
            self.current_month = this_month;
            return;
        }

        // Month changed => reset both (monthly resets daily too)
        if this_month != self.current_month {
            self.daily_total = 0;
            self.monthly_total = 0;
            self.current_month = this_month;
            self.current_day = today;
            return;
        }

        // Day changed => reset daily only
        if today != self.current_day {
            self.daily_total = 0;
            self.current_day = today;
        }
    }
}

/// Thread-safe in-memory spend tracker.
/// Updated synchronously when verify_and_deduct succeeds (before async DB write).
/// Reloaded from DB on gateway startup.
pub struct SpendAccumulator {
    inner: Mutex<HashMap<SpendKey, Accumulator>>,
}

impl SpendAccumulator {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Load initial totals from DB on startup.
    pub fn load_from_db(db_reader: &crate::db::DbReader) -> Self {
        let acc = Self::new();
        // We don't pre-populate — the first check for each payer will
        // query the DB via daily_spend_for_payer/monthly_spend_for_payer
        // and seed the accumulator. This avoids scanning the full table.
        acc
    }

    /// Check if adding `amount` would exceed daily or monthly limits.
    /// Returns Ok(()) if within limits, Err with period info if exceeded.
    ///
    /// IMPORTANT: This method acquires the Mutex. Do NOT hold across await points.
    pub fn check_limits(
        &self,
        payer: &str,
        agent: &str,
        amount: u64,
        daily_limit: u64,
        monthly_limit: u64,
    ) -> Result<(), SpendLimitInfo> {
        let mut map = self.inner.lock().unwrap();

        // Check payer-level accumulator (agent = "")
        let payer_key = SpendKey {
            payer: payer.to_lowercase(),
            agent: String::new(),
        };
        let payer_acc = map.entry(payer_key).or_insert_with(Accumulator::new_now);
        payer_acc.maybe_reset();

        if payer_acc.daily_total + amount > daily_limit {
            return Err(SpendLimitInfo {
                period: "daily".to_string(),
                limit: daily_limit,
                spent: payer_acc.daily_total,
            });
        }
        if payer_acc.monthly_total + amount > monthly_limit {
            return Err(SpendLimitInfo {
                period: "monthly".to_string(),
                limit: monthly_limit,
                spent: payer_acc.monthly_total,
            });
        }

        Ok(())
    }

    /// Record a successful spend. Called AFTER verify_and_deduct succeeds
    /// but BEFORE the async DB write, so the in-memory total is always
    /// ahead of or equal to the DB.
    pub fn record_spend(&self, payer: &str, agent: &str, amount: u64) {
        let mut map = self.inner.lock().unwrap();

        // Update payer-level accumulator
        let payer_key = SpendKey {
            payer: payer.to_lowercase(),
            agent: String::new(),
        };
        let payer_acc = map.entry(payer_key).or_insert_with(Accumulator::new_now);
        payer_acc.maybe_reset();
        payer_acc.daily_total += amount;
        payer_acc.monthly_total += amount;

        // Update agent-level accumulator (if agent is non-empty)
        if !agent.is_empty() {
            let agent_key = SpendKey {
                payer: payer.to_lowercase(),
                agent: agent.to_string(),
            };
            let agent_acc = map.entry(agent_key).or_insert_with(Accumulator::new_now);
            agent_acc.maybe_reset();
            agent_acc.daily_total += amount;
            agent_acc.monthly_total += amount;
        }
    }

    /// Get current spend totals for a payer. Returns (daily, monthly).
    /// Used by GET /paygate/spend endpoint.
    pub fn get_payer_totals(&self, payer: &str) -> (u64, u64) {
        let mut map = self.inner.lock().unwrap();
        let key = SpendKey {
            payer: payer.to_lowercase(),
            agent: String::new(),
        };
        match map.get_mut(&key) {
            Some(acc) => {
                acc.maybe_reset();
                (acc.daily_total, acc.monthly_total)
            }
            None => (0, 0),
        }
    }

    /// Get current spend totals for a specific agent. Returns (daily, monthly).
    pub fn get_agent_totals(&self, payer: &str, agent: &str) -> (u64, u64) {
        let mut map = self.inner.lock().unwrap();
        let key = SpendKey {
            payer: payer.to_lowercase(),
            agent: agent.to_string(),
        };
        match map.get_mut(&key) {
            Some(acc) => {
                acc.maybe_reset();
                (acc.daily_total, acc.monthly_total)
            }
            None => (0, 0),
        }
    }

    /// Seed accumulator from DB values. Called on first access for a payer
    /// or after gateway restart.
    pub fn seed_from_db(
        &self,
        payer: &str,
        daily_from_db: u64,
        monthly_from_db: u64,
    ) {
        let mut map = self.inner.lock().unwrap();
        let key = SpendKey {
            payer: payer.to_lowercase(),
            agent: String::new(),
        };
        let acc = map.entry(key).or_insert_with(Accumulator::new_now);
        acc.maybe_reset();
        // Only seed if accumulator is empty (first access)
        if acc.daily_total == 0 && acc.monthly_total == 0 {
            acc.daily_total = daily_from_db;
            acc.monthly_total = monthly_from_db;
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpendLimitInfo {
    pub period: String,   // "daily" or "monthly"
    pub limit: u64,       // base units
    pub spent: u64,       // base units
}
```

---

## 7. SessionError::SpendLimitExceeded

### File: `crates/paygate-gateway/src/sessions.rs`

```rust
#[derive(Debug)]
pub enum SessionError {
    MissingHeaders,
    SessionNotFound,
    SessionExpired,
    InsufficientBalance { balance: u64, rate: u64 },
    InvalidSignature,
    StaleTimestamp,
    DeductionFailed,
    SpendLimitExceeded {              // NEW
        period: String,
        limit: u64,
        spent: u64,
    },
}
```

---

## 8. Modified `verify_and_deduct()`

### File: `crates/paygate-gateway/src/sessions.rs`

The new signature adds `spend_accumulator` and `agent_name`:

```rust
pub async fn verify_and_deduct(
    state: &AppState,
    headers: &HeaderMap,
    request_hash: &B256,
    endpoint: &str,
    agent_name: &str,          // NEW — extracted from X-Payment-Agent by caller
) -> Result<SessionDeduction, SessionError> {
    // 1. Extract headers (unchanged)
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

    // 2. Look up session (unchanged)
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

    // 3. Verify timestamp freshness (unchanged)
    let ts: i64 = timestamp_str.parse().map_err(|_| SessionError::StaleTimestamp)?;
    if (now - ts).unsigned_abs() > 60 {
        return Err(SessionError::StaleTimestamp);
    }

    // 4. HMAC verification (unchanged)
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

    // 5. Determine rate for this endpoint (unchanged)
    let config = state.current_config();
    let rate = config.price_for_endpoint(endpoint);

    // ── NEW: 5b. Spend limit check ──────────────────────────────────
    if config.governance.enabled {
        let daily_limit = config.governance.daily_limit_base_units();
        let monthly_limit = config.governance.monthly_limit_base_units();

        // Seed accumulator from DB on first access for this payer
        // (cheap — only queries once, then in-memory)
        {
            let (daily, monthly) = state.spend_accumulator.get_payer_totals(&session.payer_address);
            if daily == 0 && monthly == 0 {
                if let (Ok(db_daily), Ok(db_monthly)) = (
                    state.db_reader.daily_spend_for_payer(&session.payer_address),
                    state.db_reader.monthly_spend_for_payer(&session.payer_address),
                ) {
                    state.spend_accumulator.seed_from_db(
                        &session.payer_address,
                        db_daily,
                        db_monthly,
                    );
                }
            }
        }

        if let Err(info) = state.spend_accumulator.check_limits(
            &session.payer_address,
            agent_name,
            rate,
            daily_limit,
            monthly_limit,
        ) {
            return Err(SessionError::SpendLimitExceeded {
                period: info.period,
                limit: info.limit,
                spent: info.spent,
            });
        }
    }
    // ── END spend limit check ────────────────────────────────────────

    // 6. Atomically deduct (unchanged)
    let deducted = state.db_writer.deduct_session_balance(&session.id, rate)
        .await
        .map_err(|_| SessionError::DeductionFailed)?;

    if !deducted {
        return Err(SessionError::InsufficientBalance {
            balance: session.balance,
            rate,
        });
    }

    // ── NEW: Record spend in accumulator (after successful deduction) ──
    if config.governance.enabled {
        state.spend_accumulator.record_spend(
            &session.payer_address,
            agent_name,
            rate,
        );
    }

    Ok(SessionDeduction {
        session_id: session.id.clone(),
        payer_address: session.payer_address.clone(),
        amount_deducted: rate,
        remaining_balance: session.balance.saturating_sub(rate),
    })
}
```

---

## 9. AppState change

### File: `crates/paygate-gateway/src/server.rs`

```rust
use crate::sessions::SpendAccumulator;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ArcSwap<Config>>,
    pub db_reader: DbReader,
    pub db_writer: DbWriter,
    pub http_client: reqwest::Client,
    pub rate_limiter: Arc<RateLimiter>,
    pub webhook_sender: Option<WebhookSender>,
    pub prometheus_handle: PrometheusHandle,
    pub started_at: std::time::Instant,
    pub spend_accumulator: Arc<SpendAccumulator>,  // NEW
}
```

Note: `SpendAccumulator` contains a `Mutex<HashMap<...>>`, which is `!Clone`. Wrap in `Arc` so `AppState` remains `Clone`.

---

## 10. serve.rs changes

### File: `crates/paygate-gateway/src/serve.rs`

#### 10a. Construct SpendAccumulator in `cmd_serve`

After creating `AppState`, add:

```rust
let spend_accumulator = Arc::new(SpendAccumulator::new());
// ... include in AppState construction:
let state = AppState {
    // ... existing fields ...
    spend_accumulator,
};
```

#### 10b. Extract X-Payment-Agent in gateway_handler (session branch)

In the session auth block (line ~335), extract agent name from headers:

```rust
// Session auth: HMAC-based
if parts.headers.contains_key("x-payment-session") {
    let request_hash = paygate_common::hash::request_hash(&method, &path, &body_bytes);

    // Extract agent name (optional header)
    let agent_name = parts.headers
        .get(paygate_common::mpp::HEADER_PAYMENT_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    match sessions::verify_and_deduct(&state, &parts.headers, &request_hash, &endpoint, &agent_name).await {
```

#### 10c. Pass agent_name to log_request calls

Every call to `state.db_writer.log_request(...)` in the session branch must include the extracted `agent_name`:

```rust
let _ = state.db_writer.log_request(
    None,
    Some(deduction.session_id),
    endpoint,
    deduction.payer_address,
    final_cost,
    Some(resp.status().as_u16() as i32),
    None,
    agent_name.clone(),  // NEW
).await;
```

For non-session (per-request payment) code paths, pass `String::new()` as agent_name.

#### 10d. Handle SpendLimitExceeded in error match

Add a new arm in the session error match block:

```rust
Err(sessions::SessionError::SpendLimitExceeded { period, limit, spent }) => {
    let limit_str = format!("{:.6}", limit as f64 / 1_000_000.0);
    let spent_str = format!("{:.6}", spent as f64 / 1_000_000.0);
    return (StatusCode::PAYMENT_REQUIRED, Json(json!({
        "error": "spend_limit_exceeded",
        "message": format!("{} spend limit exceeded", period),
        "period": period,
        "limit": limit_str,
        "spent": spent_str,
    }))).into_response();
}
```

NOTE: This returns 402 (Payment Required), NOT 429 (Too Many Requests). Spend limits are a payment-domain concept.

#### 10e. Add `/paygate/spend` route

In the router construction:

```rust
let mut gateway_app = Router::new()
    .route("/paygate/sessions/nonce", axum::routing::post(sessions::handle_nonce))
    .route("/paygate/sessions", axum::routing::post(sessions::handle_create_session)
        .get(sessions::handle_get_sessions))
    .route("/paygate/spend", axum::routing::get(sessions::handle_get_spend))  // NEW
    .merge(admin::receipt_route())
    .merge(admin::transactions_route())
    .fallback(gateway_handler)
    // ... rest unchanged
```

#### 10f. ArcSwap config reload interaction

In the SIGHUP handler, after storing new config, the SpendAccumulator does NOT need to be reset. The accumulator tracks running totals; new limits from the reloaded config will be read on the next `check_limits()` call via `state.current_config()`. The totals persist across reloads, which is correct behavior (changing the limit doesn't reset what you've already spent).

No code change needed here -- just documenting the interaction.

---

## 11. GET /paygate/spend handler

### File: `crates/paygate-gateway/src/sessions.rs`

```rust
/// GET /paygate/spend?payer=0x...&agent=my-agent
///
/// Requires HMAC authentication (same pattern as session auth).
/// Does NOT deduct session balance.
///
/// Required headers:
///   X-Payment-Session: <session_id>
///   X-Payment-Session-Sig: HMAC-SHA256(secret, "GET /paygate/spend" || timestamp)
///   X-Payment-Timestamp: <unix_timestamp>
pub async fn handle_get_spend(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
    req: Request,
) -> Response {
    let headers = req.headers();

    // 1. Extract HMAC auth headers
    let session_id = match headers.get(mpp::HEADER_PAYMENT_SESSION).and_then(|v| v.to_str().ok()) {
        Some(s) => s.to_string(),
        None => {
            return (StatusCode::UNAUTHORIZED, Json(json!({
                "error": "auth_required",
                "message": "Valid session required to query spend data"
            }))).into_response();
        }
    };

    let sig_header = match headers.get(mpp::HEADER_PAYMENT_SESSION_SIG).and_then(|v| v.to_str().ok()) {
        Some(s) => s.to_string(),
        None => {
            return (StatusCode::UNAUTHORIZED, Json(json!({
                "error": "auth_required",
                "message": "Valid session required to query spend data"
            }))).into_response();
        }
    };

    let timestamp_str = match headers.get(mpp::HEADER_PAYMENT_TIMESTAMP).and_then(|v| v.to_str().ok()) {
        Some(s) => s.to_string(),
        None => {
            return (StatusCode::UNAUTHORIZED, Json(json!({
                "error": "auth_required",
                "message": "Valid session required to query spend data"
            }))).into_response();
        }
    };

    // 2. Look up session
    let session = match state.db_reader.get_session(&session_id) {
        Ok(Some(s)) => s,
        _ => {
            return (StatusCode::UNAUTHORIZED, Json(json!({
                "error": "auth_required",
                "message": "Valid session required to query spend data"
            }))).into_response();
        }
    };

    if session.status != "active" {
        return (StatusCode::UNAUTHORIZED, Json(json!({
            "error": "auth_required",
            "message": "Session expired or inactive"
        }))).into_response();
    }

    let now = chrono::Utc::now().timestamp();
    if session.expires_at <= now {
        return (StatusCode::UNAUTHORIZED, Json(json!({
            "error": "auth_required",
            "message": "Session expired"
        }))).into_response();
    }

    // 3. Verify timestamp freshness
    let ts: i64 = match timestamp_str.parse() {
        Ok(t) => t,
        Err(_) => {
            return (StatusCode::FORBIDDEN, Json(json!({
                "error": "invalid_session_auth",
                "message": "Invalid timestamp"
            }))).into_response();
        }
    };
    if (now - ts).unsigned_abs() > 60 {
        return (StatusCode::FORBIDDEN, Json(json!({
            "error": "stale_timestamp",
            "message": "Timestamp too old"
        }))).into_response();
    }

    // 4. HMAC verification
    // Message: "GET /paygate/spend" || timestamp
    let raw_secret = session.secret.strip_prefix("ssec_").unwrap_or(&session.secret);
    let key_bytes = match hex::decode(raw_secret) {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "internal error"}))).into_response();
        }
    };

    let request_hash = paygate_common::hash::request_hash("GET", "/paygate/spend", &[]);
    let rh_hex = format!("0x{}", hex::encode(request_hash.as_slice()));

    let mut mac = match HmacSha256::new_from_slice(&key_bytes) {
        Ok(m) => m,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "internal error"}))).into_response();
        }
    };
    mac.update(rh_hex.as_bytes());
    mac.update(timestamp_str.as_bytes());

    let sig_hex = sig_header.strip_prefix("0x").unwrap_or(&sig_header);
    let sig_bytes = match hex::decode(sig_hex) {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::FORBIDDEN, Json(json!({"error": "invalid_session_auth"}))).into_response();
        }
    };
    if mac.verify_slice(&sig_bytes).is_err() {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "invalid_session_auth"}))).into_response();
    }

    // 5. Verify payer matches session (if payer query param provided)
    let payer = params.get("payer")
        .map(|p| p.to_string())
        .unwrap_or_else(|| session.payer_address.clone());

    if payer.to_lowercase() != session.payer_address.to_lowercase() {
        return (StatusCode::FORBIDDEN, Json(json!({
            "error": "payer_mismatch",
            "message": "Session does not belong to requested payer"
        }))).into_response();
    }

    let agent = params.get("agent").cloned().unwrap_or_default();

    // 6. Get spend data
    let config = state.current_config();
    let daily_limit = config.governance.daily_limit_base_units();
    let monthly_limit = config.governance.monthly_limit_base_units();

    let (daily_spent, monthly_spent) = if !agent.is_empty() {
        state.spend_accumulator.get_agent_totals(&payer, &agent)
    } else {
        state.spend_accumulator.get_payer_totals(&payer)
    };

    let remaining_daily = daily_limit.saturating_sub(daily_spent);
    let remaining_monthly = monthly_limit.saturating_sub(monthly_spent);

    let fmt = |v: u64| -> String {
        if v == u64::MAX { "unlimited".to_string() }
        else { format!("{:.6}", v as f64 / 1_000_000.0) }
    };

    (StatusCode::OK, Json(json!({
        "daily_spent": fmt(daily_spent),
        "daily_limit": fmt(daily_limit),
        "monthly_spent": fmt(monthly_spent),
        "monthly_limit": fmt(monthly_limit),
        "remaining_daily": fmt(remaining_daily),
        "remaining_monthly": fmt(remaining_monthly),
        "governance_enabled": config.governance.enabled,
    }))).into_response()
}
```

---

## 12. handle_get_sessions: include agentName

### File: `crates/paygate-gateway/src/sessions.rs`

Update the session JSON in `handle_get_sessions` to include `agentName`:

```rust
let session_json: Vec<serde_json::Value> = active.iter().map(|s| {
    json!({
        "sessionId": s.id,
        "balance": format!("{:.6}", s.balance as f64 / 1_000_000.0),
        "ratePerRequest": format!("{:.6}", s.rate_per_request as f64 / 1_000_000.0),
        "requestsMade": s.requests_made,
        "expiresAt": chrono::DateTime::from_timestamp(s.expires_at, 0)
            .map(|d| d.to_rfc3339()).unwrap_or_default(),
        "status": s.status,
        "agentName": s.agent_name,   // NEW
    })
}).collect();
```

This requires `FullSessionRecord` to have `agent_name` and the SELECT in `list_sessions_for_payer` to include it.

Update `get_session` and `list_sessions_for_payer` queries in `DbReader`:

```sql
SELECT id, secret, payer_address, deposit_tx, nonce, deposit_amount, balance,
       rate_per_request, requests_made, created_at, expires_at, status,
       COALESCE(agent_name, '') as agent_name
FROM sessions WHERE ...
```

And the row mapping:

```rust
Ok(FullSessionRecord {
    // ... existing fields ...
    agent_name: row.get::<_, String>(12)?,
})
```

---

## 13. Session creation: store agent_name

### File: `crates/paygate-gateway/src/sessions.rs` — in `handle_create_session`

Extract agent from headers and include in `FullSessionRecord`:

```rust
// Extract agent name (optional)
let agent_name = req.headers()
    .get(paygate_common::mpp::HEADER_PAYMENT_AGENT)
    .and_then(|v| v.to_str().ok())
    .unwrap_or("")
    .to_string();

// ... later in session construction:
let session = FullSessionRecord {
    // ... existing fields ...
    agent_name,
};
```

But wait -- `handle_create_session` consumes `req` for the body. The agent header must be extracted BEFORE consuming the body. Move the extraction before `axum::body::to_bytes`:

```rust
pub async fn handle_create_session(State(state): State<AppState>, req: Request) -> Response {
    let tx_hash = match req.headers().get(mpp::HEADER_PAYMENT_TX).and_then(|v| v.to_str().ok()) {
        Some(t) => t.to_string(),
        None => { /* ... */ }
    };
    let payer = match req.headers().get(mpp::HEADER_PAYMENT_PAYER).and_then(|v| v.to_str().ok()) {
        Some(p) => p.to_string(),
        None => { /* ... */ }
    };
    // NEW: Extract agent name before consuming body
    let agent_name = req.headers()
        .get(paygate_common::mpp::HEADER_PAYMENT_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Parse body for nonce
    let body_bytes = match axum::body::to_bytes(req.into_body(), 4096).await {
        // ...
    };
    // ... rest unchanged, but use agent_name in FullSessionRecord construction
```

---

## 14. Tests (minimum 10)

All tests go in `#[cfg(test)] mod tests` blocks in the respective files.

### Test 1: GovernanceConfig parsing (config.rs)

```rust
#[test]
fn test_governance_config_parsing() {
    let toml = r#"
[gateway]
upstream = "http://localhost:3000"
[tempo]
rpc_urls = ["https://rpc.example.com"]
[provider]
address = "0x7F3a000000000000000000000000000000000001"
[governance]
enabled = true
default_daily_limit = "5.00"
default_monthly_limit = "50.00"
"#;
    let config: Config = toml::from_str(toml).unwrap();
    config.validate().unwrap();
    assert!(config.governance.enabled);
    assert_eq!(config.governance.daily_limit_base_units(), 5_000_000);
    assert_eq!(config.governance.monthly_limit_base_units(), 50_000_000);
}
```

### Test 2: GovernanceConfig defaults when missing (config.rs)

```rust
#[test]
fn test_governance_config_defaults() {
    let toml = r#"
[gateway]
upstream = "http://localhost:3000"
[tempo]
rpc_urls = ["https://rpc.example.com"]
[provider]
address = "0x7F3a000000000000000000000000000000000001"
"#;
    let config: Config = toml::from_str(toml).unwrap();
    assert!(!config.governance.enabled);
    // When disabled, limits should be u64::MAX (effectively unlimited)
    assert_eq!(config.governance.daily_limit_base_units(), u64::MAX);
    assert_eq!(config.governance.monthly_limit_base_units(), u64::MAX);
}
```

### Test 3: SpendAccumulator check_limits within budget (sessions.rs)

```rust
#[test]
fn test_spend_accumulator_within_limits() {
    let acc = SpendAccumulator::new();
    let payer = "0x9E2b000000000000000000000000000000000001";
    let daily_limit = 10_000_000;   // $10
    let monthly_limit = 100_000_000; // $100

    // First spend: $1
    let result = acc.check_limits(payer, "", 1_000_000, daily_limit, monthly_limit);
    assert!(result.is_ok());
    acc.record_spend(payer, "", 1_000_000);

    // Second spend: $2
    let result = acc.check_limits(payer, "", 2_000_000, daily_limit, monthly_limit);
    assert!(result.is_ok());
    acc.record_spend(payer, "", 2_000_000);

    let (daily, monthly) = acc.get_payer_totals(payer);
    assert_eq!(daily, 3_000_000);
    assert_eq!(monthly, 3_000_000);
}
```

### Test 4: SpendAccumulator daily limit exceeded (sessions.rs)

```rust
#[test]
fn test_spend_accumulator_daily_limit_exceeded() {
    let acc = SpendAccumulator::new();
    let payer = "0x9E2b000000000000000000000000000000000001";
    let daily_limit = 5_000_000;    // $5
    let monthly_limit = 100_000_000; // $100

    // Spend $4.99
    acc.record_spend(payer, "", 4_990_000);

    // Try to spend $0.02 more — exceeds daily limit
    let result = acc.check_limits(payer, "", 20_000, daily_limit, monthly_limit);
    assert!(result.is_err());
    let info = result.unwrap_err();
    assert_eq!(info.period, "daily");
    assert_eq!(info.limit, 5_000_000);
    assert_eq!(info.spent, 4_990_000);
}
```

### Test 5: SpendAccumulator monthly limit exceeded (sessions.rs)

```rust
#[test]
fn test_spend_accumulator_monthly_limit_exceeded() {
    let acc = SpendAccumulator::new();
    let payer = "0x9E2b000000000000000000000000000000000001";
    let daily_limit = 100_000_000;  // $100 (high daily)
    let monthly_limit = 5_000_000;  // $5 (low monthly)

    // Spend $4.99
    acc.record_spend(payer, "", 4_990_000);

    // Try to spend $0.02 more — exceeds monthly limit
    let result = acc.check_limits(payer, "", 20_000, daily_limit, monthly_limit);
    assert!(result.is_err());
    let info = result.unwrap_err();
    assert_eq!(info.period, "monthly");
}
```

### Test 6: SpendAccumulator tracks agents independently (sessions.rs)

```rust
#[test]
fn test_spend_accumulator_agent_tracking() {
    let acc = SpendAccumulator::new();
    let payer = "0x9E2b000000000000000000000000000000000001";

    // Agent A spends $2
    acc.record_spend(payer, "agent-a", 2_000_000);

    // Agent B spends $3
    acc.record_spend(payer, "agent-b", 3_000_000);

    // Agent-level totals
    let (daily_a, _) = acc.get_agent_totals(payer, "agent-a");
    assert_eq!(daily_a, 2_000_000);

    let (daily_b, _) = acc.get_agent_totals(payer, "agent-b");
    assert_eq!(daily_b, 3_000_000);

    // Payer-level total = sum of all agents
    let (daily_payer, _) = acc.get_payer_totals(payer);
    assert_eq!(daily_payer, 5_000_000);
}
```

### Test 7: SpendAccumulator seed_from_db (sessions.rs)

```rust
#[test]
fn test_spend_accumulator_seed_from_db() {
    let acc = SpendAccumulator::new();
    let payer = "0xaaa";

    // Seed with DB values (gateway restart scenario)
    acc.seed_from_db(payer, 3_000_000, 15_000_000);

    let (daily, monthly) = acc.get_payer_totals(payer);
    assert_eq!(daily, 3_000_000);
    assert_eq!(monthly, 15_000_000);

    // Additional spend stacks on top
    acc.record_spend(payer, "", 1_000_000);
    let (daily, monthly) = acc.get_payer_totals(payer);
    assert_eq!(daily, 4_000_000);
    assert_eq!(monthly, 16_000_000);
}
```

### Test 8: SpendLimitExceeded returns 402 (sessions.rs integration)

```rust
#[tokio::test]
async fn test_spend_limit_exceeded_returns_402() {
    let (state, db_path) = test_state_with_governance().await;

    // Insert a session with sufficient balance
    let session_secret = "ssec_deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    let session_id = "sess_gov_test";
    let payer = "0x9E2b000000000000000000000000000000000001";
    let future = chrono::Utc::now().timestamp() + 86400;
    insert_session(&db_path, session_id, session_secret, payer, 50_000_000, future);

    // Pre-load accumulator to just under limit
    // Daily limit is $5.00 = 5_000_000 base units
    state.spend_accumulator.record_spend(payer, "", 4_999_000);

    // Now try verify_and_deduct — rate is 1000 base units
    let rh = B256::repeat_byte(0xAB);
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

    let result = verify_and_deduct(&state, &headers, &rh, "POST /v1/chat", "").await;
    assert!(matches!(result, Err(SessionError::SpendLimitExceeded { .. })));

    if let Err(SessionError::SpendLimitExceeded { period, .. }) = result {
        assert_eq!(period, "daily");
    }
}

/// Helper: create test state with governance enabled (daily=$5, monthly=$50)
async fn test_state_with_governance() -> (AppState, String) {
    let db_path = format!("/tmp/paygate_gov_test_{}.db", uuid::Uuid::new_v4());
    let (db_reader, db_writer) = crate::db::init_db(&db_path).unwrap();

    let mut config = test_config();
    config.governance = GovernanceConfig {
        enabled: true,
        default_daily_limit: "5.00".to_string(),
        default_monthly_limit: "50.00".to_string(),
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
        spend_accumulator: Arc::new(SpendAccumulator::new()),
    };
    (state, db_path)
}
```

### Test 9: Governance disabled allows unlimited spend (sessions.rs)

```rust
#[tokio::test]
async fn test_governance_disabled_allows_unlimited() {
    let (state, db_path) = test_state().await;
    // test_state() uses default config with governance.enabled = false

    let session_secret = "ssec_deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    let session_id = "sess_no_gov";
    let payer = "0x9E2b000000000000000000000000000000000001";
    let future = chrono::Utc::now().timestamp() + 86400;
    insert_session(&db_path, session_id, session_secret, payer, 50_000_000, future);

    let rh = B256::repeat_byte(0xAB);
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

    // Should succeed even without any spend limit configuration
    let result = verify_and_deduct(&state, &headers, &rh, "POST /v1/chat", "").await;
    assert!(result.is_ok());
}
```

### Test 10: DB daily_spend_for_payer query (db.rs)

```rust
#[test]
fn test_daily_spend_for_payer() {
    let (path, reader) = setup_test_db();
    let conn = Connection::open(&path).unwrap();

    // Apply migration
    let _ = conn.execute("ALTER TABLE request_log ADD COLUMN agent_name TEXT DEFAULT ''", []);

    let now = chrono::Utc::now().timestamp();
    let yesterday = now - 86400 - 100;  // > 24h ago
    let payer = "0x9E2b000000000000000000000000000000000001";

    // Insert today's spend
    conn.execute(
        "INSERT INTO request_log (payment_id, endpoint, payer_address, amount_charged, agent_name, created_at)
         VALUES ('p1', 'POST /v1/chat', ?, 5000, '', ?)",
        params![payer, now - 100],
    ).unwrap();
    conn.execute(
        "INSERT INTO request_log (payment_id, endpoint, payer_address, amount_charged, agent_name, created_at)
         VALUES ('p2', 'POST /v1/chat', ?, 3000, '', ?)",
        params![payer, now - 50],
    ).unwrap();

    // Insert yesterday's spend (should NOT be counted)
    conn.execute(
        "INSERT INTO request_log (payment_id, endpoint, payer_address, amount_charged, agent_name, created_at)
         VALUES ('p3', 'POST /v1/chat', ?, 10000, '', ?)",
        params![payer, yesterday],
    ).unwrap();
    drop(conn);

    let daily = reader.daily_spend_for_payer(payer).unwrap();
    assert_eq!(daily, 8000); // 5000 + 3000 (yesterday's 10000 excluded)

    let _ = std::fs::remove_file(&path);
}
```

### Test 11: DB monthly_spend_for_payer query (db.rs)

```rust
#[test]
fn test_monthly_spend_for_payer() {
    let (path, reader) = setup_test_db();
    let conn = Connection::open(&path).unwrap();
    let _ = conn.execute("ALTER TABLE request_log ADD COLUMN agent_name TEXT DEFAULT ''", []);

    let now = chrono::Utc::now().timestamp();
    let payer = "0x9E2b000000000000000000000000000000000001";

    // Insert this month's spend
    conn.execute(
        "INSERT INTO request_log (payment_id, endpoint, payer_address, amount_charged, agent_name, created_at)
         VALUES ('p1', 'POST /v1/chat', ?, 5000, '', ?)",
        params![payer, now - 100],
    ).unwrap();
    conn.execute(
        "INSERT INTO request_log (payment_id, endpoint, payer_address, amount_charged, agent_name, created_at)
         VALUES ('p2', 'POST /v1/chat', ?, 7000, '', ?)",
        params![payer, now - 50],
    ).unwrap();
    drop(conn);

    let monthly = reader.monthly_spend_for_payer(payer).unwrap();
    assert_eq!(monthly, 12000);

    let _ = std::fs::remove_file(&path);
}
```

### Test 12: DB daily_spend_for_agent query (db.rs)

```rust
#[test]
fn test_daily_spend_for_agent() {
    let (path, reader) = setup_test_db();
    let conn = Connection::open(&path).unwrap();
    let _ = conn.execute("ALTER TABLE request_log ADD COLUMN agent_name TEXT DEFAULT ''", []);

    let now = chrono::Utc::now().timestamp();
    let payer = "0x9E2b000000000000000000000000000000000001";

    // Agent A: $2
    conn.execute(
        "INSERT INTO request_log (payment_id, endpoint, payer_address, amount_charged, agent_name, created_at)
         VALUES ('p1', 'POST /v1/chat', ?, 2000, 'agent-a', ?)",
        params![payer, now - 100],
    ).unwrap();

    // Agent B: $3
    conn.execute(
        "INSERT INTO request_log (payment_id, endpoint, payer_address, amount_charged, agent_name, created_at)
         VALUES ('p2', 'POST /v1/chat', ?, 3000, 'agent-b', ?)",
        params![payer, now - 50],
    ).unwrap();
    drop(conn);

    let agent_a = reader.daily_spend_for_agent(payer, "agent-a").unwrap();
    assert_eq!(agent_a, 2000);

    let agent_b = reader.daily_spend_for_agent(payer, "agent-b").unwrap();
    assert_eq!(agent_b, 3000);

    let _ = std::fs::remove_file(&path);
}
```

### Test 13: ALTER TABLE migration idempotent (db.rs)

```rust
#[tokio::test]
async fn test_init_db_migration_idempotent() {
    let db_path = format!("/tmp/paygate_migration_test_{}.db", uuid::Uuid::new_v4());

    // First init — creates tables and runs ALTER TABLE
    let (reader1, writer1) = init_db(&db_path).unwrap();
    drop(reader1);
    drop(writer1);

    // Second init — ALTER TABLE should not fail (catches "duplicate column")
    let result = init_db(&db_path);
    assert!(result.is_ok(), "Second init_db should succeed (idempotent migration)");

    let _ = std::fs::remove_file(&db_path);
}
```

### Test 14: GET /paygate/spend with valid HMAC (sessions.rs)

```rust
#[tokio::test]
async fn test_get_spend_endpoint_authenticated() {
    let (state, db_path) = test_state_with_governance().await;

    let session_secret = "ssec_deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    let session_id = "sess_spend_test";
    let payer = "0x9E2b000000000000000000000000000000000001";
    let future = chrono::Utc::now().timestamp() + 86400;
    insert_session(&db_path, session_id, session_secret, payer, 50_000_000, future);

    // Record some spend
    state.spend_accumulator.record_spend(payer, "", 2_500_000);

    let app = axum::Router::new()
        .route("/paygate/spend", axum::routing::get(handle_get_spend))
        .with_state(state.clone());

    // Compute HMAC for "GET /paygate/spend"
    let ts = chrono::Utc::now().timestamp().to_string();
    let raw_secret = session_secret.strip_prefix("ssec_").unwrap();
    let key_bytes = hex::decode(raw_secret).unwrap();
    let request_hash = paygate_common::hash::request_hash("GET", "/paygate/spend", &[]);
    let rh_hex = format!("0x{}", hex::encode(request_hash.as_slice()));

    let mut mac = HmacSha256::new_from_slice(&key_bytes).unwrap();
    mac.update(rh_hex.as_bytes());
    mac.update(ts.as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());

    let req = axum::http::Request::builder()
        .method("GET")
        .uri("/paygate/spend")
        .header("X-Payment-Session", session_id)
        .header("X-Payment-Session-Sig", &sig)
        .header("X-Payment-Timestamp", &ts)
        .body(Body::empty())
        .unwrap();

    let resp = tower::ServiceExt::oneshot(app, req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap(),
    ).unwrap();

    assert_eq!(body["daily_spent"], "2.500000");
    assert_eq!(body["daily_limit"], "5.000000");
    assert!(body["governance_enabled"].as_bool().unwrap());
}
```

### Test 15: GET /paygate/spend without auth returns 401 (sessions.rs)

```rust
#[tokio::test]
async fn test_get_spend_endpoint_unauthenticated() {
    let (state, _db_path) = test_state_with_governance().await;

    let app = axum::Router::new()
        .route("/paygate/spend", axum::routing::get(handle_get_spend))
        .with_state(state);

    let req = axum::http::Request::builder()
        .method("GET")
        .uri("/paygate/spend")
        .body(Body::empty())
        .unwrap();

    let resp = tower::ServiceExt::oneshot(app, req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap(),
    ).unwrap();
    assert_eq!(body["error"], "auth_required");
}
```

---

## 15. Caller site updates

Every existing call to `state.db_writer.log_request(...)` must be updated to include `agent_name`. There are multiple call sites in `serve.rs`:

1. Session auth branch (line ~376) -- use extracted `agent_name`
2. Per-request payment branch (line ~487) -- use `String::new()` (no agent for per-request)

Every existing call to `verify_and_deduct(...)` must be updated to include `agent_name`:

1. Session auth branch in `gateway_handler` (line ~337) -- use extracted `agent_name`

Every existing construction of `FullSessionRecord` must include `agent_name`:

1. `handle_create_session` (line ~318) -- use extracted agent from header

Every existing test that constructs `test_config()` must include `governance` field:

```rust
fn test_config() -> Config {
    Config {
        // ... existing fields ...
        governance: Default::default(),  // disabled by default
    }
}
```

Every test that constructs `AppState` must include `spend_accumulator`:

```rust
let state = AppState {
    // ... existing fields ...
    spend_accumulator: Arc::new(SpendAccumulator::new()),
};
```

Every test `insert_session` helper should be updated or left as-is (the DEFAULT '' on the column handles missing values).

---

## 16. Error rescue registry

Add to `docs/designs/error-rescue-registry.md`:

```
| `spend_limit_exceeded` | 402 Payment Required | Daily or monthly spend limit reached | Wait for limit reset (UTC midnight/month start), or increase limit in gateway config |
```

---

## Summary of all new/modified public signatures

```rust
// config.rs
pub struct GovernanceConfig { pub enabled: bool, pub default_daily_limit: String, pub default_monthly_limit: String }
impl GovernanceConfig { pub fn daily_limit_base_units(&self) -> u64; pub fn monthly_limit_base_units(&self) -> u64 }

// mpp.rs
pub const HEADER_PAYMENT_AGENT: &str = "X-Payment-Agent";

// db.rs
pub struct FullSessionRecord { /* + agent_name: String */ }
impl DbReader {
    pub fn daily_spend_for_payer(&self, payer: &str) -> Result<u64, DbError>;
    pub fn monthly_spend_for_payer(&self, payer: &str) -> Result<u64, DbError>;
    pub fn daily_spend_for_agent(&self, payer: &str, agent: &str) -> Result<u64, DbError>;
}
impl DbWriter {
    pub async fn log_request(/* existing args + agent_name: String */) -> Result<(), DbError>;
}

// sessions.rs
pub struct SpendKey { pub payer: String, pub agent: String }
pub struct Accumulator { /* internal */ }
pub struct SpendAccumulator { /* Mutex<HashMap<SpendKey, Accumulator>> */ }
impl SpendAccumulator {
    pub fn new() -> Self;
    pub fn load_from_db(db_reader: &DbReader) -> Self;
    pub fn check_limits(&self, payer: &str, agent: &str, amount: u64, daily_limit: u64, monthly_limit: u64) -> Result<(), SpendLimitInfo>;
    pub fn record_spend(&self, payer: &str, agent: &str, amount: u64);
    pub fn get_payer_totals(&self, payer: &str) -> (u64, u64);
    pub fn get_agent_totals(&self, payer: &str, agent: &str) -> (u64, u64);
    pub fn seed_from_db(&self, payer: &str, daily: u64, monthly: u64);
}
pub struct SpendLimitInfo { pub period: String, pub limit: u64, pub spent: u64 }
pub enum SessionError { /* + SpendLimitExceeded { period, limit, spent } */ }
pub async fn verify_and_deduct(state, headers, request_hash, endpoint, agent_name) -> Result<SessionDeduction, SessionError>;
pub async fn handle_get_spend(State(state), Query(params), req) -> Response;

// server.rs
pub struct AppState { /* + spend_accumulator: Arc<SpendAccumulator> */ }
```
