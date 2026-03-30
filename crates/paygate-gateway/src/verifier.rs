use crate::config::Config;
use crate::metrics;
use crate::server::AppState;
use alloy_primitives::{Address, B256, U256, keccak256};
use paygate_common::hash;
use paygate_common::types::*;
use serde_json::Value;
use std::time::Instant;
use tracing::info;

/// Transfer event signature: keccak256("Transfer(address,address,uint256)")
fn transfer_event_sig() -> B256 {
    keccak256("Transfer(address,address,uint256)")
}

/// TransferWithMemo event signature.
fn transfer_with_memo_sig() -> B256 {
    keccak256("TransferWithMemo(address,address,uint256,bytes32)")
}

/// Decoded transfer event data.
#[derive(Debug)]
pub(crate) struct DecodedTransfer {
    pub from: Address,
    pub to: Address,
    pub amount: BaseUnits,
}

/// Constant-time byte comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Make a JSON-RPC call with failover across configured RPC URLs.
pub(crate) async fn rpc_call(
    http_client: &reqwest::Client,
    rpc_urls: &[String],
    timeout_ms: u64,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1
    });
    let timeout = std::time::Duration::from_millis(timeout_ms);

    for rpc_url in rpc_urls {
        match http_client.post(rpc_url).json(&body).timeout(timeout).send().await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    metrics::record_rpc_error();
                    continue;
                }
                let json: Value = match resp.json().await {
                    Ok(j) => j,
                    Err(_) => {
                        metrics::record_rpc_error();
                        continue;
                    }
                };
                if json.get("error").is_some() {
                    metrics::record_rpc_error();
                    return Err(format!("RPC error: {}", json["error"]));
                }
                return Ok(json["result"].clone());
            }
            Err(_) => {
                metrics::record_rpc_error();
                continue;
            }
        }
    }

    Err("all RPC endpoints failed".to_string())
}

/// Decode Transfer events from receipt logs, matching provider and token.
pub(crate) fn decode_transfer_events(
    logs: &[Value],
    provider_address: &Address,
    accepted_token: &Address,
) -> Result<DecodedTransfer, VerificationResult> {
    let sig = transfer_event_sig();
    let mut matches = Vec::new();

    for log in logs {
        let log_address: Address = match log
            .get("address")
            .and_then(|a| a.as_str())
            .and_then(|s| s.parse().ok())
        {
            Some(a) => a,
            None => continue,
        };
        if &log_address != accepted_token {
            continue;
        }

        let topics = match log.get("topics").and_then(|t| t.as_array()) {
            Some(t) if t.len() >= 3 => t,
            _ => continue,
        };

        let topic0 = match parse_b256(topics.get(0)) {
            Some(t) => t,
            None => continue,
        };
        if topic0 != sig {
            continue;
        }

        let from = match address_from_topic(topics.get(1)) {
            Some(a) => a,
            None => continue,
        };
        let to = match address_from_topic(topics.get(2)) {
            Some(a) => a,
            None => continue,
        };
        if &to != provider_address {
            continue;
        }

        let data = match log.get("data").and_then(|d| d.as_str()).and_then(parse_hex) {
            Some(d) if d.len() >= 32 => d,
            _ => continue,
        };
        let amount_u256 = U256::from_be_slice(&data[..32]);
        let amount = u64::try_from(amount_u256).unwrap_or(u64::MAX);

        matches.push(DecodedTransfer { from, to, amount });
    }

    match matches.len() {
        0 => Err(VerificationResult::InvalidTransfer(
            "no matching Transfer event".into(),
        )),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => Err(VerificationResult::AmbiguousTransfer),
    }
}

