use async_trait::async_trait;

use crate::errors::StorageResult;
use crate::model::{Entity, Page};
use sb_types::prelude::TenantId;

#[async_trait]
pub trait Search: Send + Sync {
    async fn search<T: Entity + Send + Sync>(
        &self,
        tenant: &TenantId,
        query: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> StorageResult<Page<T>>;
}
