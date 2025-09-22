use async_trait::async_trait;

use crate::errors::StorageResult;
use crate::spi::migrate::{MigrationExecutor, MigrationScript, Migrator};

use super::datastore::MockDatastore;

pub struct InMemoryMigrator {
    datastore: MockDatastore,
}

impl InMemoryMigrator {
    pub fn new() -> Self {
        Self {
            datastore: MockDatastore::new(),
        }
    }

    pub fn from_datastore(ds: &MockDatastore) -> Self {
        Self {
            datastore: ds.clone(),
        }
    }
}

#[async_trait]
impl Migrator for InMemoryMigrator {
    async fn current_version(&self) -> StorageResult<String> {
        let migrations = self.datastore.state.migrations.read();
        Ok(migrations.last().cloned().unwrap_or_else(|| "none".into()))
    }

    async fn applied_versions(&self) -> StorageResult<Vec<String>> {
        let migrations = self.datastore.state.migrations.read();
        Ok(migrations.clone())
    }
}

#[async_trait]
impl MigrationExecutor for InMemoryMigrator {
    async fn apply_up(&self, scripts: &[MigrationScript]) -> StorageResult<()> {
        let mut migrations = self.datastore.state.migrations.write();
        for script in scripts {
            if !migrations.contains(&script.version) {
                migrations.push(script.version.clone());
            }
        }
        Ok(())
    }

    async fn apply_down(&self, scripts: &[MigrationScript]) -> StorageResult<()> {
        let mut migrations = self.datastore.state.migrations.write();
        for script in scripts {
            migrations.retain(|v| v != &script.version);
        }
        Ok(())
    }
}