/// Decode TransferWithMemo log to extract memo bytes32.
///
/// TransferWithMemo event signature:
///   event TransferWithMemo(address indexed from, address indexed to, uint256 value, bytes32 indexed memo)
///
/// Topics layout:
///   topics[0] = event signature hash
///   topics[1] = from (indexed)
///   topics[2] = to (indexed)
///   topics[3] = memo (indexed) ← the memo is here, NOT in data
///
/// Data layout:
///   data = abi.encode(uint256 value)
pub(crate) fn decode_memo_from_logs(logs: &[Value]) -> Result<B256, VerificationResult> {
    let sig = transfer_with_memo_sig();

    for log in logs {
        let topics = match log.get("topics").and_then(|t| t.as_array()) {
            Some(t) => t,
            None => continue,
        };
        let topic0 = match parse_b256(topics.get(0)) {
            Some(t) => t,
            None => continue,
        };
        if topic0 != sig {
            continue;
        }

        // memo is indexed → topics[3]
        if topics.len() < 4 {
            return Err(VerificationResult::InvalidTransfer(
                "TransferWithMemo log missing memo topic (expected 4 topics)".into(),
            ));
        }
        let memo = match parse_b256(topics.get(3)) {
            Some(m) => m,
            None => {
                return Err(VerificationResult::InvalidTransfer(
                    "failed to parse memo from topics[3]".into(),
                ));
            }
        };
        return Ok(memo);
    }

    Err(VerificationResult::InvalidTransfer(
        "no TransferWithMemo event found".into(),
    ))
}

fn parse_b256(val: Option<&Value>) -> Option<B256> {
    let s = val?.as_str()?;
    let hex_str = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(hex_str).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    Some(B256::from_slice(&bytes))
}

fn address_from_topic(val: Option<&Value>) -> Option<Address> {
    let b = parse_b256(val)?;
    Some(Address::from_slice(&b.as_slice()[12..]))
}

fn parse_hex(s: &str) -> Option<Vec<u8>> {
    let hex_str = s.strip_prefix("0x").unwrap_or(s);
    hex::decode(hex_str).ok()
}

fn parse_hex_u64(s: &str) -> Option<u64> {
    let hex_str = s.strip_prefix("0x").unwrap_or(s);
    u64::from_str_radix(hex_str, 16).ok()
}

async fn fetch_block_timestamp(
    state: &AppState,
    block_number_hex: &str,
    config: &Config,
) -> Result<u64, String> {
    let result = rpc_call(
        &state.http_client,
        &config.tempo.rpc_urls,
        config.tempo.rpc_timeout_ms,
        "eth_getBlockByNumber",
        serde_json::json!([block_number_hex, false]),
    )
    .await?;

    result
        .get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(parse_hex_u64)
        .ok_or_else(|| "missing block timestamp".to_string())
}

