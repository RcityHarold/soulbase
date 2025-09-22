use async_trait::async_trait;

use crate::errors::StorageResult;
use crate::model::Entity;
use sb_types::prelude::TenantId;

#[async_trait]
pub trait Graph: Send + Sync {
    async fn relate(
        &self,
        tenant: &TenantId,
        from: &str,
        edge: &str,
        to: &str,
        properties: serde_json::Value,
    ) -> StorageResult<()>;

    async fn out<T: Entity + Send + Sync>(
        &self,
        tenant: &TenantId,
        from: &str,
        edge: &str,
        limit: usize,
    ) -> StorageResult<Vec<T>>;

    async fn r#in<T: Entity + Send + Sync>(
        &self,
        tenant: &TenantId,
        to: &str,
        edge: &str,
        limit: usize,
    ) -> StorageResult<Vec<T>>;
}
