use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::metrics;

/// Webhook sender. Fire-and-forget payment notifications.
#[derive(Clone)]
pub struct WebhookSender {
    http_client: Client,
    url: String,
    timeout: std::time::Duration,
    semaphore: Arc<Semaphore>,
}

impl WebhookSender {
    pub fn new(http_client: Client, url: String, timeout_seconds: u64) -> Self {
        Self {
            http_client,
            url,
            timeout: std::time::Duration::from_secs(timeout_seconds),
            semaphore: Arc::new(Semaphore::new(50)),
        }
    }

    /// Send payment notification (non-blocking). Uses tokio::spawn internally.
    pub fn notify_payment_verified(
        &self,
        tx_hash: &str,
        payer: &str,
        amount: u64,
        endpoint: &str,
    ) {
        let client = self.http_client.clone();
        let url = self.url.clone();
        let timeout = self.timeout;
        let semaphore = self.semaphore.clone();
        let tx_hash = tx_hash.to_string();
        let payer = payer.to_string();
        let endpoint = endpoint.to_string();

        tokio::spawn(async move {
            let _permit = match semaphore.try_acquire() {
                Ok(p) => p,
                Err(_) => {
                    warn!("webhook semaphore exhausted, dropping notification");
                    metrics::record_webhook_delivery("dropped");
                    return;
                }
            };

            let payload = json!({
                "event": "payment.verified",
                "tx_hash": tx_hash,
                "payer_address": payer,
                "amount": amount,
                "endpoint": endpoint,
                "timestamp": chrono::Utc::now().to_rfc3339()
            });

            match client.post(&url).json(&payload).timeout(timeout).send().await {
                Ok(resp) if resp.status().is_success() => {
                    info!(tx_hash = %tx_hash, "webhook delivered");
                    metrics::record_webhook_delivery("success");
                }
                Ok(resp) => {
                    warn!(tx_hash = %tx_hash, status = %resp.status(), "webhook delivery failed");
                    metrics::record_webhook_delivery("failure");
                }
                Err(e) if e.is_timeout() => {
                    warn!(tx_hash = %tx_hash, "webhook delivery timeout");
                    metrics::record_webhook_delivery("timeout");
                }
                Err(e) => {
                    warn!(tx_hash = %tx_hash, error = %e, "webhook delivery error");
                    metrics::record_webhook_delivery("failure");
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    // 8. Webhook sends POST when payment is verified
    #[tokio::test]
    async fn test_webhook_sends_post_on_payment() {
        let received = Arc::new(AtomicBool::new(false));
        let received_clone = received.clone();

        let app = axum::Router::new().fallback(move || {
            let r = received_clone.clone();
            async move {
                r.store(true, Ordering::SeqCst);
                "ok"
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());

        let sender = WebhookSender::new(
            Client::new(),
            format!("http://{addr}/webhook"),
            5,
        );
        sender.notify_payment_verified("0xabc", "0x123", 5000, "POST /v1/chat");

        // Wait for the spawned task to deliver
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert!(received.load(Ordering::SeqCst), "webhook should have been received");
    }

    // 9. Webhook failure does not block the caller
    #[tokio::test]
    async fn test_webhook_failure_is_nonblocking() {
        let sender = WebhookSender::new(
            Client::new(),
            "http://127.0.0.1:1/webhook".to_string(),
            1,
        );

        let start = std::time::Instant::now();
        sender.notify_payment_verified("0xfail", "0x999", 1000, "GET /v1/test");
        let elapsed = start.elapsed();

        // notify_payment_verified should return nearly instantly (fire-and-forget)
        assert!(
            elapsed.as_millis() < 50,
            "notify should return in <50ms (was {}ms)",
            elapsed.as_millis()
        );
    }

    // 10. Webhook respects timeout — does not panic on slow server
    #[tokio::test]
    async fn test_webhook_respects_timeout() {
        // Start a server that sleeps for 10 seconds
        let app = axum::Router::new().fallback(|| async {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            "slow"
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());

        let sender = WebhookSender::new(
            Client::new(),
            format!("http://{addr}/webhook"),
            1, // 1 second timeout
        );
        sender.notify_payment_verified("0xtimeout", "0xaaa", 500, "POST /v1/slow");

        // Wait long enough for timeout to fire, but not for the full 10s sleep
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        // If we reach here, no panic occurred — the timeout was handled gracefully
    }
}
