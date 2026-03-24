# Prompt: Pane Governance — Spend Governance + Agent Identity

## Role

You are building Wave 3 Stream 2: Spend Governance + Agent Identity for the PayGate gateway. This adds per-payer spend limits (daily/monthly), agent identity tracking via `X-Payment-Agent` header, and a `GET /paygate/spend` endpoint with HMAC authentication.

## File Read List

Read these files FIRST before writing any code:

```
# Build brief (your spec — every struct, function, SQL query is defined here)
worktree-briefs/pane-governance-brief.md

# Files you are modifying
crates/paygate-gateway/src/config.rs
crates/paygate-gateway/src/db.rs
crates/paygate-gateway/src/sessions.rs
crates/paygate-gateway/src/serve.rs
crates/paygate-gateway/src/server.rs
crates/paygate-common/src/mpp.rs
schema.sql

# Context files (read-only, do not modify)
crates/paygate-gateway/src/proxy.rs
crates/paygate-gateway/src/admin.rs
crates/paygate-gateway/src/rate_limit.rs
crates/paygate-gateway/src/metrics.rs
crates/paygate-common/src/types.rs
crates/paygate-common/src/hash.rs
```

## Build Order (4 Phases)

### Phase 1: Config + Schema + DB Queries (parallel-safe)

These three changes are independent and can be written in any order.

**1a. GovernanceConfig (config.rs)**
- Add `GovernanceConfig` struct with `enabled`, `default_daily_limit`, `default_monthly_limit`
- Add `governance: GovernanceConfig` field to `Config` struct (with `#[serde(default)]`)
- Add `daily_limit_base_units()` and `monthly_limit_base_units()` helper methods
- Add validation in `Config::validate()` for governance limits
- Add default value functions: `default_daily_limit`, `default_monthly_limit`
- Add Test 1 (governance config parsing) and Test 2 (governance config defaults)

**1b. Schema migration + DB queries (db.rs)**
- Add ALTER TABLE migrations in `init_db()` with "duplicate column name" error catching
- Add `utc_day_start()` and `utc_month_start()` helper functions
- Add `daily_spend_for_payer()`, `monthly_spend_for_payer()`, `daily_spend_for_agent()` to `DbReader`
- Add `agent_name: String` field to `WriteCommand::InsertRequestLog`
- Add `agent_name: String` field to `FullSessionRecord`
- Update `log_request()` signature on `DbWriter` to include `agent_name`
- Update `flush_batch` for `InsertRequestLog` and `CreateSession` to include `agent_name`
- Update `get_session()` and `list_sessions_for_payer()` SELECT queries to include `COALESCE(agent_name, '')`
- Add indexes: `idx_request_log_payer_created`, `idx_request_log_agent`
- Add Test 10, 11, 12 (DB query tests) and Test 13 (idempotent migration)

**1c. Header constant (mpp.rs)**
- Add `pub const HEADER_PAYMENT_AGENT: &str = "X-Payment-Agent";`
- Update `is_payment_header()` to include `HEADER_PAYMENT_AGENT`

**Codex Review Gate 1:** `cargo build` must succeed. The build will have errors because `serve.rs` calls `log_request()` with the old signature and `AppState` is missing `spend_accumulator`. That is expected — those are fixed in Phase 2. To pass the build at this point, you may temporarily keep the old `log_request` signature alongside the new one, OR proceed directly to Phase 2 after writing Phase 1 code. The important thing: config parses, DB migrations run, queries compile.

### Phase 2: SpendAccumulator + verify_and_deduct Integration

**2a. SpendAccumulator (sessions.rs)**
- Add `SpendKey`, `Accumulator`, `SpendAccumulator`, `SpendLimitInfo` structs
- Implement `new()`, `check_limits()`, `record_spend()`, `get_payer_totals()`, `get_agent_totals()`, `seed_from_db()`
- Implement `Accumulator::maybe_reset()` with UTC day/month boundary detection
- Add `SpendLimitExceeded { period, limit, spent }` variant to `SessionError`

**2b. Modify verify_and_deduct (sessions.rs)**
- Add `agent_name: &str` parameter
- Insert spend limit check AFTER HMAC verification (step 4), BEFORE balance deduction (step 6)
- Insert `record_spend()` call AFTER successful deduction
- Seed accumulator from DB on first access for each payer

**2c. AppState (server.rs)**
- Add `spend_accumulator: Arc<SpendAccumulator>` field

**2d. serve.rs integration**
- Construct `SpendAccumulator::new()` in `cmd_serve()`, wrap in `Arc`
- Extract `X-Payment-Agent` header in session auth branch of `gateway_handler`
- Pass `agent_name` to `verify_and_deduct()` calls
- Pass `agent_name` to `log_request()` calls (session branch uses extracted agent, per-request branch uses `String::new()`)
- Add `SpendLimitExceeded` match arm returning 402
- Extract agent in `handle_create_session`

