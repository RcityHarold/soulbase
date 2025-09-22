use std::collections::HashMap;

use async_trait::async_trait;
use parking_lot::RwLock;
use sb_types::prelude::{Id, TenantId};

use crate::errors::{TxError, TxResult};
use crate::model::{DeadLetter, NewOutboxMessage, OutboxMessage, OutboxStatus};
use crate::outbox::{build_dead_letter, build_outbox_message, select_messages, OutboxStore};

#[derive(Default)]
pub struct InMemoryOutboxStore {
    messages: RwLock<HashMap<(String, String), OutboxMessage>>,
}

impl InMemoryOutboxStore {
    fn key(tenant: &TenantId, id: &Id) -> (String, String) {
        (tenant.as_str().to_owned(), id.as_str().to_owned())
    }

    fn key_raw(tenant: &str, id: &str) -> (String, String) {
        (tenant.to_owned(), id.to_owned())
    }

    pub fn status(&self, tenant: &str, id: &str) -> Option<OutboxStatus> {
        self.messages
            .read()
            .get(&Self::key_raw(tenant, id))
            .map(|m| m.status)
    }
}

#[async_trait]
impl OutboxStore for InMemoryOutboxStore {
    async fn enqueue(&self, message: NewOutboxMessage) -> TxResult<OutboxMessage> {
        let mut guard = self.messages.write();
        let stored = build_outbox_message(message);
        let key = Self::key(&stored.tenant, &stored.id);
        if guard.contains_key(&key) {
            return Err(TxError::conflict("outbox id already exists"));
        }
        guard.insert(key, stored.clone());
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
        let mut guard = self.messages.write();
        let snapshot: Vec<OutboxMessage> = guard.values().cloned().collect();
        let ids = select_messages(
            snapshot.iter(),
            tenant,
            now_ms,
            batch,
            worker_id,
            group_by_key,
        );

        let mut leased = Vec::new();
        for id in ids {
            if let Some(msg) = guard.get_mut(&Self::key(tenant, &id)) {
                msg.status = OutboxStatus::Leased;
                msg.worker = Some(worker_id.to_owned());
                msg.lease_until = Some(now_ms + lease_ms);
                leased.push(msg.clone());
            }
        }

        Ok(leased)
    }

    async fn ack_done(&self, tenant: &TenantId, id: &Id) -> TxResult<()> {
        let mut guard = self.messages.write();
        let Some(msg) = guard.get_mut(&Self::key(tenant, id)) else {
            return Err(TxError::unknown("outbox message missing"));
        };
        msg.status = OutboxStatus::Done;
        msg.worker = None;
        msg.lease_until = None;
        msg.last_error = None;
        Ok(())
    }

    async fn nack_backoff(
        &self,
        tenant: &TenantId,
        id: &Id,
        not_before: i64,
        error: Option<String>,
    ) -> TxResult<()> {
        let mut guard = self.messages.write();
        let Some(msg) = guard.get_mut(&Self::key(tenant, id)) else {
            return Err(TxError::unknown("outbox message missing"));
        };
        msg.status = OutboxStatus::Pending;
        msg.not_before = not_before;
        msg.worker = None;
        msg.lease_until = None;
        msg.last_error = error.clone();
        msg.attempts = msg.attempts.saturating_add(1);
        Ok(())
    }

    async fn dead_letter(
        &self,
        tenant: &TenantId,
        id: &Id,
        error: Option<String>,
    ) -> TxResult<DeadLetter> {
        let mut guard = self.messages.write();
        let Some(msg) = guard.get_mut(&Self::key(tenant, id)) else {
            return Err(TxError::unknown("outbox message missing"));
        };
        msg.status = OutboxStatus::Dead;
        msg.worker = None;
        msg.lease_until = None;
        msg.last_error = error.clone();
        msg.attempts = msg.attempts.saturating_add(1);
        Ok(build_dead_letter(msg, error))
    }

    async fn heartbeat(
        &self,
        tenant: &TenantId,
        id: &Id,
        lease_until: i64,
        worker_id: &str,
    ) -> TxResult<()> {
        let mut guard = self.messages.write();
        let Some(msg) = guard.get_mut(&Self::key(tenant, id)) else {
            return Err(TxError::unknown("outbox message missing"));
        };
        if msg.worker.as_deref() != Some(worker_id) {
            return Err(TxError::conflict("lease owned by another worker"));
        }
        msg.lease_until = Some(lease_until);
        Ok(())
    }

    async fn revive(&self, tenant: &TenantId, id: &Id, at: i64) -> TxResult<()> {
        let mut guard = self.messages.write();
        let Some(msg) = guard.get_mut(&Self::key(tenant, id)) else {
            return Err(TxError::unknown("outbox message missing"));
        };
        msg.status = OutboxStatus::Pending;
        msg.worker = None;
        msg.lease_until = None;
        msg.last_error = None;
        msg.not_before = at;
        msg.attempts = 0;
        Ok(())
    }

    async fn get(&self, tenant: &TenantId, id: &Id) -> TxResult<Option<OutboxMessage>> {
        Ok(self.messages.read().get(&Self::key(tenant, id)).cloned())
    }
}

impl InMemoryOutboxStore {
    pub fn insert_raw(&self, message: OutboxMessage) {
        self.messages
            .write()
            .insert(Self::key(&message.tenant, &message.id), message);
    }

    pub fn all(&self) -> Vec<OutboxMessage> {
        self.messages.read().values().cloned().collect::<Vec<_>>()
    }
}
