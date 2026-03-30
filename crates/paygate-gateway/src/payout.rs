//! On-chain payout: transfer USDC from gateway wallet to claim wallet.
//!
//! Uses the same EIP-155 signing and RLP encoding as cli.rs registration.

use alloy_primitives::{Address, U256, keccak256};
use tracing::{error, info, warn};

use crate::server::AppState;

/// Transfer USDC from the gateway wallet to a recipient address.
///
/// Returns the tx hash on success, or an error string on failure.
pub async fn transfer_usdc(
    state: &AppState,
    to: &str,
    amount_base_units: u64,
) -> Result<String, String> {
    let config = state.current_config();

    // Load private key from env
    let pk_env = &config.tempo.private_key_env;
    let private_key = std::env::var(pk_env)
        .map_err(|_| format!("Private key env var {pk_env} not set"))?;

    let pk_bytes = private_key.strip_prefix("0x").unwrap_or(&private_key);
    let pk_decoded = hex::decode(pk_bytes)
        .map_err(|_| "Invalid private key hex".to_string())?;
    if pk_decoded.len() != 32 {
        return Err("Private key must be 32 bytes".to_string());
    }

    let signing_key = k256::ecdsa::SigningKey::from_bytes(pk_decoded.as_slice().into())
        .map_err(|_| "Invalid private key".to_string())?;

    // Derive sender address
    let verifying_key = signing_key.verifying_key();
    let public_key_bytes = verifying_key.to_encoded_point(false);
    let sender_address = Address::from_slice(
        &keccak256(&public_key_bytes.as_bytes()[1..])[12..],
    );

    let to_address: Address = to.parse()
        .map_err(|_| format!("Invalid recipient address: {to}"))?;

    let token_address: Address = config.tempo.accepted_token.parse()
        .map_err(|_| "Invalid token address in config".to_string())?;

    let rpc_url = config.tempo.rpc_urls.first()
        .ok_or("No RPC URL configured".to_string())?;

    let chain_id = if config.tempo.chain_id > 0 {
        config.tempo.chain_id
    } else {
        42431 // Tempo testnet
    };

    // Encode ERC-20 transfer(address,uint256) call
    // function selector: keccak256("transfer(address,uint256)")[..4] = 0xa9059cbb
    let mut calldata = Vec::with_capacity(68);
    calldata.extend_from_slice(&[0xa9, 0x05, 0x9c, 0xbb]); // transfer selector
    calldata.extend_from_slice(&[0u8; 12]); // pad address to 32 bytes
    calldata.extend_from_slice(to_address.as_slice());
    let amount_u256 = U256::from(amount_base_units);
    calldata.extend_from_slice(&amount_u256.to_be_bytes::<32>());

    // Get nonce
    let nonce = rpc_get_nonce(&state.http_client, rpc_url, &sender_address).await
        .map_err(|e| format!("Failed to get nonce: {e}"))?;

    // Get gas price
    let gas_price = rpc_gas_price(&state.http_client, rpc_url).await
        .map_err(|e| format!("Failed to get gas price: {e}"))?;

    let gas_limit: u64 = 100_000; // ERC-20 transfer typically uses ~60k

    // Sign and send
    let raw_tx = sign_legacy_tx(
        &signing_key,
        nonce,
        gas_price,
        gas_limit,
        token_address,
        U256::ZERO,
        &calldata,
        chain_id,
    );

    let tx_hash = rpc_send_raw_tx(&state.http_client, rpc_url, &raw_tx).await
        .map_err(|e| format!("Transaction send failed: {e}"))?;

    info!(
        to = to,
        amount = amount_base_units,
        tx_hash = %tx_hash,
        "USDC payout sent"
    );

    Ok(tx_hash)
}

// ─── RPC helpers (non-panicking versions of cli.rs functions) ───────────────

async fn rpc_get_nonce(
    client: &reqwest::Client,
    rpc_url: &str,
    address: &Address,
) -> Result<u64, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionCount",
        "params": [format!("{address}"), "latest"],
        "id": 1
    });
    let resp = client
        .post(rpc_url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let hex_str = json["result"].as_str().unwrap_or("0x0");
    Ok(u64::from_str_radix(hex_str.trim_start_matches("0x"), 16).unwrap_or(0))
}

async fn rpc_gas_price(
    client: &reqwest::Client,
    rpc_url: &str,
) -> Result<u64, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_gasPrice",
        "params": [],
        "id": 1
    });
    let resp = client
        .post(rpc_url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let hex_str = json["result"].as_str().unwrap_or("0x3B9ACA00");
    Ok(u64::from_str_radix(hex_str.trim_start_matches("0x"), 16).unwrap_or(1_000_000_000))
}

