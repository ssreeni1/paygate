use crate::config::Config;
use crate::db::{DbReader, DbWriter};
use crate::rate_limit::RateLimiter;
use crate::sessions::SpendAccumulator;
use crate::webhook::WebhookSender;
use arc_swap::ArcSwap;
use metrics_exporter_prometheus::PrometheusHandle;
use std::sync::Arc;

/// Shared application state accessible by all middleware and handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ArcSwap<Config>>,
    pub db_reader: DbReader,
    pub db_writer: DbWriter,
    pub http_client: reqwest::Client,
    pub rate_limiter: Arc<RateLimiter>,
    pub webhook_sender: Option<WebhookSender>,
    pub prometheus_handle: PrometheusHandle,
    pub started_at: std::time::Instant,
    pub spend_accumulator: Arc<SpendAccumulator>,
}

impl AppState {
    pub fn current_config(&self) -> Arc<Config> {
        self.config.load_full()
    }
}
