use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config file not found: {0}")]
    NotFound(String),
    #[error("invalid TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("validation error: {field}: {message}")]
    Validation { field: String, message: String },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub gateway: GatewayConfig,
    pub tempo: TempoConfig,
    pub provider: ProviderConfig,
    #[serde(default)]
    pub sponsorship: SponsorshipConfig,
    #[serde(default)]
    pub sessions: SessionsConfig,
    #[serde(default)]
    pub pricing: PricingConfig,
    #[serde(default)]
    pub rate_limiting: RateLimitConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub webhooks: WebhookConfig,
    #[serde(default)]
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_admin_listen")]
    pub admin_listen: String,
    pub upstream: String,
    #[serde(default = "default_upstream_timeout")]
    pub upstream_timeout_seconds: u64,
    #[serde(default = "default_max_response_body")]
    pub max_response_body_bytes: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TempoConfig {
    #[serde(default = "default_network")]
    pub network: String,
    pub rpc_urls: Vec<String>,
    #[serde(default = "default_failover_timeout")]
    pub failover_timeout_ms: u64,
    #[serde(default = "default_rpc_pool_max_idle")]
    pub rpc_pool_max_idle: usize,
    #[serde(default = "default_rpc_timeout")]
    pub rpc_timeout_ms: u64,
    #[serde(default = "default_chain_id")]
    pub chain_id: u64,
    #[serde(default = "default_private_key_env")]
    pub private_key_env: String,
    #[serde(default)]
    pub accepted_token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub address: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SponsorshipConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_sponsor_listen")]
    pub sponsor_listen: String,
    #[serde(default)]
    pub budget_per_day: String,
    #[serde(default)]
    pub max_per_tx: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_discount")]
    pub discount_percent: u8,
    #[serde(default = "default_min_deposit")]
    pub minimum_deposit: String,
    #[serde(default = "default_max_duration")]
    pub max_duration_hours: u64,
    #[serde(default = "default_true")]
    pub auto_refund: bool,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_per_payer: u32,
}

impl Default for SessionsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            discount_percent: 0,
            minimum_deposit: "0.05".to_string(),
            max_duration_hours: 24,
            auto_refund: true,
            max_concurrent_per_payer: 5,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PricingConfig {
    #[serde(default = "default_price")]
    pub default_price: String,
    #[serde(default = "default_quote_ttl")]
    pub quote_ttl_seconds: u64,
    #[serde(default)]
    pub endpoints: HashMap<String, String>,
    #[serde(default)]
    pub dynamic: DynamicPricingConfig,
    #[serde(default)]
    pub tiers: HashMap<String, String>,
    #[serde(default)]
    pub no_charge_on_5xx: Vec<String>,
}

