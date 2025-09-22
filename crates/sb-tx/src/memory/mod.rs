use std::sync::Arc;

use async_trait::async_trait;
use sb_types::prelude::{Id, TenantId};

use crate::a2a::{A2AHooks, NoopA2AHooks};
use crate::config::TxConfig;
use crate::errors::TxResult;
use crate::idempo::IdempotencyStore;
use crate::model::{
    DeadLetter, DeadLetterPayload, IdempoRecord, IdempoStatus, NewOutboxMessage, OutboxMessage,
    OutboxStatus,
};
use crate::observe::{NoopTxMetrics, TxMetrics};
use crate::outbox::OutboxStore;
use crate::qos::BudgetGuard;
use sb_errors::prelude::codes;

pub mod dead_store;
pub mod idempo_store;
pub mod outbox_store;
pub mod saga_store;

pub use dead_store::InMemoryDeadStore;
pub use idempo_store::InMemoryIdempoStore;
pub use outbox_store::InMemoryOutboxStore;
pub use saga_store::InMemorySagaStore;

#[derive(Clone)]
pub struct InMemoryTxStore {
    pub outbox: Arc<InMemoryOutboxStore>,
    pub idempo: Arc<InMemoryIdempoStore>,
    pub saga: InMemorySagaStore,
    pub dead: Arc<InMemoryDeadStore>,
    pub config: TxConfig,
    pub metrics: Arc<dyn TxMetrics>,
    pub qos: Arc<dyn BudgetGuard>,
    pub a2a: Arc<dyn A2AHooks>,
}

impl Default for InMemoryTxStore {
    fn default() -> Self {
        let config = TxConfig::default();
        let budget = config.build_budget_guard();
        Self::with_config(
            config,
            Arc::new(NoopTxMetrics),
            budget,
            Arc::new(NoopA2AHooks),
        )
    }
}

impl InMemoryTxStore {
    pub fn with_config(
        config: TxConfig,
        metrics: Arc<dyn TxMetrics>,
        qos: Arc<dyn BudgetGuard>,
        a2a: Arc<dyn A2AHooks>,
    ) -> Self {
        let outbox = Arc::new(InMemoryOutboxStore::default());
        let idempo = Arc::new(InMemoryIdempoStore::default());
        let saga = InMemorySagaStore::default();
        let dead = Arc::new(InMemoryDeadStore::new(outbox.clone(), saga.clone()));
        Self {
            outbox,
            idempo,
            saga,
            dead,
            config,
            metrics,
            qos,
            a2a,
        }
    }

    pub fn status(&self, tenant: &str, id: &str) -> Option<OutboxStatus> {
        self.outbox.status(tenant, id)
    }
}

#[async_trait]
impl OutboxStore for InMemoryTxStore {
    async fn enqueue(&self, message: NewOutboxMessage) -> TxResult<OutboxMessage> {
        let stored = self.outbox.enqueue(message).await?;
        self.metrics
            .record_outbox_enqueue(&stored.tenant, &stored.topic);
        self.qos.on_enqueue(&stored)?;
        Ok(stored)
    }

    async fn lease_batch(
        &self,
        tenant: &TenantId,
        now_ms: i64,
        lease_ms: i64,
        batch: usize,
        worker_id: &str,
        group_by_key: bool,
    ) -> TxResult<Vec<OutboxMessage>> {
        let grouping = if self.config.outbox.group_by_dispatch_key {
            group_by_key
        } else {
            false
        };
        self.outbox
            .lease_batch(tenant, now_ms, lease_ms, batch, worker_id, grouping)
            .await
    }

    async fn ack_done(&self, tenant: &TenantId, id: &Id) -> TxResult<()> {
        self.outbox.ack_done(tenant, id).await
    }

    async fn nack_backoff(
        &self,
        tenant: &TenantId,
        id: &Id,
        not_before: i64,
        error: Option<String>,
    ) -> TxResult<()> {
        self.outbox
            .nack_backoff(tenant, id, not_before, error)
            .await
    }

    async fn dead_letter(
        &self,
        tenant: &TenantId,
        id: &Id,
        error: Option<String>,
    ) -> TxResult<DeadLetter> {
        let letter = self.outbox.dead_letter(tenant, id, error).await?;
        if let DeadLetterPayload::Outbox(message) = &letter.payload {
            self.metrics
                .record_outbox_dead_letter(tenant, &message.topic, letter.error.as_deref());
            self.qos
                .on_dead_letter(tenant, message, letter.error.as_deref())?;
            self.a2a
                .on_outbox_dead_letter(tenant, message, letter.error.as_deref())?;
        }
        Ok(letter)
    }

    async fn heartbeat(
        &self,
        tenant: &TenantId,
        id: &Id,
        lease_until: i64,
        worker_id: &str,
    ) -> TxResult<()> {
        self.outbox
            .heartbeat(tenant, id, lease_until, worker_id)
            .await
    }

    async fn revive(&self, tenant: &TenantId, id: &Id, at: i64) -> TxResult<()> {
        self.outbox.revive(tenant, id, at).await
    }

    async fn get(&self, tenant: &TenantId, id: &Id) -> TxResult<Option<OutboxMessage>> {
        self.outbox.get(tenant, id).await
    }
}

#[async_trait]
impl IdempotencyStore for InMemoryTxStore {
    async fn check_and_put(
        &self,
        tenant: &TenantId,
        key: &str,
        hash: &str,
        ttl_ms: u64,
    ) -> TxResult<Option<String>> {
        let result = self.idempo.check_and_put(tenant, key, hash, ttl_ms).await;
        match &result {
            Ok(Some(_)) => self
                .metrics
                .record_idempotency(tenant, IdempoStatus::Succeeded),
            Ok(None) => self
                .metrics
                .record_idempotency(tenant, IdempoStatus::InFlight),
            Err(err) => {
                let code = err.as_public().code;
                if code == codes::TX_IDEMPOTENT_BUSY.0 {
                    self.metrics
                        .record_idempotency(tenant, IdempoStatus::InFlight);
                } else if code == codes::TX_IDEMPOTENT_LAST_FAILED.0 {
                    self.metrics
                        .record_idempotency(tenant, IdempoStatus::Failed);
                }
            }
        }
        result
    }

    async fn finish(&self, tenant: &TenantId, key: &str, result_digest: &str) -> TxResult<()> {
        let res = self.idempo.finish(tenant, key, result_digest).await;
        if res.is_ok() {
            self.metrics
                .record_idempotency(tenant, IdempoStatus::Succeeded);
        }
        res
    }

    async fn fail(&self, tenant: &TenantId, key: &str, error: Option<String>) -> TxResult<()> {
        let res = self.idempo.fail(tenant, key, error).await;
        if res.is_ok() {
            self.metrics
                .record_idempotency(tenant, IdempoStatus::Failed);
        }
        res
    }

    async fn get(&self, tenant: &TenantId, key: &str) -> TxResult<Option<IdempoRecord>> {
        self.idempo.get(tenant, key).await
    }
}
