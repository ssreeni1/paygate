use metrics::{counter, gauge, histogram};

// Payment verification
pub fn record_payment_verified(endpoint: &str, status: &str) {
    counter!("paygate_payments_verified_total", "endpoint" => endpoint.to_string(), "status" => status.to_string()).increment(1);
}

pub fn record_verification_duration(duration_secs: f64) {
    histogram!("paygate_payment_verification_duration_seconds").record(duration_secs);
}

// Upstream proxy
pub fn record_upstream_duration(endpoint: &str, status_code: u16, duration_secs: f64) {
    histogram!("paygate_upstream_request_duration_seconds", "endpoint" => endpoint.to_string(), "status_code" => status_code.to_string()).record(duration_secs);
}

// Revenue
pub fn record_revenue(token: &str, amount: u64) {
    counter!("paygate_revenue_total_base_units", "token" => token.to_string()).increment(amount);
}

// Sessions
pub fn set_active_sessions(count: u64) {
    gauge!("paygate_active_sessions").set(count as f64);
}

// Rate limiting
pub fn record_rate_limit_rejected() {
    counter!("paygate_rate_limit_rejected_total").increment(1);
}

// RPC errors
pub fn record_rpc_error() {
    counter!("paygate_rpc_errors_total").increment(1);
}

// DB errors
pub fn record_db_error() {
    counter!("paygate_db_errors_total").increment(1);
}

// DB writer queue
pub fn set_writer_queue_depth(depth: usize) {
    gauge!("paygate_db_writer_queue_depth").set(depth as f64);
}

// Webhooks
pub fn record_webhook_delivery(status: &str) {
    counter!("paygate_webhook_delivery_total", "status" => status.to_string()).increment(1);
}

// Quotes
pub fn set_active_quotes(count: u64) {
    gauge!("paygate_quotes_active").set(count as f64);
}

// Config reloads
pub fn record_config_reload(status: &str) {
    counter!("paygate_config_reloads_total", "status" => status.to_string()).increment(1);
}