impl Default for PricingConfig {
    fn default() -> Self {
        Self {
            default_price: "0.001".to_string(),
            quote_ttl_seconds: 300,
            endpoints: HashMap::new(),
            dynamic: DynamicPricingConfig::default(),
            tiers: HashMap::new(),
            no_charge_on_5xx: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DynamicPricingConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token_price: String,
    #[serde(default)]
    pub compute_price: String,
    #[serde(default)]
    pub header_source: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_rps")]
    pub requests_per_second: u32,
    #[serde(default = "default_per_payer_rps")]
    pub per_payer_per_second: u32,
    #[serde(default = "default_min_interval")]
    pub min_payment_interval_ms: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 100,
            per_payer_per_second: 10,
            min_payment_interval_ms: 100,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    #[serde(default = "default_true")]
    pub require_payment_before_forward: bool,
    #[serde(default = "default_max_body")]
    pub max_request_body_bytes: usize,
    #[serde(default = "default_tx_expiry")]
    pub tx_expiry_seconds: u64,
    #[serde(default = "default_true")]
    pub replay_protection: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            require_payment_before_forward: true,
            max_request_body_bytes: 10_485_760,
            tx_expiry_seconds: 300,
            replay_protection: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WebhookConfig {
    #[serde(default)]
    pub payment_verified_url: String,
    #[serde(default = "default_webhook_timeout")]
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_retention_days")]
    pub request_log_retention_days: u32,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            request_log_retention_days: 30,
        }
    }
}

// Default value functions
fn default_listen() -> String { "0.0.0.0:8080".to_string() }
fn default_admin_listen() -> String { "127.0.0.1:8081".to_string() }
fn default_upstream_timeout() -> u64 { 30 }
fn default_max_response_body() -> usize { 10_485_760 }
fn default_network() -> String { "testnet".to_string() }
fn default_failover_timeout() -> u64 { 2000 }
fn default_rpc_pool_max_idle() -> usize { 10 }
fn default_rpc_timeout() -> u64 { 5000 }
fn default_chain_id() -> u64 { 4217 }
fn default_private_key_env() -> String { "PAYGATE_PRIVATE_KEY".to_string() }
fn default_sponsor_listen() -> String { "/paygate/sponsor".to_string() }
fn default_true() -> bool { true }
fn default_discount() -> u8 { 0 }
fn default_min_deposit() -> String { "0.05".to_string() }
fn default_max_duration() -> u64 { 24 }
fn default_max_concurrent() -> u32 { 5 }
fn default_price() -> String { "0.001".to_string() }
fn default_quote_ttl() -> u64 { 300 }
fn default_rps() -> u32 { 100 }
fn default_per_payer_rps() -> u32 { 10 }
fn default_min_interval() -> u64 { 100 }
fn default_max_body() -> usize { 10_485_760 }
fn default_tx_expiry() -> u64 { 300 }
fn default_webhook_timeout() -> u64 { 5 }
fn default_retention_days() -> u32 { 30 }

impl Config {
    /// Load config from a TOML file.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Err(ConfigError::NotFound(path.display().to_string()));
        }
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate all config fields.
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Provider address
        validate_address(&self.provider.address, "provider.address")?;

        // At least one RPC URL
        if self.tempo.rpc_urls.is_empty() {
            return Err(ConfigError::Validation {
                field: "tempo.rpc_urls".to_string(),
                message: "at least one RPC URL is required".to_string(),
            });
        }

        // Upstream URL
        if self.gateway.upstream.is_empty() {
            return Err(ConfigError::Validation {
                field: "gateway.upstream".to_string(),
                message: "upstream URL is required".to_string(),
            });
        }

        // Webhook URL validation (SSRF protection)
        if !self.webhooks.payment_verified_url.is_empty() {
            validate_webhook_url(&self.webhooks.payment_verified_url)?;
        }

        // Accepted token (if set)
        if !self.tempo.accepted_token.is_empty() {
            validate_address(&self.tempo.accepted_token, "tempo.accepted_token")?;
        }

        // Prices must be non-negative
        validate_price(&self.pricing.default_price, "pricing.default_price")?;
        for (endpoint, price) in &self.pricing.endpoints {
            validate_price(price, &format!("pricing.endpoints.{endpoint}"))?;
        }

        Ok(())
    }

    /// Check if an endpoint has no_charge_on_5xx enabled.
    pub fn is_no_charge_on_5xx(&self, endpoint: &str) -> bool {
        self.pricing.no_charge_on_5xx.iter().any(|e| e == endpoint)
    }

    /// Get the price for an endpoint in base units.
    pub fn price_for_endpoint(&self, endpoint: &str) -> u64 {
        if let Some(price_str) = self.pricing.endpoints.get(endpoint) {
            parse_price_to_base_units(price_str).unwrap_or(0)
        } else {
            parse_price_to_base_units(&self.pricing.default_price).unwrap_or(1000)
        }
    }
}

fn validate_address(addr: &str, field: &str) -> Result<(), ConfigError> {
    if !addr.starts_with("0x") || addr.len() != 42 {
        return Err(ConfigError::Validation {
            field: field.to_string(),
            message: "must start with 0x and be 42 characters".to_string(),
        });
    }
    if hex::decode(&addr[2..]).is_err() {
        return Err(ConfigError::Validation {
            field: field.to_string(),
            message: "invalid hex characters".to_string(),
        });
    }
    Ok(())
}

