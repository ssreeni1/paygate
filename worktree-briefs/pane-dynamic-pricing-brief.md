# Build Brief: Dynamic Pricing + Session Balance Widget + Demo Headers (Pane 3)

Runs AFTER sessions merge to main. Depends on session infrastructure from Pane 1.

## Part A: Dynamic Pricing in the Gateway

### Goal

After proxying a request with session auth, read the upstream's `X-Token-Count` response header and compute the actual cost using a per-token formula. Adjust the session balance to match.

### Modify: `crates/paygate-gateway/src/serve.rs` — `gateway_handler`

In the session auth branch, AFTER the proxy response is received and BEFORE logging:

```rust
// Dynamic pricing adjustment (session auth only)
let actual_cost = if config.pricing.dynamic.enabled {
    if let Some(token_count_header) = resp.headers().get(&config.pricing.dynamic.header_source) {
        if let Ok(token_count_str) = token_count_header.to_str() {
            if let Ok(token_count) = token_count_str.parse::<u64>() {
                let base = config.pricing.dynamic.base_cost_per_token_units;
                let spread = config.pricing.dynamic.spread_per_token_units;
                let dynamic_cost = token_count * (base + spread);

                if dynamic_cost > deduction.amount_deducted {
                    // Under-charged: deduct the difference
                    let diff = dynamic_cost - deduction.amount_deducted;
                    let _ = sessions::deduct_additional(&state, &deduction.session_id, diff).await;
                } else if dynamic_cost < deduction.amount_deducted {
                    // Over-charged: refund the difference
                    let diff = deduction.amount_deducted - dynamic_cost;
                    let _ = state.db_writer.refund_session_balance(&deduction.session_id, diff).await;
                }

                // Set actual cost header
                let cost_decimal = format!("{:.6}", dynamic_cost as f64 / 1_000_000.0);
                if let Ok(v) = HeaderValue::from_str(&cost_decimal) {
                    resp.headers_mut().insert("X-Payment-Cost", v);
                }

                Some(dynamic_cost)
            } else { None }
        } else { None }
    } else { None }
} else { None };

let final_cost = actual_cost.unwrap_or(deduction.amount_deducted);
```

Use `final_cost` instead of `deduction.amount_deducted` in the request log.

### Modify: `crates/paygate-gateway/src/config.rs`

Update `DynamicPricingConfig` to parse token prices into base units at load time:

```rust
#[derive(Debug, Clone, Deserialize, Default)]
pub struct DynamicPricingConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub formula: String,                    // "token" | "compute" | "fixed"
    #[serde(default)]
    pub base_cost_per_token: String,        // e.g., "0.00001"
    #[serde(default)]
    pub spread_per_token: String,           // e.g., "0.000005"
    #[serde(default = "default_header_source")]
    pub header_source: String,              // e.g., "X-Token-Count"
}

fn default_header_source() -> String { "X-Token-Count".to_string() }
```

Add computed fields or a method to get base units:

```rust
impl DynamicPricingConfig {
    /// Parse base_cost_per_token to sub-microUSDC units for per-token math.
    /// Since tokens can cost fractions of a microUSDC, we work in nanoUSDC (1e-9)
    /// and convert back to base units (1e-6) after multiplying by token count.
    ///
    /// Actually simpler: parse as f64, multiply by token_count, round to base units.
    pub fn compute_cost(&self, token_count: u64) -> u64 {
        let base: f64 = self.base_cost_per_token.parse().unwrap_or(0.0);
        let spread: f64 = self.spread_per_token.parse().unwrap_or(0.0);
        let cost_usd = token_count as f64 * (base + spread);
        (cost_usd * 1_000_000.0).round() as u64
    }
}
```

Use `config.pricing.dynamic.compute_cost(token_count)` in serve.rs instead of inline math.

### Modify: `crates/paygate-gateway/src/sessions.rs`

Add a helper for additional deduction (used when dynamic cost > pre-deducted):

```rust
pub async fn deduct_additional(state: &AppState, session_id: &str, amount: u64) -> Result<bool, DbError> {
    state.db_writer.deduct_session_balance(session_id, amount).await
}
```

### Update 402 Response for Dynamic Endpoints

In `crates/paygate-gateway/src/mpp.rs`, when generating the 402 response, if the endpoint has dynamic pricing enabled, include a note:

```json
{
  "pricing": {
    "amount": "0.003000",
    "note": "Estimated price. Actual cost varies by response token count. Requires an active session.",
    "dynamic": true
  }
}
```

Add a `dynamic: bool` field to the 402 JSON when `config.pricing.dynamic.enabled` is true.

### Update `demo/paygate.toml`

Add dynamic pricing config:
```toml
[pricing.dynamic]
enabled = true
formula = "token"
base_cost_per_token = "0.00001"
spread_per_token = "0.000005"
header_source = "X-Token-Count"
```

### Tests

1. **Dynamic pricing adjusts cost downward**: Mock upstream returns `X-Token-Count: 100`, base+spread = 0.000015/token, so cost = 0.0015 USDC = 1500 base units. If static rate was 3000, refund 1500.
2. **Dynamic pricing adjusts cost upward**: Mock upstream returns `X-Token-Count: 5000`, cost = 75000. If static rate was 3000, deduct additional 72000.
3. **No X-Token-Count header**: Falls back to static deduction, no adjustment.
4. **Dynamic pricing disabled**: Even if upstream sends X-Token-Count, no adjustment occurs.
5. **compute_cost() unit test**: Verify formula produces correct base units for known inputs.

## Part B: Demo Server `X-Token-Count` Header

### Modify: `demo/src/routes/summarize.ts`

