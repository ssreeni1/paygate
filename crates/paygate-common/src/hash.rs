use alloy_primitives::{B256, keccak256};

/// Compute requestHash = keccak256(method || " " || path || "\n" || body).
///
/// Both the gateway verifier and client SDKs must compute this identically.
/// Method is uppercase, path includes query string, body is raw bytes (empty for GET/DELETE).
pub fn request_hash(method: &str, path: &str, body: &[u8]) -> B256 {
    let mut input = Vec::with_capacity(method.len() + 1 + path.len() + 1 + body.len());
    input.extend_from_slice(method.as_bytes());
    input.push(b' ');
    input.extend_from_slice(path.as_bytes());
    input.push(b'\n');
    input.extend_from_slice(body);
    keccak256(&input)
}

/// Compute session deposit memo = keccak256("paygate-session" || nonce).
///
/// Used to bind a session deposit transaction to a server-issued nonce.
pub fn session_deposit_memo(nonce: &str) -> B256 {
    let mut input = Vec::with_capacity(15 + nonce.len());
    input.extend_from_slice(b"paygate-session");
    input.extend_from_slice(nonce.as_bytes());
    keccak256(&input)
}

/// Compute memo = keccak256("paygate" || quoteId || requestHash).
///
/// Inputs are UTF-8 encoded and concatenated as raw bytes before hashing.
/// keccak256 output is already bytes32 (no truncation needed).
pub fn payment_memo(quote_id: &str, request_hash: &B256) -> B256 {
    let mut input = Vec::with_capacity(7 + quote_id.len() + 32);
    input.extend_from_slice(b"paygate");
    input.extend_from_slice(quote_id.as_bytes());
    input.extend_from_slice(request_hash.as_slice());
    keccak256(&input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cross_language_hash_vectors() {
        let json_str = include_str!("../../../tests/fixtures/request_hash_vectors.json");
        let vectors: serde_json::Value = serde_json::from_str(json_str).unwrap();

        // Verify request_hash vectors
        for vector in vectors["request_hash_vectors"].as_array().unwrap() {
            let method = vector["method"].as_str().unwrap();
            let path = vector["path"].as_str().unwrap();
            let body = vector["body"].as_str().unwrap();
            let expected_hash = vector["expected_hash"].as_str().unwrap();

            let hash = request_hash(method, path, body.as_bytes());
            let hash_hex = format!("0x{}", hex::encode(hash.as_slice()));
            assert_eq!(
                hash_hex, expected_hash,
                "Hash mismatch for vector: {}",
                vector["description"].as_str().unwrap()
            );

            // Also verify input_hex
            let expected_input_hex = vector["input_hex"].as_str().unwrap();
            let mut input = Vec::new();
            input.extend_from_slice(method.as_bytes());
            input.push(b' ');
            input.extend_from_slice(path.as_bytes());
            input.push(b'\n');
            input.extend_from_slice(body.as_bytes());
            assert_eq!(
                hex::encode(&input),
                expected_input_hex,
                "Input hex mismatch for vector: {}",
                vector["description"].as_str().unwrap()
            );
        }

        // Verify memo vectors
        for memo_vector in vectors["memo_vectors"].as_array().unwrap() {
            let quote_id = memo_vector["quote_id"].as_str().unwrap();
            let expected_memo = memo_vector["expected_memo"].as_str().unwrap();
            let rh_index = memo_vector["request_hash_vector_index"].as_u64().unwrap() as usize;

            // Get the request hash from the referenced vector
            let rh_vector = &vectors["request_hash_vectors"][rh_index];
            let rh = request_hash(
                rh_vector["method"].as_str().unwrap(),
                rh_vector["path"].as_str().unwrap(),
                rh_vector["body"].as_str().unwrap().as_bytes(),
            );

            let memo = payment_memo(quote_id, &rh);
            let memo_hex = format!("0x{}", hex::encode(memo.as_slice()));
            assert_eq!(
                memo_hex, expected_memo,
                "Memo mismatch for vector: {}",
                memo_vector["description"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn test_request_hash_post() {
        let hash = request_hash("POST", "/v1/chat/completions", b"{\"model\":\"gpt-4\"}");
        // Deterministic — same input always produces same hash.
        let hash2 = request_hash("POST", "/v1/chat/completions", b"{\"model\":\"gpt-4\"}");
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_request_hash_get_empty_body() {
        let hash = request_hash("GET", "/v1/models", b"");
        assert_ne!(hash, B256::ZERO);
    }

    #[test]
    fn test_request_hash_different_methods() {
        let get_hash = request_hash("GET", "/v1/models", b"");
        let post_hash = request_hash("POST", "/v1/models", b"");
        assert_ne!(get_hash, post_hash);
    }

    #[test]
    fn test_payment_memo_deterministic() {
        let rh = request_hash("POST", "/v1/chat/completions", b"{\"model\":\"gpt-4\"}");
        let memo1 = payment_memo("qt_abc123", &rh);
        let memo2 = payment_memo("qt_abc123", &rh);
        assert_eq!(memo1, memo2);
    }

    #[test]
    fn test_payment_memo_different_quotes() {
        let rh = request_hash("POST", "/v1/chat/completions", b"{}");
        let memo1 = payment_memo("qt_abc123", &rh);
        let memo2 = payment_memo("qt_def456", &rh);
        assert_ne!(memo1, memo2);
    }
}