fn validate_webhook_url(url: &str) -> Result<(), ConfigError> {
    // Must be HTTPS
    if !url.starts_with("https://") {
        return Err(ConfigError::Validation {
            field: "webhooks.payment_verified_url".to_string(),
            message: "webhook URL must use HTTPS".to_string(),
        });
    }

    // Extract host and check for private IPs
    if let Some(host) = url
        .strip_prefix("https://")
        .and_then(|s| s.split('/').next())
        .and_then(|s| s.split(':').next())
    {
        let lower = host.to_lowercase();
        if lower == "localhost"
            || lower == "127.0.0.1"
            || lower.starts_with("10.")
            || lower.starts_with("192.168.")
            || lower.starts_with("169.254.")
        {
            return Err(ConfigError::Validation {
                field: "webhooks.payment_verified_url".to_string(),
                message: "webhook URL must not point to private/localhost addresses".to_string(),
            });
        }
        // Check 172.16.0.0/12
        if lower.starts_with("172.") {
            if let Some(second_octet) = lower.split('.').nth(1) {
                if let Ok(octet) = second_octet.parse::<u8>() {
                    if (16..=31).contains(&octet) {
                        return Err(ConfigError::Validation {
                            field: "webhooks.payment_verified_url".to_string(),
                            message: "webhook URL must not point to private addresses".to_string(),
                        });
                    }
                }
            }
        }
    }

    Ok(())
}

fn validate_price(price: &str, field: &str) -> Result<(), ConfigError> {
    let parsed: f64 = price.parse().map_err(|_| ConfigError::Validation {
        field: field.to_string(),
        message: "must be a valid decimal number".to_string(),
    })?;
    if parsed < 0.0 {
        return Err(ConfigError::Validation {
            field: field.to_string(),
            message: "price must be non-negative".to_string(),
        });
    }
    Ok(())
}

/// Parse a decimal price string (e.g., "0.001") to base units (e.g., 1000) with 6 decimals.
pub fn parse_price_to_base_units(price: &str) -> Result<u64, ConfigError> {
    let parsed: f64 = price.parse().map_err(|_| ConfigError::Validation {
        field: "price".to_string(),
        message: format!("invalid price: {price}"),
    })?;
    Ok((parsed * 1_000_000.0).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_minimal_config() {
        let toml = r#"
[gateway]
upstream = "http://localhost:3000"

[tempo]
rpc_urls = ["https://rpc.presto.tempo.xyz"]

[provider]
address = "0x7F3a000000000000000000000000000000000001"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        config.validate().unwrap();
        assert_eq!(config.gateway.listen, "0.0.0.0:8080");
        assert_eq!(config.pricing.default_price, "0.001");
    }

    #[test]
    fn test_invalid_address() {
        let toml = r#"
[gateway]
upstream = "http://localhost:3000"
[tempo]
rpc_urls = ["https://rpc.presto.tempo.xyz"]
[provider]
address = "not-an-address"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_private_webhook_url_rejected() {
        assert!(validate_webhook_url("https://192.168.1.1/hook").is_err());
        assert!(validate_webhook_url("https://10.0.0.1/hook").is_err());
        assert!(validate_webhook_url("https://localhost/hook").is_err());
        assert!(validate_webhook_url("http://example.com/hook").is_err());
        assert!(validate_webhook_url("https://172.16.0.1/hook").is_err());
    }

    #[test]
    fn test_valid_webhook_url() {
        assert!(validate_webhook_url("https://hooks.example.com/paygate").is_ok());
    }

    #[test]
    fn test_price_parsing() {
        assert_eq!(parse_price_to_base_units("0.001").unwrap(), 1000);
        assert_eq!(parse_price_to_base_units("0.005").unwrap(), 5000);
        assert_eq!(parse_price_to_base_units("1.000").unwrap(), 1_000_000);
        assert_eq!(parse_price_to_base_units("0.000").unwrap(), 0);
    }

    #[test]
    fn test_endpoint_pricing() {
        let toml = r#"
[gateway]
upstream = "http://localhost:3000"
[tempo]
rpc_urls = ["https://rpc.presto.tempo.xyz"]
[provider]
address = "0x7F3a000000000000000000000000000000000001"
[pricing]
default_price = "0.001"
[pricing.endpoints]
"POST /v1/chat/completions" = "0.005"
"GET /v1/models" = "0.000"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.price_for_endpoint("POST /v1/chat/completions"), 5000);
        assert_eq!(config.price_for_endpoint("GET /v1/models"), 0);
        assert_eq!(config.price_for_endpoint("POST /v1/unknown"), 1000);
    }
}
