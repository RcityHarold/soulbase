use async_trait::async_trait;

use sb_types::prelude::TenantId;

use crate::{
    errors::TxResult,
    model::{DeadKind, DeadLetter, DeadLetterRef},
};

#[async_trait]
pub trait DeadStore: Send + Sync {
    async fn push(&self, letter: DeadLetter) -> TxResult<()>;
    async fn list(
        &self,
        tenant: &TenantId,
        kind: Option<DeadKind>,
        limit: usize,
    ) -> TxResult<Vec<DeadLetter>>;
    async fn get(&self, reference: &DeadLetterRef) -> TxResult<Option<DeadLetter>>;
    async fn remove(&self, reference: &DeadLetterRef) -> TxResult<()>;
    async fn replay(&self, reference: &DeadLetterRef) -> TxResult<()>;
    async fn purge_older_than(&self, tenant: &TenantId, before_epoch_ms: i64) -> TxResult<()>;
}
