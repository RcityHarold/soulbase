#![cfg(feature = "surreal")]

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serde_json::Value;

use crate::errors::{StorageError, StorageResult};
use crate::model::{Entity, Page, Sort};
use crate::observe::StorageMetrics;
use crate::spi::metrics_labels;
use crate::spi::query::NamedArgs;
use crate::spi::repo::Repository;
use crate::surreal::binder::bind_tenant;
use crate::surreal::datastore::SurrealDatastore;
use crate::surreal::errors::map_surreal_error;
use crate::surreal::mapper::{append_where, build_filter_clause, build_sort_clause};
use crate::{named, spi::Datastore};
use sb_types::prelude::TenantId;

pub struct SurrealRepository<T: Entity> {
    datastore: SurrealDatastore,
    metrics: Arc<dyn StorageMetrics>,
    _marker: PhantomData<T>,
}

impl<T: Entity> SurrealRepository<T> {
    pub fn new(datastore: SurrealDatastore) -> Self {
        let metrics = datastore.metrics();
        Self {
            datastore,
            metrics,
            _marker: PhantomData,
        }
    }

    fn base_params(tenant: &TenantId) -> NamedArgs {
        named! {
            "tenant" => tenant.as_ref(),
            "table" => T::TABLE,
        }
    }
}

#[async_trait]
impl<T: Entity> Repository<T> for SurrealRepository<T> {
    async fn get(&self, tenant: &TenantId, id: &str) -> StorageResult<Option<T>> {
        let mut params = Self::base_params(tenant);
        params.insert("id".into(), Value::String(id.to_string()));
        params.insert("__kind".into(), Value::String("read".into()));

        let mut session = self.datastore.session().await?;
        let raw = session
            .query_json(
                "SELECT * FROM type::thing($table, $id) WHERE tenant = $tenant LIMIT 1",
                &params,
            )
            .await?;

        match raw {
            Some(value) => serde_json::from_value(value)
                .map(Some)
                .map_err(|err| StorageError::schema(format!("deserialize entity failed: {err}"))),
            None => Ok(None),
        }
    }

    async fn create(&self, tenant: &TenantId, doc: &T) -> StorageResult<T> {
        let mut params = Self::base_params(tenant);
        params.insert("id".into(), Value::String(doc.id().to_string()));
        params.insert("__kind".into(), Value::String("write".into()));
        params.insert(
            "data".into(),
            serde_json::to_value(doc)
                .map_err(|err| StorageError::schema(format!("serialize entity failed: {err}")))?,
        );

        let mut session = self.datastore.session().await?;
        let raw = session
            .query_json(
                "CREATE type::thing($table, $id) CONTENT $data RETURN AFTER",
                &params,
            )
            .await?
            .ok_or_else(|| StorageError::unknown("failed to create record"))?;

        serde_json::from_value(raw)
            .map_err(|err| StorageError::schema(format!("deserialize entity failed: {err}")))
    }

    async fn upsert(
        &self,
        tenant: &TenantId,
        id: &str,
        patch: Value,
        expected_version: Option<u64>,
    ) -> StorageResult<T> {
        let mut params = Self::base_params(tenant);
        params.insert("id".into(), Value::String(id.to_string()));
        params.insert("patch".into(), patch);
        params.insert("__kind".into(), Value::String("write".into()));
        if let Some(ver) = expected_version {
            params.insert(
                "expected_ver".into(),
                Value::Number(serde_json::Number::from(ver)),
            );
        }

        let stmt = if expected_version.is_some() {
            "UPDATE type::thing($table, $id) MERGE $patch WHERE tenant = $tenant AND ver = $expected_ver RETURN AFTER"
        } else {
            "UPDATE type::thing($table, $id) MERGE $patch WHERE tenant = $tenant RETURN AFTER"
        };

        let mut session = self.datastore.session().await?;
        let raw = session
            .query_json(stmt, &params)
            .await?
            .ok_or_else(|| StorageError::not_found("record not found"))?;

        serde_json::from_value(raw)
            .map_err(|err| StorageError::schema(format!("deserialize entity failed: {err}")))
    }

    async fn delete(&self, tenant: &TenantId, id: &str) -> StorageResult<()> {
        let mut params = Self::base_params(tenant);
        params.insert("id".into(), Value::String(id.to_string()));
        params.insert("__kind".into(), Value::String("write".into()));
        let mut session = self.datastore.session().await?;
        session
            .query(
                "DELETE type::thing($table, $id) WHERE tenant = $tenant",
                &params,
            )
            .await?;
        Ok(())
    }

    async fn select(
        &self,
        tenant: &TenantId,
        filter: Value,
        sorts: Option<Vec<Sort>>,
        limit: usize,
        cursor: Option<String>,
    ) -> StorageResult<Page<T>> {
        let mut params = Self::base_params(tenant);
        params.insert("__kind".into(), Value::String("read".into()));
        params.insert(
            "limit".into(),
            Value::Number(serde_json::Number::from(limit as u64)),
        );

        let mut where_clause = String::from("tenant = $tenant");
        let filter_clause = build_filter_clause(&filter, &mut params, "select")?;
        if !filter_clause.is_empty() {
            append_where(&mut where_clause, &filter_clause);
        }
        if let Some(cursor) = cursor {
            params.insert("cursor".into(), Value::String(cursor));
            append_where(&mut where_clause, "id > $cursor");
        }
        let order_clause = if let Some(sorts) = sorts.as_ref() {
            build_sort_clause(sorts)?
        } else {
            String::new()
        };

        let statement = format!(
            "SELECT * FROM {} WHERE {}{} LIMIT $limit",
            T::TABLE,
            where_clause,
            order_clause
        );

        let args = bind_tenant(params.clone(), tenant)?;
        let start = Instant::now();
        let mut response = self.datastore.pool().run_raw(&statement, &args).await?;
        let latency = start.elapsed();
        let items: Vec<T> = response.take(0).map_err(map_surreal_error)?;
        let rows = items.len() as u64;
        self.metrics.record_request(
            metrics_labels(tenant, T::TABLE, "read", None),
            rows,
            0,
            latency,
        );
        let next = if items.len() == limit && limit > 0 {
            items.last().map(|item| item.id().to_string())
        } else {
            None
        };
        Ok(Page { items, next })
    }
}
