#![cfg(feature = "surreal")]

use std::sync::Arc;
use std::time::Duration;

use sb_types::prelude::TenantId;

use crate::observe::{StorageLabels, StorageMetrics};
use crate::spi::metrics_labels;

#[derive(Clone)]
pub struct SurrealMetricsProxy {
    metrics: Arc<dyn StorageMetrics>,
}

impl SurrealMetricsProxy {
    pub fn new(metrics: Arc<dyn StorageMetrics>) -> Self {
        Self { metrics }
    }

    pub fn as_arc(&self) -> Arc<dyn StorageMetrics> {
        Arc::clone(&self.metrics)
    }

    pub fn record_request(
        &self,
        tenant: &TenantId,
        table: &str,
        kind: &str,
        code: Option<&str>,
        rows: u64,
        bytes: u64,
        latency: Duration,
    ) {
        let labels: StorageLabels<'_> = metrics_labels(tenant, table, kind, code);
        self.metrics.record_request(labels, rows, bytes, latency);
    }

    pub fn record_tx_rollback(&self, tenant: &TenantId) {
        self.metrics.record_tx_rollback(tenant);
    }
}
