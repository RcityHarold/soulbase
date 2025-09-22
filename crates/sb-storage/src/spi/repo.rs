use async_trait::async_trait;

use crate::errors::StorageResult;
use crate::model::{Entity, Page, Sort};
use sb_types::prelude::TenantId;

#[async_trait]
pub trait Repository<T: Entity>: Send + Sync {
    async fn get(&self, tenant: &TenantId, id: &str) -> StorageResult<Option<T>>;
    async fn create(&self, tenant: &TenantId, doc: &T) -> StorageResult<T>;
    async fn upsert(
        &self,
        tenant: &TenantId,
        id: &str,
        patch: serde_json::Value,
        expected_version: Option<u64>,
    ) -> StorageResult<T>;
    async fn delete(&self, tenant: &TenantId, id: &str) -> StorageResult<()>;
    async fn select(
        &self,
        tenant: &TenantId,
        filter: serde_json::Value,
        sorts: Option<Vec<Sort>>,
        limit: usize,
        cursor: Option<String>,
    ) -> StorageResult<Page<T>>;
}