After generating the summary response, add the `X-Token-Count` header:

- **Mock mode** (no ANTHROPIC_API_KEY): Estimate tokens from response length. Rough formula: `Math.ceil(responseText.length / 4)` (1 token ~ 4 chars).
- **Live mode** (Anthropic API): The API response includes `usage.output_tokens`. Use that value directly.

```typescript
// After getting the response
const tokenCount = isLiveMode
  ? apiResponse.usage.output_tokens
  : Math.ceil(summaryText.length / 4);

return new Response(JSON.stringify({ summary: summaryText }), {
  headers: {
    'Content-Type': 'application/json',
    'X-Token-Count': tokenCount.toString(),
  },
});
```

### Modify: `demo/src/routes/search.ts`

Add `X-Token-Count` header based on result count:

```typescript
// Each search result ~ 50 tokens (title + snippet + URL)
const tokenCount = results.length * 50;

return new Response(JSON.stringify({ results }), {
  headers: {
    'Content-Type': 'application/json',
    'X-Token-Count': tokenCount.toString(),
  },
});
```

### Tests

1. Mock summarize response includes X-Token-Count header
2. Mock search response includes X-Token-Count header proportional to result count

## Part C: Session Balance Widget on Marketplace

### New Endpoint: `GET /paygate/sessions?payer=0x...`

Add to `crates/paygate-gateway/src/sessions.rs`:

```rust
pub async fn handle_get_sessions(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let payer = match params.get("payer") {
        Some(p) => p,
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error": "payer required"}))).into_response(),
    };

    let sessions = state.db_reader.list_sessions_for_payer(payer).unwrap_or_default();
    let active: Vec<_> = sessions.into_iter().filter(|s| s.status == "active").collect();

    Json(json!({
        "sessions": active.iter().map(|s| json!({
            "sessionId": s.id,
            "balance": format!("{:.6}", s.balance as f64 / 1_000_000.0),
            "ratePerRequest": format!("{:.6}", s.rate_per_request as f64 / 1_000_000.0),
            "requestsMade": s.requests_made,
            "expiresAt": chrono::DateTime::from_timestamp(s.expires_at, 0)
                .map(|d| d.to_rfc3339()).unwrap_or_default(),
            "status": s.status,
        })).collect::<Vec<_>>(),
    })).into_response()
}
```

Wire in serve.rs:
```rust
.route("/paygate/sessions", axum::routing::get(sessions::handle_get_sessions))
```

Add to DbReader:
```rust
pub fn list_sessions_for_payer(&self, payer: &str) -> Result<Vec<FullSessionRecord>, DbError>
```

### Modify: `docs/marketplace.html`

Add a session balance widget. It should:

1. Be hidden by default (no session = no widget)
2. Poll `GET /paygate/sessions?payer=<connected_wallet>` every 5 seconds when a wallet is connected
3. Display:
   ```
   Session: $0.32 remaining (64 calls at $0.005/call)
   ```
4. Update on each poll
5. Show a "low balance" warning when < 10 calls remaining
6. Disappear when no active session exists

HTML/CSS:
```html
<div id="session-widget" style="display: none; position: fixed; bottom: 20px; right: 20px;
     background: #1a1a2e; border: 1px solid #333; border-radius: 8px; padding: 12px 16px;
     color: #e0e0e0; font-family: monospace; font-size: 13px; z-index: 1000;">
  <div style="font-weight: bold; margin-bottom: 4px;">Active Session</div>
  <div id="session-balance">$0.00 remaining</div>
  <div id="session-calls" style="color: #888; font-size: 11px;">0 calls at $0.000/call</div>
</div>
```

JavaScript:
```javascript
async function pollSessionBalance() {
  if (!window.payerAddress) return;

  try {
    const resp = await fetch(`${GATEWAY_URL}/paygate/sessions?payer=${window.payerAddress}`);
    const data = await resp.json();

    if (data.sessions && data.sessions.length > 0) {
      const session = data.sessions[0]; // most recent active
      const balance = parseFloat(session.balance);
      const rate = parseFloat(session.ratePerRequest);
      const callsRemaining = Math.floor(balance / rate);

      document.getElementById('session-widget').style.display = 'block';
      document.getElementById('session-balance').textContent = `$${balance.toFixed(2)} remaining`;
      document.getElementById('session-calls').textContent =
        `${callsRemaining} calls at $${rate.toFixed(4)}/call`;

      // Low balance warning
      if (callsRemaining < 10) {
        document.getElementById('session-balance').style.color = '#ff6b6b';
      } else {
        document.getElementById('session-balance').style.color = '#4ade80';
      }
    } else {
      document.getElementById('session-widget').style.display = 'none';
    }
  } catch (e) {
    // Silently fail — widget just won't show
  }
}

setInterval(pollSessionBalance, 5000);
```

## Source Files to Read Before Building

- `crates/paygate-gateway/src/serve.rs` — gateway_handler (you are modifying the session branch)
- `crates/paygate-gateway/src/sessions.rs` — session module (built by Pane 1, you are adding to it)
- `crates/paygate-gateway/src/config.rs` — DynamicPricingConfig (you are updating)
- `crates/paygate-gateway/src/mpp.rs` — 402 response generation
- `crates/paygate-gateway/src/db.rs` — DbReader/DbWriter
- `demo/src/routes/summarize.ts` — adding X-Token-Count header
- `demo/src/routes/search.ts` — adding X-Token-Count header
- `demo/paygate.toml` — adding dynamic pricing config
- `docs/marketplace.html` — adding session widget
- `SPEC.md` section 4.3 — session protocol
