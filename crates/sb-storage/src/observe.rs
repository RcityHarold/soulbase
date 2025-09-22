use std::time::Duration;

use sb_types::prelude::TenantId;

pub struct StorageLabels<'a> {
    pub tenant: &'a str,
    pub table: &'a str,
    pub kind: &'a str,
    pub code: Option<&'a str>,
}

pub trait StorageMetrics: Send + Sync {
    fn record_request(
        &self,
        _labels: StorageLabels<'_>,
        _rows: u64,
        _bytes: u64,
        _latency: Duration,
    ) {
    }
    fn record_tx_rollback(&self, _tenant: &TenantId) {}
}

#[derive(Default)]
pub struct NoopStorageMetrics;

impl StorageMetrics for NoopStorageMetrics {}

pub static NOOP_STORAGE_METRICS: NoopStorageMetrics = NoopStorageMetrics;