**2e. Update all test helpers**
- Add `governance: Default::default()` to every `test_config()`
- Add `spend_accumulator: Arc::new(SpendAccumulator::new())` to every test `AppState`
- Add `agent_name: String::new()` (or `"".to_string()`) to every `FullSessionRecord` construction in tests
- Update `insert_session` helper if needed (or rely on DEFAULT '')

- Add Test 3-7 (SpendAccumulator unit tests), Test 8-9 (integration tests)

**Codex Review Gate 2:** `cargo build` must succeed. `cargo test` must pass. All existing tests must still pass.

### Phase 3: GET /paygate/spend Endpoint

**3a. Handler (sessions.rs)**
- Implement `handle_get_spend()` with HMAC authentication
- Verify session ownership (payer matches)
- Return spend data from SpendAccumulator
- No balance deduction
- Format amounts with `{:.6}` and handle `u64::MAX` as "unlimited"

**3b. Route (serve.rs)**
- Add `.route("/paygate/spend", axum::routing::get(sessions::handle_get_spend))`

- Add Test 14 (authenticated spend query) and Test 15 (unauthenticated returns 401)

**Codex Review Gate 3:** `cargo test` passes. The `/paygate/spend` endpoint returns correct JSON with HMAC auth.

### Phase 4: Final Tests + Cleanup

**4a. Run full test suite**
```bash
cargo test
```

**4b. Verify all 15 tests pass**
- Test 1: GovernanceConfig parsing
- Test 2: GovernanceConfig defaults
- Test 3: SpendAccumulator within limits
- Test 4: SpendAccumulator daily limit exceeded
- Test 5: SpendAccumulator monthly limit exceeded
- Test 6: SpendAccumulator agent tracking
- Test 7: SpendAccumulator seed_from_db
- Test 8: SpendLimitExceeded returns 402 (integration)
- Test 9: Governance disabled allows unlimited
- Test 10: DB daily_spend_for_payer
- Test 11: DB monthly_spend_for_payer
- Test 12: DB daily_spend_for_agent
- Test 13: ALTER TABLE migration idempotent
- Test 14: GET /paygate/spend authenticated
- Test 15: GET /paygate/spend unauthenticated returns 401

**4c. Verify existing tests still pass**
All pre-existing tests in sessions.rs, db.rs, serve.rs, config.rs must continue to pass. The main risk is the `log_request()` signature change and the new `agent_name` field on `FullSessionRecord`.

**Codex Review Gate 4 (Final):**
```bash
cargo test 2>&1 | tail -5
# Expected: "test result: ok. XX passed; 0 failed"
cargo build 2>&1 | tail -3
# Expected: no errors, no warnings about unused imports
```

## Key Constraints

1. **402 not 429** — Spend limit exceeded returns HTTP 402 (Payment Required), not 429 (Too Many Requests). This is a payment-domain error.

2. **UTC boundaries** — Daily resets at 00:00 UTC. Monthly resets on the 1st at 00:00 UTC. Use `chrono::Utc::now()` and check-on-access pattern (no background timer).

3. **Mutex not RwLock** — SpendAccumulator uses `Mutex` because every access potentially writes (maybe_reset). The lock is held only for the HashMap lookup + arithmetic, never across `.await` points.

4. **Seed-on-first-access** — Don't scan the full request_log table on startup. Instead, seed each payer's accumulator from DB on first access (lazy initialization).

5. **Record before DB write** — `record_spend()` is called synchronously after `deduct_session_balance` succeeds but before the async `log_request` DB write. This ensures the in-memory total is always >= the DB total, preventing the write-batch race.

6. **ArcSwap config reload** — When governance config changes via SIGHUP, new limits take effect immediately (read from `state.current_config()` on every `check_limits` call). Accumulated totals are NOT reset — changing the limit doesn't erase past spending.

7. **X-Payment-Agent stripping** — The agent header must be stripped before proxying to upstream, per the existing X-Payment-* stripping rule in the header sanitizer (proxy.rs). Adding the constant to `is_payment_header()` in mpp.rs handles this automatically.

8. **Backward compatibility** — The `agent_name` column has `DEFAULT ''`. Existing rows and sessions created without the header get empty string. All queries use `COALESCE(agent_name, '')` for safety.

9. **No per-agent config overrides** — Per the CEO plan, `[governance.agents]` is deferred. Only per-payer limits ship in v0.5.0.

10. **HMAC for /paygate/spend** — Uses the same HMAC pattern as session auth: `request_hash("GET", "/paygate/spend", &[])` + timestamp. No balance deduction for this endpoint.

## Common Pitfalls

- Forgetting to update ALL call sites of `log_request()` (there are multiple in serve.rs)
- Forgetting to update ALL constructions of `FullSessionRecord` (in sessions.rs tests and handle_create_session)
- Forgetting to add `spend_accumulator` to ALL `AppState` constructions in tests
- Holding the Mutex across an `.await` point (compile error — Mutex guard is !Send)
- Using `utc_day_start()` in test assertions without accounting for timezone edge cases (tests may run near midnight UTC)
- The `handle_create_session` function consumes `req` body — extract agent header BEFORE `req.into_body()`
