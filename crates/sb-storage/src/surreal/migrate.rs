#![cfg(feature = "surreal")]

use async_trait::async_trait;
use std::sync::Arc;
use std::time::Instant;

use crate::errors::StorageResult;
use crate::observe::StorageMetrics;
use crate::spi::metrics_labels;
use crate::spi::migrate::{MigrationExecutor, MigrationScript, Migrator};
use crate::spi::query::NamedArgs;
use crate::surreal::datastore::SurrealDatastore;
use crate::surreal::errors::map_surreal_error;
use sb_types::prelude::TenantId;

const MIGRATION_TABLE: &str = "__sb_migrations";

pub struct SurrealMigrator {
    datastore: SurrealDatastore,
    metrics: Arc<dyn StorageMetrics>,
}

impl SurrealMigrator {
    pub fn new(datastore: SurrealDatastore) -> Self {
        let metrics = datastore.metrics();
        Self { datastore, metrics }
    }

    async fn ensure_table(&self) -> StorageResult<()> {
        let stmt = format!(
            "DEFINE TABLE IF NOT EXISTS {table} SCHEMAFULL;\n\
             DEFINE FIELD version ON {table} TYPE string ASSERT string::len(value) > 0;\n\
             DEFINE FIELD checksum ON {table} TYPE string;\n\
             DEFINE FIELD applied_at ON {table} TYPE datetime;\n\
             DEFINE INDEX idx_{table}_version ON TABLE {table} UNIQUE FIELDS version;",
            table = MIGRATION_TABLE
        );
        let params = NamedArgs::default();
        self.datastore.pool().run_raw(&stmt, &params).await?;
        Ok(())
    }
}

#[async_trait]
impl Migrator for SurrealMigrator {
    async fn current_version(&self) -> StorageResult<String> {
        self.ensure_table().await?;
        let stmt = format!(
            "SELECT version FROM {} ORDER BY applied_at DESC LIMIT 1",
            MIGRATION_TABLE
        );
        let params = NamedArgs::default();
        let mut response = self.datastore.pool().run_raw(&stmt, &params).await?;
        let rows: Vec<serde_json::Value> = response.take(0).map_err(map_surreal_error)?;
        if let Some(row) = rows.first() {
            Ok(row
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("none")
                .to_string())
        } else {
            Ok("none".into())
        }
    }

    async fn applied_versions(&self) -> StorageResult<Vec<String>> {
        self.ensure_table().await?;
        let stmt = format!(
            "SELECT version FROM {} ORDER BY applied_at ASC",
            MIGRATION_TABLE
        );
        let params = NamedArgs::default();
        let mut response = self.datastore.pool().run_raw(&stmt, &params).await?;
        let rows: Vec<serde_json::Value> = response.take(0).map_err(map_surreal_error)?;
        Ok(rows
            .into_iter()
            .filter_map(|row| row.get("version")?.as_str().map(|s| s.to_string()))
            .collect())
    }
}

#[async_trait]
impl MigrationExecutor for SurrealMigrator {
    async fn apply_up(&self, scripts: &[MigrationScript]) -> StorageResult<()> {
        self.ensure_table().await?;
        if scripts.is_empty() {
            return Ok(());
        }

        let start = Instant::now();
        self.datastore
            .pool()
            .run_raw("BEGIN TRANSACTION", &NamedArgs::default())
            .await?;

        for script in scripts {
            let mut params = NamedArgs::default();
            params.insert("__kind".into(), serde_json::Value::String("migrate".into()));
            if let Err(err) = self.datastore.pool().run_raw(&script.up_sql, &params).await {
                let _ = self
                    .datastore
                    .pool()
                    .run_raw("CANCEL TRANSACTION", &NamedArgs::default())
                    .await;
                return Err(err);
            }

            let record_stmt = format!(
                "INSERT INTO {table} (version, checksum, applied_at) VALUES ($version, $checksum, time::now())",
                table = MIGRATION_TABLE
            );
            let mut record_params = NamedArgs::default();
            record_params.insert(
                "version".into(),
                serde_json::Value::String(script.version.clone()),
            );
            record_params.insert(
                "checksum".into(),
                serde_json::Value::String(script.checksum.clone()),
            );
            self.datastore
                .pool()
                .run_raw(&record_stmt, &record_params)
                .await?;
        }

        self.datastore
            .pool()
            .run_raw("COMMIT TRANSACTION", &NamedArgs::default())
            .await?;

        let elapsed = start.elapsed();
        let tenant = TenantId::from("__system__");
        self.metrics.record_request(
            metrics_labels(&tenant, MIGRATION_TABLE, "migrate", None),
            scripts.len() as u64,
            0,
            elapsed,
        );
        Ok(())
    }

    async fn apply_down(&self, scripts: &[MigrationScript]) -> StorageResult<()> {
        self.ensure_table().await?;
        if scripts.is_empty() {
            return Ok(());
        }

        let start = Instant::now();
        self.datastore
            .pool()
            .run_raw("BEGIN TRANSACTION", &NamedArgs::default())
            .await?;

        for script in scripts {
            let mut params = NamedArgs::default();
            params.insert("__kind".into(), serde_json::Value::String("migrate".into()));
            if let Err(err) = self
                .datastore
                .pool()
                .run_raw(&script.down_sql, &params)
                .await
            {
                let _ = self
                    .datastore
                    .pool()
                    .run_raw("CANCEL TRANSACTION", &NamedArgs::default())
                    .await;
                return Err(err);
            }
            let stmt = format!(
                "DELETE {table} WHERE version = $version",
                table = MIGRATION_TABLE
            );
            let mut record_params = NamedArgs::default();
            record_params.insert(
                "version".into(),
                serde_json::Value::String(script.version.clone()),
            );
            self.datastore.pool().run_raw(&stmt, &record_params).await?;
        }

        self.datastore
            .pool()
            .run_raw("COMMIT TRANSACTION", &NamedArgs::default())
            .await?;

        let elapsed = start.elapsed();
        let tenant = TenantId::from("__system__");
        self.metrics.record_request(
            metrics_labels(&tenant, MIGRATION_TABLE, "migrate", None),
            scripts.len() as u64,
            0,
            elapsed,
        );
        Ok(())
    }
}
