use async_trait::async_trait;

use crate::errors::StorageResult;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationScript {
    pub version: String,
    pub up_sql: String,
    pub down_sql: String,
    pub checksum: String,
}

#[async_trait]
pub trait Migrator: Send + Sync {
    async fn current_version(&self) -> StorageResult<String>;
    async fn applied_versions(&self) -> StorageResult<Vec<String>>;
}

#[async_trait]
pub trait MigrationExecutor: Migrator {
    async fn apply_up(&self, scripts: &[MigrationScript]) -> StorageResult<()>;
    async fn apply_down(&self, scripts: &[MigrationScript]) -> StorageResult<()>;
}
