use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::Value;

use crate::errors::{StorageError, StorageResult};
use crate::observe::{NoopStorageMetrics, StorageMetrics};
use crate::spi::health::HealthInfo;
use crate::spi::query::NamedArgs;
use crate::spi::{Datastore, QueryResult, Session, Tx};

#[derive(Clone)]
pub struct MockDatastore {
    pub(crate) state: Arc<MockState>,
    metrics: Arc<dyn StorageMetrics>,
}

impl MockDatastore {
    pub fn new() -> Self {
        let metrics: Arc<dyn StorageMetrics> = Arc::new(NoopStorageMetrics::default());
        Self {
            state: Arc::new(MockState::new()),
            metrics,
        }
    }

    pub fn with_metrics(metrics: Arc<dyn StorageMetrics>) -> Self {
        Self {
            state: Arc::new(MockState::new()),
            metrics,
        }
    }

    pub(crate) fn state(&self) -> Arc<MockState> {
        Arc::clone(&self.state)
    }
}

#[async_trait]
impl Datastore for MockDatastore {
    async fn session(&self) -> StorageResult<Box<dyn Session>> {
        Ok(Box::new(MockSession {
            state: self.state(),
        }))
    }

    async fn health(&self) -> StorageResult<HealthInfo> {
        Ok(HealthInfo::healthy())
    }

    fn metrics(&self) -> &dyn StorageMetrics {
        self.metrics.as_ref()
    }
}

pub struct MockSession {
    state: Arc<MockState>,
}

#[async_trait]
impl Session for MockSession {
    async fn begin(&mut self) -> StorageResult<Box<dyn Tx>> {
        Ok(Box::new(MockTx {
            state: Arc::clone(&self.state),
        }))
    }

    async fn query(&mut self, _statement: &str, _params: &NamedArgs) -> StorageResult<QueryResult> {
        Err(StorageError::unknown(
            "mock session query is not implemented",
        ))
    }

    async fn query_json(
        &mut self,
        _statement: &str,
        _params: &NamedArgs,
    ) -> StorageResult<Option<Value>> {
        Ok(None)
    }
}

pub struct MockTx {
    state: Arc<MockState>,
}

#[async_trait]
impl Tx for MockTx {
    async fn execute(
        &mut self,
        _statement: &str,
        _params: &NamedArgs,
    ) -> StorageResult<QueryResult> {
        Err(StorageError::unknown("mock tx execute is not implemented"))
    }

    async fn commit(self: Box<Self>) -> StorageResult<()> {
        let _ = self.state; // consume
        Ok(())
    }

    async fn rollback(self: Box<Self>) -> StorageResult<()> {
        let _ = self.state;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct EdgeRecord {
    pub tenant: String,
    pub from: String,
    pub edge: String,
    pub to: String,
    pub _properties: Value,
}

#[derive(Default)]
pub(crate) struct MockState {
    pub tables: RwLock<HashMap<String, HashMap<String, Value>>>,
    pub edges: RwLock<Vec<EdgeRecord>>,
    pub vectors: RwLock<HashMap<String, Vec<f32>>>,
    pub migrations: RwLock<Vec<String>>,
}

impl MockState {
    fn new() -> Self {
        Self::default()
    }
}