async fn rpc_send_raw_tx(
    client: &reqwest::Client,
    rpc_url: &str,
    raw_tx: &[u8],
) -> Result<String, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_sendRawTransaction",
        "params": [format!("0x{}", hex::encode(raw_tx))],
        "id": 1
    });
    let resp = client
        .post(rpc_url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if let Some(error) = json.get("error") {
        let msg = error["message"].as_str().unwrap_or("unknown error");
        return Err(msg.to_string());
    }
    json["result"].as_str()
        .map(|s| s.to_string())
        .ok_or("No tx hash in response".to_string())
}

// ─── Transaction signing (same as cli.rs) ───────────────────────────────────

fn sign_legacy_tx(
    signing_key: &k256::ecdsa::SigningKey,
    nonce: u64,
    gas_price: u64,
    gas_limit: u64,
    to: Address,
    value: U256,
    data: &[u8],
    chain_id: u64,
) -> Vec<u8> {
    let mut unsigned = Vec::new();
    rlp_encode_u64(&mut unsigned, nonce);
    rlp_encode_u64(&mut unsigned, gas_price);
    rlp_encode_u64(&mut unsigned, gas_limit);
    rlp_encode_bytes(&mut unsigned, to.as_slice());
    rlp_encode_u256(&mut unsigned, value);
    rlp_encode_bytes(&mut unsigned, data);
    rlp_encode_u64(&mut unsigned, chain_id);
    rlp_encode_u64(&mut unsigned, 0);
    rlp_encode_u64(&mut unsigned, 0);

    let encoded_unsigned = rlp_encode_list(&unsigned);
    let msg_hash = keccak256(&encoded_unsigned);

    let (sig, recovery_id) = signing_key
        .sign_prehash_recoverable(msg_hash.as_slice())
        .expect("signing failed");

    let v = recovery_id.to_byte() as u64 + chain_id * 2 + 35;
    let r_bytes = sig.r().to_bytes();
    let s_bytes = sig.s().to_bytes();

    let mut signed = Vec::new();
    rlp_encode_u64(&mut signed, nonce);
    rlp_encode_u64(&mut signed, gas_price);
    rlp_encode_u64(&mut signed, gas_limit);
    rlp_encode_bytes(&mut signed, to.as_slice());
    rlp_encode_u256(&mut signed, value);
    rlp_encode_bytes(&mut signed, data);
    rlp_encode_u64(&mut signed, v);
    rlp_encode_bytes(&mut signed, trim_leading_zeros(&r_bytes));
    rlp_encode_bytes(&mut signed, trim_leading_zeros(&s_bytes));

    rlp_encode_list(&signed)
}

fn trim_leading_zeros(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    &bytes[start..]
}

fn rlp_encode_u64(buf: &mut Vec<u8>, val: u64) {
    if val == 0 {
        buf.push(0x80);
    } else if val < 128 {
        buf.push(val as u8);
    } else {
        let bytes = val.to_be_bytes();
        let start = bytes.iter().position(|&b| b != 0).unwrap_or(7);
        let len = 8 - start;
        buf.push(0x80 + len as u8);
        buf.extend_from_slice(&bytes[start..]);
    }
}

fn rlp_encode_u256(buf: &mut Vec<u8>, val: U256) {
    if val.is_zero() {
        buf.push(0x80);
    } else {
        let bytes: [u8; 32] = val.to_be_bytes();
        let start = bytes.iter().position(|&b| b != 0).unwrap_or(31);
        let trimmed = &bytes[start..];
        if trimmed.len() == 1 && trimmed[0] < 128 {
            buf.push(trimmed[0]);
        } else {
            buf.push(0x80 + trimmed.len() as u8);
            buf.extend_from_slice(trimmed);
        }
    }
}

fn rlp_encode_bytes(buf: &mut Vec<u8>, data: &[u8]) {
    if data.len() == 1 && data[0] < 128 {
        buf.push(data[0]);
    } else if data.len() < 56 {
        buf.push(0x80 + data.len() as u8);
        buf.extend_from_slice(data);
    } else {
        let len_bytes = data.len().to_be_bytes();
        let len_start = len_bytes.iter().position(|&b| b != 0).unwrap_or(7);
        let len_len = 8 - len_start;
        buf.push(0xb7 + len_len as u8);
        buf.extend_from_slice(&len_bytes[len_start..]);
        buf.extend_from_slice(data);
    }
}

fn rlp_encode_list(items: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    if items.len() < 56 {
        buf.push(0xc0 + items.len() as u8);
    } else {
        let len_bytes = items.len().to_be_bytes();
        let len_start = len_bytes.iter().position(|&b| b != 0).unwrap_or(7);
        let len_len = 8 - len_start;
        buf.push(0xf7 + len_len as u8);
        buf.extend_from_slice(&len_bytes[len_start..]);
    }
    buf.extend_from_slice(items);
    buf
}
