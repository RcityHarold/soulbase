use async_trait::async_trait;
use sb_types::prelude::TenantId;

use crate::errors::TxResult;
use crate::model::{IdempoRecord, IdempoStatus};
use crate::util::now_ms;

#[async_trait]
pub trait IdempotencyStore: Send + Sync {
    async fn check_and_put(
        &self,
        tenant: &TenantId,
        key: &str,
        hash: &str,
        ttl_ms: u64,
    ) -> TxResult<Option<String>>;

    async fn finish(&self, tenant: &TenantId, key: &str, result_digest: &str) -> TxResult<()>;

    async fn fail(&self, tenant: &TenantId, key: &str, error: Option<String>) -> TxResult<()>;

    async fn get(&self, tenant: &TenantId, key: &str) -> TxResult<Option<IdempoRecord>>;
}

pub fn build_record(tenant: TenantId, key: &str, hash: &str, ttl_ms: u64) -> IdempoRecord {
    let now = now_ms();
    IdempoRecord {
        key: key.to_string(),
        tenant,
        hash: hash.to_string(),
        status: IdempoStatus::InFlight,
        result_digest: None,
        last_error: None,
        ttl_ms,
        created_at: now,
        updated_at: now,
    }
}
