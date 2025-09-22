use async_trait::async_trait;

use crate::errors::{StorageError, StorageResult};
use crate::observe::{StorageLabels, StorageMetrics, NOOP_STORAGE_METRICS};
use sb_types::prelude::TenantId;

pub mod graph;
pub mod health;
pub mod migrate;
pub mod query;
pub mod repo;
pub mod search;
pub mod vector;

pub use graph::Graph;
pub use health::HealthInfo;
pub use migrate::{MigrationExecutor, MigrationScript, Migrator};
pub use query::NamedArgs;
pub use repo::Repository;
pub use search::Search;
pub use vector::VectorIndex;

#[async_trait]
pub trait Datastore: Send + Sync {
    async fn session(&self) -> StorageResult<Box<dyn Session>>;
    async fn health(&self) -> StorageResult<HealthInfo> {
        Ok(HealthInfo::healthy())
    }
    fn metrics(&self) -> &dyn StorageMetrics {
        &NOOP_STORAGE_METRICS
    }
}

#[async_trait]
pub trait Session: Send {
    async fn begin(&mut self) -> StorageResult<Box<dyn Tx>>;
    async fn query(&mut self, statement: &str, params: &NamedArgs) -> StorageResult<QueryResult>;
    async fn query_json(
        &mut self,
        statement: &str,
        params: &NamedArgs,
    ) -> StorageResult<Option<serde_json::Value>>;
}

#[async_trait]
pub trait Tx: Send {
    async fn execute(&mut self, statement: &str, params: &NamedArgs) -> StorageResult<QueryResult>;
    async fn commit(self: Box<Self>) -> StorageResult<()>;
    async fn rollback(self: Box<Self>) -> StorageResult<()>;
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueryResult {
    pub rows: u64,
    pub bytes: u64,
    pub meta: serde_json::Value,
}

impl QueryResult {
    pub fn new(rows: u64, bytes: u64) -> Self {
        Self {
            rows,
            bytes,
            meta: serde_json::Value::Null,
        }
    }
}

pub fn tenant_guard(table: &str, tenant: &TenantId, params: &NamedArgs) -> StorageResult<()> {
    match params.get("tenant") {
        Some(value) if value == &serde_json::Value::String(tenant.0.clone()) => Ok(()),
        _ => Err(StorageError::schema(format!(
            "tenant guard failed for table '{table}'"
        ))),
    }
}

pub fn metrics_labels<'a>(
    tenant: &'a TenantId,
    table: &'a str,
    kind: &'a str,
    code: Option<&'a str>,
) -> StorageLabels<'a> {
    StorageLabels {
        tenant: &tenant.0,
        table,
        kind,
        code,
    }
}

pub fn named_args() -> NamedArgs {
    NamedArgs::default()
}
