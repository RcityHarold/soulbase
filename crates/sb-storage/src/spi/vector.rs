use async_trait::async_trait;

use crate::errors::StorageResult;
use crate::model::Entity;
use crate::spi::repo::Repository;
use sb_types::prelude::TenantId;

#[async_trait]
pub trait VectorIndex: Send + Sync {
    async fn upsert_vec(&self, tenant: &TenantId, id: &str, embedding: &[f32])
        -> StorageResult<()>;
    async fn remove_vec(&self, tenant: &TenantId, id: &str) -> StorageResult<()>;
    async fn knn<T: Entity + Send + Sync>(
        &self,
        tenant: &TenantId,
        query: &[f32],
        k: usize,
        repo: Option<&dyn Repository<T>>,
    ) -> StorageResult<Vec<(T, f32)>>;
}
