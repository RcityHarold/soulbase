use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use sb_types::prelude::{Id, TenantId};

use crate::backoff::BackoffPolicy;
use crate::errors::{TxError, TxResult};
use crate::model::{
    DeadKind, DeadLetter, DeadLetterPayload, DeadLetterRef, NewOutboxMessage, OutboxMessage,
    OutboxStatus,
};
use crate::observe::TxMetrics;
use crate::qos::BudgetGuard;
use crate::replay::DeadStore;
use crate::util::now_ms;

#[async_trait]
pub trait OutboxStore: Send + Sync {
    async fn enqueue(&self, message: NewOutboxMessage) -> TxResult<OutboxMessage>;

    async fn lease_batch(
        &self,
        tenant: &TenantId,
        now_ms: i64,
        lease_ms: i64,
        batch: usize,
        worker_id: &str,
        group_by_key: bool,
    ) -> TxResult<Vec<OutboxMessage>>;

    async fn ack_done(&self, tenant: &TenantId, id: &Id) -> TxResult<()>;

    async fn nack_backoff(
        &self,
        tenant: &TenantId,
        id: &Id,
        not_before: i64,
        error: Option<String>,
    ) -> TxResult<()>;

    async fn dead_letter(
        &self,
        tenant: &TenantId,
        id: &Id,
        error: Option<String>,
    ) -> TxResult<DeadLetter>;

    async fn heartbeat(
        &self,
        tenant: &TenantId,
        id: &Id,
        lease_until: i64,
        worker_id: &str,
    ) -> TxResult<()>;

    async fn revive(&self, tenant: &TenantId, id: &Id, at: i64) -> TxResult<()>;

    async fn get(&self, tenant: &TenantId, id: &Id) -> TxResult<Option<OutboxMessage>>;
}

#[async_trait]
pub trait OutboxTransport: Send + Sync {
    async fn send(&self, message: &OutboxMessage) -> Result<(), TxError>;
}

pub struct Dispatcher<T, S>
where
    T: OutboxTransport,
    S: OutboxStore,
{
    pub transport: T,
    pub store: S,
    pub worker_id: String,
    pub max_attempts: u32,
    pub lease_ms: i64,
    pub batch: usize,
    pub backoff: Arc<dyn BackoffPolicy + Send + Sync>,
    pub group_by_dispatch_key: bool,
    pub dead_store: Option<Arc<dyn DeadStore>>,
    pub metrics: Arc<dyn TxMetrics>,
    pub qos: Arc<dyn BudgetGuard>,
}

impl<T, S> Dispatcher<T, S>
where
    T: OutboxTransport,
    S: OutboxStore,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        transport: T,
        store: S,
        worker_id: impl Into<String>,
        max_attempts: u32,
        lease_ms: i64,
        batch: usize,
        backoff: Arc<dyn BackoffPolicy + Send + Sync>,
        group_by_dispatch_key: bool,
        dead_store: Option<Arc<dyn DeadStore>>,
        metrics: Arc<dyn TxMetrics>,
        qos: Arc<dyn BudgetGuard>,
    ) -> Self {
        Self {
            transport,
            store,
            worker_id: worker_id.into(),
            max_attempts,
            lease_ms,
            batch,
            backoff,
            group_by_dispatch_key,
            dead_store,
            metrics,
            qos,
        }
    }

    pub async fn tick(&self, tenant: &TenantId, now_ms: i64) -> TxResult<()> {
        let messages = self
            .store
            .lease_batch(
                tenant,
                now_ms,
                self.lease_ms,
                self.batch,
                &self.worker_id,
                self.group_by_dispatch_key,
            )
            .await?;

        for message in messages {
            let dispatch_start = Instant::now();
            self.qos.on_dispatch_attempt(tenant, &message)?;
            match self.transport.send(&message).await {
                Ok(_) => {
                    self.store.ack_done(tenant, &message.id).await?;
                    self.metrics.record_outbox_dispatch(
                        tenant,
                        &message.topic,
                        message.attempts + 1,
                        true,
                        None,
                        Some(dispatch_start.elapsed()),
                    );
                    self.qos.on_dispatch_result(tenant, &message, true)?;
                }
                Err(err) => {
                    let attempts = message.attempts + 1;
                    if attempts >= self.max_attempts {
                        let code = err.as_public().code.to_string();
                        let err_string = err.to_string();
                        let letter = self
                            .store
                            .dead_letter(tenant, &message.id, Some(err_string))
                            .await?;
                        if let Some(dead) = &self.dead_store {
                            dead.push(letter).await?;
                        }
                        self.metrics.record_outbox_dispatch(
                            tenant,
                            &message.topic,
                            attempts,
                            false,
                            Some(&code),
                            Some(dispatch_start.elapsed()),
                        );
                        self.metrics
                            .record_outbox_dead_letter(tenant, &message.topic, Some(&code));
                        self.qos.on_dispatch_result(tenant, &message, false)?;
                    } else {
                        let next = self.backoff.next_after(now_ms, attempts);
                        let code = err.as_public().code.to_string();
                        let err_string = err.to_string();
                        self.store
                            .nack_backoff(tenant, &message.id, next, Some(err_string))
                            .await?;
                        self.metrics.record_outbox_dispatch(
                            tenant,
                            &message.topic,
                            attempts,
                            false,
                            Some(&code),
                            Some(dispatch_start.elapsed()),
                        );
                        self.qos.on_dispatch_result(tenant, &message, false)?;
                    }
                }
            }
        }

        Ok(())
    }
}

pub fn build_outbox_message(new_msg: NewOutboxMessage) -> OutboxMessage {
    OutboxMessage {
        id: new_msg.id,
        tenant: new_msg.tenant,
        envelope_id: new_msg.envelope_id,
        topic: new_msg.topic,
        payload: new_msg.payload,
        created_at: now_ms(),
        not_before: new_msg.not_before.unwrap_or_else(now_ms),
        attempts: 0,
        status: OutboxStatus::Pending,
        last_error: None,
        dispatch_key: new_msg.dispatch_key,
        lease_until: None,
        worker: None,
    }
}

pub fn select_messages<'a>(
    all: impl Iterator<Item = &'a OutboxMessage>,
    tenant: &TenantId,
    now_ms: i64,
    batch: usize,
    worker_id: &str,
    group_by_key: bool,
) -> Vec<Id> {
    let mut selected = Vec::new();
    let mut seen_keys = HashSet::new();

    for msg in all {
        if &msg.tenant != tenant {
            continue;
        }
        if msg.status == OutboxStatus::Done || msg.status == OutboxStatus::Dead {
            continue;
        }
        if let Some(lease_until) = msg.lease_until {
            if lease_until > now_ms && msg.worker.as_deref() != Some(worker_id) {
                continue;
            }
        }
        if msg.not_before > now_ms {
            continue;
        }
        if group_by_key {
            if let Some(key) = msg.dispatch_key.as_deref() {
                if !seen_keys.insert(key.to_string()) {
                    continue;
                }
            }
        }
        selected.push(msg.id.clone());
        if selected.len() >= batch {
            break;
        }
    }

    selected
}

pub fn build_dead_letter(message: &OutboxMessage, error: Option<String>) -> DeadLetter {
    DeadLetter {
        reference: DeadLetterRef {
            kind: DeadKind::Outbox,
            id: message.id.clone(),
        },
        tenant: message.tenant.clone(),
        error,
        occurred_at: now_ms(),
        payload: DeadLetterPayload::Outbox(message.clone()),
    }
}
