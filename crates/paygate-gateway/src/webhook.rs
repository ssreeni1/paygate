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
