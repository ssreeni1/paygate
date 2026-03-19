use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use governor::{Quota, RateLimiter as GovRateLimiter};
use paygate_common::mpp::HEADER_PAYMENT_PAYER;
use serde_json::json;
use std::num::NonZeroU32;
use crate::metrics;
use crate::server::AppState;

pub struct RateLimiter {
    global: GovRateLimiter<
        governor::state::NotKeyed,
        governor::state::InMemoryState,
        governor::clock::DefaultClock,
    >,
    per_payer: GovRateLimiter<
        String,
        governor::state::keyed::DashMapStateStore<String>,
        governor::clock::DefaultClock,
    >,
    per_ip_402: GovRateLimiter<
        String,
        governor::state::keyed::DashMapStateStore<String>,
        governor::clock::DefaultClock,
    >,
}

impl RateLimiter {
    pub fn new(global_rps: u32, per_payer_rps: u32) -> Self {
        Self {
            global: GovRateLimiter::direct(Quota::per_second(
                NonZeroU32::new(global_rps.max(1)).unwrap(),
            )),
            per_payer: GovRateLimiter::keyed(Quota::per_second(
                NonZeroU32::new(per_payer_rps.max(1)).unwrap(),
            )),
            per_ip_402: GovRateLimiter::keyed(Quota::per_minute(
                NonZeroU32::new(1000).unwrap(),
            )),
        }
    }

    pub fn check_global(&self) -> bool {
        self.global.check().is_ok()
    }

    pub fn check_per_payer(&self, key: &str) -> bool {
        self.per_payer.check_key(&key.to_string()).is_ok()
    }

    pub fn check_402_flood(&self, ip: &str) -> bool {
        self.per_ip_402.check_key(&ip.to_string()).is_ok()
    }
}

/// Rate limiting middleware.
pub async fn rate_limit_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let payer_key = request
        .headers()
        .get(HEADER_PAYMENT_PAYER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    if !state.rate_limiter.check_global() {
        metrics::record_rate_limit_rejected();
        return rate_limit_response();
    }

    if !state.rate_limiter.check_per_payer(&payer_key) {
        metrics::record_rate_limit_rejected();
        return rate_limit_response();
    }

    next.run(request).await
}

fn rate_limit_response() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(json!({
            "error": "rate_limit_exceeded",
            "message": "Too many requests. Please slow down.",
            "retry_after": 1
        })),
    )
        .into_response()
}

// Test 15: Rate limiter returns 429
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_rejects_at_threshold() {
        let limiter = RateLimiter::new(1, 1);
        assert!(limiter.check_global(), "first request should be allowed");
        assert!(
            !limiter.check_global(),
            "second request should be rejected (1 rps)"
        );
    }

    #[test]
    fn test_per_payer_rate_limit() {
        let limiter = RateLimiter::new(100, 1);
        assert!(limiter.check_per_payer("payer1"));
        assert!(!limiter.check_per_payer("payer1"));
        // Different payer should still be allowed
        assert!(limiter.check_per_payer("payer2"));
    }
}
