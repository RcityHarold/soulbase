use std::collections::BTreeMap;
use std::time::Duration;

use sb_types::prelude::TenantId;

use crate::model::{DeadKind, IdempoStatus, SagaState};

pub fn labels(tenant: &str, kind: &str, code: Option<&str>) -> BTreeMap<&'static str, String> {
    let mut map = BTreeMap::new();
    map.insert("tenant", tenant.to_string());
    map.insert("kind", kind.to_string());
    if let Some(code) = code {
        map.insert("code", code.to_string());
    }
    map
}

pub trait TxMetrics: Send + Sync {
    fn record_outbox_enqueue(&self, tenant: &TenantId, topic: &str) {
        let _ = (tenant, topic);
    }

    fn record_outbox_dispatch(
        &self,
        tenant: &TenantId,
        topic: &str,
        attempts: u32,
        success: bool,
        code: Option<&str>,
        latency: Option<Duration>,
    ) {
        let _ = (tenant, topic, attempts, success, code, latency);
    }

    fn record_outbox_dead_letter(&self, tenant: &TenantId, topic: &str, code: Option<&str>) {
        let _ = (tenant, topic, code);
    }

    fn record_dead_replay(&self, tenant: &TenantId, kind: DeadKind) {
        let _ = (tenant, kind);
    }

    fn record_idempotency(&self, tenant: &TenantId, status: IdempoStatus) {
        let _ = (tenant, status);
    }

    fn record_saga_state(&self, tenant: &TenantId, def: &str, state: SagaState) {
        let _ = (tenant, def, state);
    }
}

#[derive(Default)]
pub struct NoopTxMetrics;

impl TxMetrics for NoopTxMetrics {}
