use crate::config::Config;
use crate::db::{DbReader, DbWriter};
use crate::rate_limit::RateLimiter;
use crate::webhook::WebhookSender;
use arc_swap::ArcSwap;
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
}

impl AppState {
    pub fn current_config(&self) -> Arc<Config> {
        self.config.load_full()
    }
}
