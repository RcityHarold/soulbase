use std::collections::HashMap;

use async_trait::async_trait;
use parking_lot::RwLock;
use sb_types::prelude::Id;

use crate::errors::{TxError, TxResult};
use crate::model::SagaInstance;
use crate::saga::SagaStore;

#[derive(Clone, Default)]
pub struct InMemorySagaStore {
    items: std::sync::Arc<RwLock<HashMap<String, SagaInstance>>>,
}

#[async_trait]
impl SagaStore for InMemorySagaStore {
    async fn insert(&self, saga: SagaInstance) -> TxResult<()> {
        let mut guard = self.items.write();
        let key = saga.id.as_str().to_owned();
        if guard.contains_key(&key) {
            return Err(TxError::conflict("saga already exists"));
        }
        guard.insert(key, saga);
        Ok(())
    }

    async fn load(&self, id: &Id) -> TxResult<Option<SagaInstance>> {
        Ok(self.items.read().get(id.as_str()).cloned())
    }

    async fn save(&self, saga: &SagaInstance) -> TxResult<()> {
        let mut guard = self.items.write();
        let key = saga.id.as_str().to_owned();
        if !guard.contains_key(&key) {
            return Err(TxError::unknown("saga instance missing"));
        }
        guard.insert(key, saga.clone());
        Ok(())
    }
}

impl InMemorySagaStore {
    pub fn all(&self) -> Vec<SagaInstance> {
        self.items.read().values().cloned().collect()
    }
}
