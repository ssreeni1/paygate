use alloy_primitives::{Address, B256, U256};
use serde::{Deserialize, Serialize};

/// Amount in token base units (e.g., 1000 = 0.001 USDC with 6 decimals).
pub type BaseUnits = u64;

/// Token decimals (USDC = 6).
pub const TOKEN_DECIMALS: u8 = 6;

/// A verified payment proof extracted from on-chain data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentProof {
    pub tx_hash: B256,
    pub payer: Address,
    pub recipient: Address,
    pub amount: BaseUnits,
    pub token: Address,
    pub memo: B256,
    pub block_number: u64,
}

/// Result of payment verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationResult {
    Valid(PaymentProof),
    TxNotFound,
    RpcError(String),
    InvalidTransfer(String),
    AmbiguousTransfer,
    InsufficientAmount { expected: BaseUnits, actual: BaseUnits },
    PayerMismatch { expected: Address, actual: Address },
    ReplayDetected,
    ExpiredTransaction,
    MemoMismatch { expected: B256, actual: B256 },
    QuoteExpired,
}

impl VerificationResult {
    /// Returns true if the payment is valid.
    pub fn is_valid(&self) -> bool {
        matches!(self, VerificationResult::Valid(_))
    }

    /// Returns the verification step name for structured logging.
    pub fn step_name(&self) -> &'static str {
        match self {
            VerificationResult::Valid(_) => "complete",
            VerificationResult::TxNotFound => "receipt_fetch",
            VerificationResult::RpcError(_) => "receipt_fetch",
            VerificationResult::InvalidTransfer(_) => "event_decode",
            VerificationResult::AmbiguousTransfer => "event_decode",
            VerificationResult::InsufficientAmount { .. } => "amount_check",
            VerificationResult::PayerMismatch { .. } => "payer_binding",
            VerificationResult::ReplayDetected => "replay_check",
            VerificationResult::ExpiredTransaction => "tx_age_check",
            VerificationResult::MemoMismatch { .. } => "memo_check",
            VerificationResult::QuoteExpired => "quote_check",
        }
    }
}

/// Pricing info returned in 402 responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingInfo {
    pub amount: String,
    pub amount_base_units: BaseUnits,
    pub decimals: u8,
    pub token: Address,
    pub recipient: Address,
    pub quote_id: String,
    pub quote_expires_at: String,
    pub methods: Vec<String>,
}

/// Quote stored in the database.
#[derive(Debug, Clone)]
pub struct Quote {
    pub id: String,
    pub endpoint: String,
    pub price: BaseUnits,
    pub token: Address,
    pub created_at: i64,
    pub expires_at: i64,
}

/// Payment record stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRecord {
    pub id: String,
    pub tx_hash: String,
    pub payer_address: String,
    pub amount: BaseUnits,
    pub token_address: String,
    pub endpoint: String,
    pub request_hash: Option<String>,
    pub quote_id: Option<String>,
    pub block_number: u64,
    pub verified_at: i64,
    pub status: String,
}

/// Format base units as a decimal string (e.g., 1000 → "0.001000" for 6 decimals).
pub fn format_amount(base_units: BaseUnits, decimals: u8) -> String {
    let divisor = 10u64.pow(decimals as u32);
    let whole = base_units / divisor;
    let frac = base_units % divisor;
    format!("{whole}.{frac:0>width$}", width = decimals as usize)
}

/// Format base units as USD string (e.g., 1000 → "$0.00" for 6 decimals).
/// Uses integer math only — no floating point.
pub fn format_usd(base_units: BaseUnits, decimals: u8) -> String {
    let divisor = 10u64.pow(decimals as u32);
    let dollars = base_units / divisor;
    let cents = (base_units % divisor) / (divisor / 100);
    format!("${dollars}.{cents:02}")
}
