use std::collections::HashMap;

use async_trait::async_trait;
use parking_lot::RwLock;
use sb_types::prelude::TenantId;

use crate::errors::{TxError, TxResult};
use crate::model::{DeadKind, DeadLetter, DeadLetterPayload, DeadLetterRef, SagaState};
use crate::outbox::OutboxStore;
use crate::replay::DeadStore;
use crate::saga::SagaStore;
use crate::util::now_ms;

use super::outbox_store::InMemoryOutboxStore;
use super::saga_store::InMemorySagaStore;

#[derive(Clone)]
pub struct InMemoryDeadStore {
    outbox: std::sync::Arc<InMemoryOutboxStore>,
    saga: InMemorySagaStore,
    entries: std::sync::Arc<RwLock<HashMap<(DeadKind, String), DeadLetter>>>,
}

impl InMemoryDeadStore {
    pub fn new(outbox: std::sync::Arc<InMemoryOutboxStore>, saga: InMemorySagaStore) -> Self {
        Self {
            outbox,
            saga,
            entries: std::sync::Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn key(reference: &DeadLetterRef) -> (DeadKind, String) {
        (reference.kind, reference.id.as_str().to_owned())
    }
}

#[async_trait]
impl DeadStore for InMemoryDeadStore {
    async fn push(&self, letter: DeadLetter) -> TxResult<()> {
        self.entries
            .write()
            .insert(Self::key(&letter.reference), letter);
        Ok(())
    }

    async fn list(
        &self,
        tenant: &TenantId,
        kind: Option<DeadKind>,
        limit: usize,
    ) -> TxResult<Vec<DeadLetter>> {
        let mut out = Vec::new();
        for letter in self.entries.read().values() {
            if &letter.tenant != tenant {
                continue;
            }
            if let Some(kind) = kind {
                if letter.kind() != kind {
                    continue;
                }
            }
            out.push(letter.clone());
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    async fn get(&self, reference: &DeadLetterRef) -> TxResult<Option<DeadLetter>> {
        Ok(self.entries.read().get(&Self::key(reference)).cloned())
    }

    async fn remove(&self, reference: &DeadLetterRef) -> TxResult<()> {
        self.entries.write().remove(&Self::key(reference));
        Ok(())
    }

    async fn replay(&self, reference: &DeadLetterRef) -> TxResult<()> {
        let letter = {
            let guard = self.entries.read();
            guard
                .get(&Self::key(reference))
                .cloned()
                .ok_or_else(|| TxError::unknown("dead letter not found"))?
        };

        let reference = letter.reference.clone();
        match letter.payload {
            DeadLetterPayload::Outbox(_) => {
                self.outbox
                    .revive(&letter.tenant, &reference.id, now_ms())
                    .await?;
            }
            DeadLetterPayload::Saga(mut saga) => {
                saga.state = SagaState::Running;
                saga.updated_at = now_ms();
                self.saga.save(&saga).await?;
            }
        }

        self.remove(&reference).await
    }

    async fn purge_older_than(&self, tenant: &TenantId, before_epoch_ms: i64) -> TxResult<()> {
        let mut guard = self.entries.write();
        let keys: Vec<_> = guard
            .iter()
            .filter_map(|(key, letter)| {
                if &letter.tenant == tenant && letter.occurred_at < before_epoch_ms {
                    Some(key.clone())
                } else {
                    None
                }
            })
            .collect();
        for key in keys {
            guard.remove(&key);
        }
        Ok(())
    }
}