/// Payment verification pipeline.
pub async fn verify_payment(
    state: &AppState,
    tx_hash: &str,
    payer_address: &str,
    quote_id: Option<&str>,
    endpoint: &str,
    request_hash: &B256,
) -> VerificationResult {
    let start = Instant::now();
    let config = state.current_config();

    // 1. Fetch tx receipt
    let receipt = match rpc_call(
        &state.http_client,
        &config.tempo.rpc_urls,
        config.tempo.rpc_timeout_ms,
        "eth_getTransactionReceipt",
        serde_json::json!([tx_hash]),
    )
    .await
    {
        Ok(v) if v.is_null() => return VerificationResult::TxNotFound,
        Ok(v) => v,
        Err(e) => return VerificationResult::RpcError(e),
    };

    let logs = receipt
        .get("logs")
        .and_then(|l| l.as_array())
        .cloned()
        .unwrap_or_default();

    let block_number_hex = receipt
        .get("blockNumber")
        .and_then(|b| b.as_str())
        .unwrap_or("0x0");
    let block_number = parse_hex_u64(block_number_hex).unwrap_or(0);

    // 2. Decode TIP-20 Transfer event logs
    let provider_address: Address = match config.provider.address.parse() {
        Ok(a) => a,
        Err(_) => {
            return VerificationResult::InvalidTransfer("invalid provider address config".into())
        }
    };
    let accepted_token: Address = match config.tempo.accepted_token.parse() {
        Ok(a) => a,
        Err(_) => {
            return VerificationResult::InvalidTransfer("invalid accepted_token config".into())
        }
    };

    let transfer = match decode_transfer_events(&logs, &provider_address, &accepted_token) {
        Ok(t) => t,
        Err(r) => {
            metrics::record_payment_verified(endpoint, r.step_name());
            return r;
        }
    };

    // 3. Decode TransferWithMemo → extract memo
    let on_chain_memo = match decode_memo_from_logs(&logs) {
        Ok(m) => m,
        Err(r) => {
            metrics::record_payment_verified(endpoint, r.step_name());
            return r;
        }
    };

    // 4. Verify memo (constant-time comparison)
    let quote_id_str = quote_id.unwrap_or("");
    let expected_memo = hash::payment_memo(quote_id_str, request_hash);
    if !constant_time_eq(on_chain_memo.as_slice(), expected_memo.as_slice()) {
        return VerificationResult::MemoMismatch {
            expected: expected_memo,
            actual: on_chain_memo,
        };
    }

    // 5. Verify amount — honor quote price if valid, else use current price
    let expected_price = if let Some(qid) = quote_id {
        match state.db_reader.get_quote(qid) {
            Ok(Some(q)) => q.price,
            _ => config.price_for_endpoint(endpoint),
        }
    } else {
        config.price_for_endpoint(endpoint)
    };

    if transfer.amount < expected_price {
        return VerificationResult::InsufficientAmount {
            expected: expected_price,
            actual: transfer.amount,
        };
    }

    // 6. Verify payer binding (case-insensitive via parsed Address)
    let expected_payer: Address = match payer_address.parse() {
        Ok(a) => a,
        Err(_) => {
            return VerificationResult::InvalidTransfer("invalid payer address format".into())
        }
    };
    if transfer.from != expected_payer {
        return VerificationResult::PayerMismatch {
            expected: expected_payer,
            actual: transfer.from,
        };
    }

    // 7. Check replay protection
    match state.db_reader.is_tx_consumed(tx_hash) {
        Ok(true) => return VerificationResult::ReplayDetected,
        Ok(false) => {}
        Err(e) => {
            metrics::record_db_error();
            return VerificationResult::RpcError(format!("database error: {e}"));
        }
    }

    // 8. Check tx age
    let block_timestamp = fetch_block_timestamp(state, block_number_hex, &config)
        .await
        .unwrap_or_else(|_| chrono::Utc::now().timestamp() as u64);
    let now = chrono::Utc::now().timestamp() as u64;
    if now > block_timestamp && (now - block_timestamp) > config.security.tx_expiry_seconds {
        return VerificationResult::ExpiredTransaction;
    }

    // 9. Record payment
    let payment_id = uuid::Uuid::new_v4().to_string();
    let record = PaymentRecord {
        id: payment_id,
        tx_hash: tx_hash.to_string(),
        payer_address: payer_address.to_string(),
        amount: transfer.amount,
        token_address: config.tempo.accepted_token.clone(),
        endpoint: endpoint.to_string(),
        request_hash: Some(format!("{request_hash:#x}")),
        quote_id: quote_id.map(|s| s.to_string()),
        block_number,
        verified_at: chrono::Utc::now().timestamp(),
        status: "verified".to_string(),
    };

    if let Err(e) = state.db_writer.insert_payment(record).await {
        // UNIQUE constraint violation = concurrent replay (another request verified same tx)
        if let crate::db::DbError::Sqlite(rusqlite::Error::SqliteFailure(err, _)) = &e {
            if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE {
                return VerificationResult::ReplayDetected;
            }
        }
        metrics::record_db_error();
        return VerificationResult::RpcError(format!("database write error: {e}"));
    }

    // 10. Consume quote
    if let Some(qid) = quote_id {
        let _ = state.db_writer.consume_quote(qid.to_string()).await;
    }

    let duration = start.elapsed();
    metrics::record_verification_duration(duration.as_secs_f64());
    metrics::record_payment_verified(endpoint, "valid");
    metrics::record_revenue(&config.tempo.accepted_token, transfer.amount);

    info!(
        tx_hash = tx_hash,
        payer = payer_address,
        endpoint = endpoint,
        amount = transfer.amount,
        latency_ms = duration.as_millis() as u64,
        "payment verified"
    );

    let tx_b256 = parse_b256(Some(&Value::String(tx_hash.to_string()))).unwrap_or(B256::ZERO);

    VerificationResult::Valid(PaymentProof {
        tx_hash: tx_b256,
        payer: transfer.from,
        recipient: transfer.to,
        amount: transfer.amount,
        token: accepted_token,
        memo: on_chain_memo,
        block_number,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use crate::rate_limit::RateLimiter;
    use rusqlite::{params, Connection};
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    const TEST_PAYER: &str = "0x9E2b000000000000000000000000000000000001";
    const TEST_PROVIDER: &str = "0x7F3a000000000000000000000000000000000001";
    const TEST_TOKEN: &str = "0x1234000000000000000000000000000000000001";

    fn test_config_with_rpc(rpc_url: &str) -> Config {
        Config {
            gateway: GatewayConfig {
                listen: "127.0.0.1:0".into(),
                admin_listen: "127.0.0.1:0".into(),
                upstream: "http://localhost:9999".into(),
                upstream_timeout_seconds: 30,
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
                accepted_token: TEST_TOKEN.into(),
            },
            provider: ProviderConfig {
                address: TEST_PROVIDER.into(),
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
                    m.insert("GET /v1/models".into(), "0.000".into());
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
            governance: Default::default(),
            tips: None,
        }
    }

    async fn test_state(rpc_url: &str) -> (AppState, String) {
        let db_path = format!("/tmp/paygate_test_{}.db", uuid::Uuid::new_v4());
        let (db_reader, db_writer) = crate::db::init_db(&db_path).unwrap();
        let state = AppState {
            config: Arc::new(arc_swap::ArcSwap::new(Arc::new(test_config_with_rpc(
                rpc_url,
            )))),
            db_reader,
            db_writer,
            http_client: reqwest::Client::new(),
            rate_limiter: Arc::new(RateLimiter::new(100, 10)),
            webhook_sender: None,
            prometheus_handle: metrics_exporter_prometheus::PrometheusBuilder::new()
                .build_recorder()
                .handle(),
            started_at: std::time::Instant::now(),
            spend_accumulator: Arc::new(crate::sessions::SpendAccumulator::new()),
        };
        (state, db_path)
    }

    fn mock_receipt_logs(
        from: &str,
        to: &str,
        amount: u64,
        memo: &B256,
        token: &str,
    ) -> Vec<Value> {
        let tsig = transfer_event_sig();
        let msig = transfer_with_memo_sig();
        let from_hex = from.strip_prefix("0x").unwrap();
        let to_hex = to.strip_prefix("0x").unwrap();
        let from_topic = format!("0x{from_hex:0>64}");
        let to_topic = format!("0x{to_hex:0>64}");
        let amount_data = format!("0x{amount:064x}");
        let memo_topic = format!("0x{}", hex::encode(memo.as_slice()));

        vec![
            json!({
                "address": token,
                "topics": [format!("0x{}", hex::encode(tsig)), from_topic, to_topic],
                "data": amount_data
            }),
            json!({
                "address": token,
                // memo is indexed → topics[3], data only has amount
                "topics": [format!("0x{}", hex::encode(msig)), from_topic, to_topic, memo_topic],
                "data": amount_data
            }),
        ]
    }

    fn mock_receipt(block_number: u64, logs: Vec<Value>) -> Value {
        json!({
            "blockNumber": format!("0x{block_number:x}"),
            "logs": logs
        })
    }

    async fn start_mock_rpc(
        receipt: Option<Value>,
        block_timestamp: u64,
    ) -> String {
        let app = axum::Router::new().fallback({
            let receipt = receipt.clone();
            move |body: String| {
                let receipt = receipt.clone();
                async move {
                    let req: Value = serde_json::from_str(&body).unwrap_or_default();
                    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
                    let resp = match method {
                        "eth_getTransactionReceipt" => match &receipt {
                            Some(r) => json!({"jsonrpc":"2.0","result":r,"id":1}),
                            None => json!({"jsonrpc":"2.0","result":null,"id":1}),
                        },
                        "eth_getBlockByNumber" => json!({
                            "jsonrpc":"2.0",
                            "result": {"timestamp": format!("0x{block_timestamp:x}")},
                            "id":1
                        }),
                        _ => json!({"jsonrpc":"2.0","error":{"code":-1},"id":1}),
                    };
                    axum::Json(resp)
                }
            }
        });

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());
        format!("http://{addr}")
    }

    fn compute_test_memo(quote_id: &str, method: &str, path: &str, body: &[u8]) -> B256 {
        let rh = hash::request_hash(method, path, body);
        hash::payment_memo(quote_id, &rh)
    }

    // Test 1: Valid payment verification with mock RPC response
    #[tokio::test]
    async fn test_valid_payment_verification() {
        let quote_id = "qt_test123";
        let memo = compute_test_memo(quote_id, "POST", "/v1/chat/completions", b"{}");
        let logs = mock_receipt_logs(TEST_PAYER, TEST_PROVIDER, 5000, &memo, TEST_TOKEN);
        let receipt = mock_receipt(100, logs);
        let now = chrono::Utc::now().timestamp() as u64;
        let rpc_url = start_mock_rpc(Some(receipt), now).await;
        let (state, _db) = test_state(&rpc_url).await;

        let rh = hash::request_hash("POST", "/v1/chat/completions", b"{}");
        let result = verify_payment(
            &state,
            "0xabc123",
            TEST_PAYER,
            Some(quote_id),
            "POST /v1/chat/completions",
            &rh,
        )
        .await;

        assert!(result.is_valid(), "expected Valid, got {result:?}");
    }

    // Test 2: Replay rejection
    #[tokio::test]
    async fn test_replay_rejection() {
        let quote_id = "qt_replay1";
        let memo = compute_test_memo(quote_id, "POST", "/v1/chat/completions", b"{}");
        let logs = mock_receipt_logs(TEST_PAYER, TEST_PROVIDER, 5000, &memo, TEST_TOKEN);
        let receipt = mock_receipt(100, logs);
        let now = chrono::Utc::now().timestamp() as u64;
        let rpc_url = start_mock_rpc(Some(receipt), now).await;
        let (state, _db) = test_state(&rpc_url).await;

        let rh = hash::request_hash("POST", "/v1/chat/completions", b"{}");

        // First verification should succeed
        let r1 = verify_payment(
            &state,
            "0xreplay1",
            TEST_PAYER,
            Some(quote_id),
            "POST /v1/chat/completions",
            &rh,
        )
        .await;
        assert!(r1.is_valid());

        // Wait for DB write to flush
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Second verification with SAME quote_id should be ReplayDetected
        let r2 = verify_payment(
            &state,
            "0xreplay1",
            TEST_PAYER,
            Some(quote_id),
            "POST /v1/chat/completions",
            &rh,
        )
        .await;
        assert!(
            matches!(r2, VerificationResult::ReplayDetected),
            "expected ReplayDetected, got {r2:?}"
        );
    }

    // Test 3: Payer mismatch detection
    #[test]
    fn test_payer_mismatch_detection() {
        let provider: Address = TEST_PROVIDER.parse().unwrap();
        let token: Address = TEST_TOKEN.parse().unwrap();
        let wrong_payer = "0xAAAA000000000000000000000000000000000001";
        let memo = B256::ZERO;
        let logs = mock_receipt_logs(wrong_payer, TEST_PROVIDER, 5000, &memo, TEST_TOKEN);

        let transfer = decode_transfer_events(&logs, &provider, &token).unwrap();
        let expected_payer: Address = TEST_PAYER.parse().unwrap();
        assert_ne!(transfer.from, expected_payer);
    }

    // Test 4: Insufficient amount detection
    #[test]
    fn test_insufficient_amount_detection() {
        let provider: Address = TEST_PROVIDER.parse().unwrap();
        let token: Address = TEST_TOKEN.parse().unwrap();
        let memo = B256::ZERO;
        let logs = mock_receipt_logs(TEST_PAYER, TEST_PROVIDER, 100, &memo, TEST_TOKEN); // only 100, need 5000

        let transfer = decode_transfer_events(&logs, &provider, &token).unwrap();
        assert!(transfer.amount < 5000);
    }

    // Test 5: Expired transaction detection
    #[tokio::test]
    async fn test_expired_transaction_detection() {
        let quote_id = "qt_expired";
        let memo = compute_test_memo(quote_id, "POST", "/v1/chat/completions", b"{}");
        let logs = mock_receipt_logs(TEST_PAYER, TEST_PROVIDER, 5000, &memo, TEST_TOKEN);
        let receipt = mock_receipt(100, logs);
        // Block timestamp 600 seconds ago (> 300s expiry)
        let old_ts = chrono::Utc::now().timestamp() as u64 - 600;
        let rpc_url = start_mock_rpc(Some(receipt), old_ts).await;
        let (state, _db) = test_state(&rpc_url).await;

        let rh = hash::request_hash("POST", "/v1/chat/completions", b"{}");
        let result = verify_payment(
            &state,
            "0xexpired1",
            TEST_PAYER,
            Some(quote_id),
            "POST /v1/chat/completions",
            &rh,
        )
        .await;

        assert!(
            matches!(result, VerificationResult::ExpiredTransaction),
            "expected ExpiredTransaction, got {result:?}"
        );
    }

    // Test 6: Memo mismatch detection
    #[test]
    fn test_memo_mismatch_detection() {
        let wrong_memo = B256::repeat_byte(0xFF);
        let logs = mock_receipt_logs(TEST_PAYER, TEST_PROVIDER, 5000, &wrong_memo, TEST_TOKEN);

        let on_chain_memo = decode_memo_from_logs(&logs).unwrap();
        let rh = hash::request_hash("POST", "/v1/chat/completions", b"{}");
        let expected_memo = hash::payment_memo("qt_test", &rh);

        assert!(!constant_time_eq(
            on_chain_memo.as_slice(),
            expected_memo.as_slice()
        ));
    }

    // Test 7: Ambiguous transfer (multiple matching events) detection
    #[test]
    fn test_ambiguous_transfer_detection() {
        let provider: Address = TEST_PROVIDER.parse().unwrap();
        let token: Address = TEST_TOKEN.parse().unwrap();
        let memo = B256::ZERO;

        // Create logs with two Transfer events matching provider + token
        let mut logs = mock_receipt_logs(TEST_PAYER, TEST_PROVIDER, 5000, &memo, TEST_TOKEN);
        // Add a duplicate Transfer event
        logs.push(logs[0].clone());

        let result = decode_transfer_events(&logs, &provider, &token);
        assert!(
            matches!(result, Err(VerificationResult::AmbiguousTransfer)),
            "expected AmbiguousTransfer, got {result:?}"
        );
    }

    // Test 8: Null receipt → TxNotFound
    #[tokio::test]
    async fn test_null_receipt_tx_not_found() {
        let rpc_url = start_mock_rpc(None, 0).await;
        let (state, _db) = test_state(&rpc_url).await;

        let rh = hash::request_hash("GET", "/v1/models", b"");
        let result = verify_payment(
            &state,
            "0xnonexistent",
            TEST_PAYER,
            None,
            "GET /v1/models",
            &rh,
        )
        .await;

        assert!(
            matches!(result, VerificationResult::TxNotFound),
            "expected TxNotFound, got {result:?}"
        );
    }

    // Test 9: RPC error handling
    #[tokio::test]
    async fn test_rpc_error_handling() {
        // Point to a non-existent server
        let (state, _db) = test_state("http://127.0.0.1:1").await;

        let rh = hash::request_hash("GET", "/v1/models", b"");
        let result = verify_payment(
            &state,
            "0xtest",
            TEST_PAYER,
            None,
            "GET /v1/models",
            &rh,
        )
        .await;

        assert!(
            matches!(result, VerificationResult::RpcError(_)),
            "expected RpcError, got {result:?}"
        );
    }

    // Test 10: Quote honored within TTL
    #[tokio::test]
    async fn test_quote_honored_within_ttl() {
        let quote_id = "qt_honored";
        // Use a lower quote price (1000) than the endpoint price (5000)
        let memo = compute_test_memo(quote_id, "POST", "/v1/chat/completions", b"{}");
        // Payment of 1000 (matches quote, not endpoint price of 5000)
        let logs = mock_receipt_logs(TEST_PAYER, TEST_PROVIDER, 1000, &memo, TEST_TOKEN);
        let receipt = mock_receipt(100, logs);
        let now = chrono::Utc::now().timestamp() as u64;
        let rpc_url = start_mock_rpc(Some(receipt), now).await;
        let (state, db_path) = test_state(&rpc_url).await;

        // Insert a valid quote with price 1000 directly into DB
        let conn = Connection::open(&db_path).unwrap();
        let now_ts = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO quotes (id, endpoint, price, token_address, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![quote_id, "POST /v1/chat/completions", 1000i64, TEST_TOKEN, now_ts, now_ts + 300],
        ).unwrap();

        let rh = hash::request_hash("POST", "/v1/chat/completions", b"{}");
        let result = verify_payment(
            &state,
            "0xquoted1",
            TEST_PAYER,
            Some(quote_id),
            "POST /v1/chat/completions",
            &rh,
        )
        .await;

        assert!(result.is_valid(), "expected Valid with quoted price, got {result:?}");
    }

    // Test 11: Quote expired → fallback to current price
    #[tokio::test]
    async fn test_quote_expired_fallback() {
        let quote_id = "qt_expired_q";
        let memo = compute_test_memo(quote_id, "POST", "/v1/chat/completions", b"{}");
        // Payment of 1000, but expired quote → fallback to endpoint price 5000
        let logs = mock_receipt_logs(TEST_PAYER, TEST_PROVIDER, 1000, &memo, TEST_TOKEN);
        let receipt = mock_receipt(100, logs);
        let now = chrono::Utc::now().timestamp() as u64;
        let rpc_url = start_mock_rpc(Some(receipt), now).await;
        let (state, db_path) = test_state(&rpc_url).await;

        // Insert an expired quote
        let conn = Connection::open(&db_path).unwrap();
        let now_ts = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO quotes (id, endpoint, price, token_address, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![quote_id, "POST /v1/chat/completions", 1000i64, TEST_TOKEN, now_ts - 600, now_ts - 300],
        ).unwrap();

        let rh = hash::request_hash("POST", "/v1/chat/completions", b"{}");
        let result = verify_payment(
            &state,
            "0xexpq1",
            TEST_PAYER,
            Some(quote_id),
            "POST /v1/chat/completions",
            &rh,
        )
        .await;

        // Should fail with InsufficientAmount (1000 < 5000)
        assert!(
            matches!(result, VerificationResult::InsufficientAmount { expected: 5000, actual: 1000 }),
            "expected InsufficientAmount, got {result:?}"
        );
    }
}
