#![cfg(feature = "surreal")]

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serde_json::Value;

use crate::errors::{StorageError, StorageResult};
use crate::model::{Entity, Page};
use crate::observe::StorageMetrics;
use crate::spi::metrics_labels;
use crate::spi::query::NamedArgs;
use crate::spi::search::Search;
use crate::surreal::binder::bind_tenant;
use crate::surreal::datastore::SurrealDatastore;
use crate::surreal::errors::map_surreal_error;
use sb_types::prelude::TenantId;

pub struct SurrealSearch {
    datastore: SurrealDatastore,
    metrics: Arc<dyn StorageMetrics>,
}

impl SurrealSearch {
    pub fn new(datastore: SurrealDatastore) -> Self {
        let metrics = datastore.metrics();
        Self { datastore, metrics }
    }
}

#[async_trait]
impl Search for SurrealSearch {
    async fn search<T: Entity + Send + Sync>(
        &self,
        tenant: &TenantId,
        query: &str,
        limit: usize,
        _cursor: Option<String>,
    ) -> StorageResult<Page<T>> {
        if query.trim().is_empty() {
            return Ok(Page::empty());
        }

        let mut params = NamedArgs::default();
        params.insert("tenant".into(), Value::String(tenant.0.clone()));
        params.insert(
            "limit".into(),
            Value::Number(serde_json::Number::from(limit as u64)),
        );
        params.insert("query".into(), Value::String(query.to_string()));
        params.insert("__kind".into(), Value::String("search".into()));

        let args = bind_tenant(params, tenant)?;
        let stmt = format!(
            "SELECT *, SEARCH::SCORE({table}, $query) AS score FROM {table} \
             WHERE tenant = $tenant AND SEARCH::SCORE({table}, $query) > 0 \
             ORDER BY score DESC LIMIT $limit",
            table = T::TABLE
        );

        let start = Instant::now();
        let mut response = self.datastore.pool().run_raw(&stmt, &args).await?;
        let latency = start.elapsed();
        let rows: Vec<serde_json::Value> = response.take(0).map_err(map_surreal_error)?;
        self.metrics.record_request(
            metrics_labels(tenant, T::TABLE, "search", None),
            rows.len() as u64,
            0,
            latency,
        );
        let mut items = Vec::new();
        for mut row in rows.into_iter() {
            if let Some(obj) = row.as_object_mut() {
                obj.remove("score");
            }
            let entity: T = serde_json::from_value(row)
                .map_err(|err| StorageError::schema(format!("deserialize entity failed: {err}")))?;
            items.push(entity);
        }
        Ok(Page { items, next: None })
    }
}
