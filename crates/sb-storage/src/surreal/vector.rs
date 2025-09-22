#![cfg(feature = "surreal")]

use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;

use crate::errors::{StorageError, StorageResult};
use crate::model::Entity;
use crate::observe::StorageMetrics;
use crate::spi::metrics_labels;
use crate::spi::query::NamedArgs;
use crate::spi::repo::Repository;
use crate::spi::vector::VectorIndex;
use crate::surreal::binder::bind_tenant;
use crate::surreal::datastore::SurrealDatastore;
use crate::surreal::errors::map_surreal_error;
use sb_types::prelude::TenantId;

pub struct SurrealVectorIndex {
    datastore: SurrealDatastore,
    metrics: Arc<dyn StorageMetrics>,
}

impl SurrealVectorIndex {
    pub fn new(datastore: SurrealDatastore) -> Self {
        let metrics = datastore.metrics();
        Self { datastore, metrics }
    }
}

#[async_trait]
impl VectorIndex for SurrealVectorIndex {
    async fn upsert_vec(
        &self,
        tenant: &TenantId,
        id: &str,
        embedding: &[f32],
    ) -> StorageResult<()> {
        let mut params = NamedArgs::default();
        params.insert("tenant".into(), Value::String(tenant.0.clone()));
        params.insert("id".into(), Value::String(id.to_string()));
        params.insert("vector".into(), serde_json::json!(embedding));
        params.insert("__kind".into(), Value::String("vector".into()));
        let args = bind_tenant(params, tenant)?;

        let stmt =
            "UPDATE type::thing($id) SET vector = $vector WHERE tenant = $tenant RETURN AFTER";
        let start = Instant::now();
        let _response = self.datastore.pool().run_raw(stmt, &args).await?;
        let latency = start.elapsed();
        self.metrics.record_request(
            metrics_labels(tenant, "vector", "write", None),
            0,
            0,
            latency,
        );
        Ok(())
    }

    async fn remove_vec(&self, tenant: &TenantId, id: &str) -> StorageResult<()> {
        let mut params = NamedArgs::default();
        params.insert("tenant".into(), Value::String(tenant.0.clone()));
        params.insert("id".into(), Value::String(id.to_string()));
        params.insert("__kind".into(), Value::String("vector".into()));
        let args = bind_tenant(params, tenant)?;
        let stmt = "UPDATE type::thing($id) REMOVE vector WHERE tenant = $tenant";
        let start = Instant::now();
        let _response = self.datastore.pool().run_raw(stmt, &args).await?;
        let latency = start.elapsed();
        self.metrics.record_request(
            metrics_labels(tenant, "vector", "write", None),
            0,
            0,
            latency,
        );
        Ok(())
    }

    async fn knn<T: Entity + Send + Sync>(
        &self,
        tenant: &TenantId,
        query: &[f32],
        k: usize,
        repo: Option<&dyn Repository<T>>,
    ) -> StorageResult<Vec<(T, f32)>> {
        if k == 0 || query.is_empty() {
            return Ok(Vec::new());
        }

        let mut params = NamedArgs::default();
        params.insert("tenant".into(), Value::String(tenant.0.clone()));
        params.insert(
            "limit".into(),
            Value::Number(serde_json::Number::from(k as u64)),
        );
        params.insert("query_vec".into(), serde_json::json!(query));
        params.insert("__kind".into(), Value::String("vector".into()));
        let args = bind_tenant(params, tenant)?;

        let stmt = format!(
            "SELECT *, SIMILARITY::COSINE(vector, $query_vec) AS score FROM {} \
             WHERE tenant = $tenant AND vector IS NOT NONE ORDER BY score DESC LIMIT $limit",
            T::TABLE
        );

        let start = Instant::now();
        let mut response = self.datastore.pool().run_raw(&stmt, &args).await?;
        let latency = start.elapsed();
        let rows: Vec<serde_json::Value> = response.take(0).map_err(map_surreal_error)?;
        self.metrics.record_request(
            metrics_labels(tenant, T::TABLE, "vector", None),
            rows.len() as u64,
            0,
            latency,
        );

        let mut results = Vec::new();
        for mut row in rows {
            let score = row.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let record_id = row
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| StorageError::schema("vector query missing id"))?;

            if let Some(repo) = repo {
                if let Some(entity) = repo.get(tenant, record_id).await? {
                    results.push((entity, score));
                }
                continue;
            }

            if let Some(obj) = row.as_object_mut() {
                obj.remove("score");
            }
            let entity: T = serde_json::from_value(row)
                .map_err(|err| StorageError::schema(format!("deserialize entity failed: {err}")))?;
            results.push((entity, score));
        }

        Ok(results)
    }
}
