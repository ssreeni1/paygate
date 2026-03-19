use crate::config::Config;
use crate::db::{DbReader, DbWriter};
use metrics_exporter_prometheus::PrometheusHandle;
use std::sync::Arc;
use arc_swap::ArcSwap;

/// Shared application state accessible by all middleware and handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ArcSwap<Config>>,
    pub db_reader: DbReader,
    pub db_writer: DbWriter,
    pub http_client: reqwest::Client,
    pub prometheus_handle: PrometheusHandle,
    pub started_at: std::time::Instant,
}

impl AppState {
    pub fn current_config(&self) -> Arc<Config> {
        self.config.load_full()
    }
}
