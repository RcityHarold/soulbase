use std::collections::HashMap;

use async_trait::async_trait;
use parking_lot::RwLock;
use sb_types::prelude::TenantId;

use crate::errors::{TxError, TxResult};
use crate::idempo::{build_record, IdempotencyStore};
use crate::model::{IdempoRecord, IdempoStatus};
use crate::util::now_ms;

#[derive(Default)]
pub struct InMemoryIdempoStore {
    records: RwLock<HashMap<(String, String), IdempoRecord>>,
}

impl InMemoryIdempoStore {
    fn key(tenant: &TenantId, key: &str) -> (String, String) {
        (tenant.as_str().to_owned(), key.to_owned())
    }
}

#[async_trait]
impl IdempotencyStore for InMemoryIdempoStore {
    async fn check_and_put(
        &self,
        tenant: &TenantId,
        key: &str,
        hash: &str,
        ttl_ms: u64,
    ) -> TxResult<Option<String>> {
        let mut guard = self.records.write();
        let now = now_ms();
        let map_key = Self::key(tenant, key);
        if let Some(existing) = guard.get(&map_key) {
            if now.saturating_sub(existing.updated_at) as u64 > existing.ttl_ms {
                guard.remove(&map_key);
            }
        }

        if let Some(existing) = guard.get(&map_key) {
            if existing.hash != hash {
                return Err(TxError::conflict("idempotency key hash mismatch"));
            }
            return match existing.status {
                IdempoStatus::InFlight => Err(TxError::idempo_busy()),
                IdempoStatus::Succeeded => Ok(existing.result_digest.clone()),
                IdempoStatus::Failed => Err(TxError::idempo_failed()),
            };
        }

        let record = build_record(tenant.clone(), key, hash, ttl_ms);
        guard.insert(map_key, record);
        Ok(None)
    }

    async fn finish(&self, tenant: &TenantId, key: &str, result_digest: &str) -> TxResult<()> {
        let mut guard = self.records.write();
        let Some(existing) = guard.get_mut(&Self::key(tenant, key)) else {
            return Err(TxError::unknown("idempotency record missing"));
        };
        existing.status = IdempoStatus::Succeeded;
        existing.result_digest = Some(result_digest.to_owned());
        existing.last_error = None;
        existing.updated_at = now_ms();
        Ok(())
    }

    async fn fail(&self, tenant: &TenantId, key: &str, error: Option<String>) -> TxResult<()> {
        let mut guard = self.records.write();
        let Some(existing) = guard.get_mut(&Self::key(tenant, key)) else {
            return Err(TxError::unknown("idempotency record missing"));
        };
        existing.status = IdempoStatus::Failed;
        existing.last_error = error;
        existing.updated_at = now_ms();
        Ok(())
    }

    async fn get(&self, tenant: &TenantId, key: &str) -> TxResult<Option<IdempoRecord>> {
        Ok(self.records.read().get(&Self::key(tenant, key)).cloned())
    }
}
