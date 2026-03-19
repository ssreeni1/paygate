/// Machine Payments Protocol (MPP) header constants.
///
/// These are PayGate-specific until the formal MPP wire spec is published by Tempo.
/// Track: https://github.com/tempoxyz/tempo/issues

// Request headers (sent by consumer)
pub const HEADER_PAYMENT_TX: &str = "X-Payment-Tx";
pub const HEADER_PAYMENT_PAYER: &str = "X-Payment-Payer";
pub const HEADER_PAYMENT_QUOTE_ID: &str = "X-Payment-Quote-Id";
pub const HEADER_PAYMENT_SESSION: &str = "X-Payment-Session";
pub const HEADER_PAYMENT_SESSION_SIG: &str = "X-Payment-Session-Sig";
pub const HEADER_PAYMENT_TIMESTAMP: &str = "X-Payment-Timestamp";

// Response headers (sent by gateway in 402)
pub const HEADER_PAYMENT_REQUIRED: &str = "X-Payment-Required";
pub const HEADER_PAYMENT_AMOUNT: &str = "X-Payment-Amount";
pub const HEADER_PAYMENT_DECIMALS: &str = "X-Payment-Decimals";
pub const HEADER_PAYMENT_TOKEN: &str = "X-Payment-Token";
pub const HEADER_PAYMENT_RECIPIENT: &str = "X-Payment-Recipient";
pub const HEADER_PAYMENT_NETWORK: &str = "X-Payment-Network";
pub const HEADER_PAYMENT_CHAIN_ID: &str = "X-Payment-Chain-Id";
pub const HEADER_PAYMENT_QUOTE_ID_RESP: &str = "X-Payment-Quote-Id";
pub const HEADER_PAYMENT_QUOTE_EXPIRES: &str = "X-Payment-Quote-Expires";
pub const HEADER_PAYMENT_METHODS: &str = "X-Payment-Methods";
pub const HEADER_PAYMENT_SHORTFALL: &str = "X-Payment-Shortfall";

// Response headers (sent by gateway on success)
pub const HEADER_PAYMENT_RECEIPT: &str = "X-Payment-Receipt";
pub const HEADER_PAYMENT_COST: &str = "X-Payment-Cost";

/// Prefix for all payment headers — used by the sanitizer to strip before forwarding.
pub const PAYMENT_HEADER_PREFIX: &str = "x-payment-";

/// Check if a header name is a payment header that should be stripped.
pub fn is_payment_header(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with(PAYMENT_HEADER_PREFIX)
}
