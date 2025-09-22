#![cfg(feature = "surreal")]

use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;

use crate::errors::{StorageError, StorageResult};
use crate::model::Entity;
use crate::observe::StorageMetrics;
use crate::spi::graph::Graph;
use crate::spi::metrics_labels;
use crate::spi::query::NamedArgs;
use crate::surreal::binder::bind_tenant;
use crate::surreal::datastore::SurrealDatastore;
use crate::surreal::errors::map_surreal_error;
use sb_types::prelude::TenantId;

pub struct SurrealGraph {
    datastore: SurrealDatastore,
    metrics: Arc<dyn StorageMetrics>,
}

impl SurrealGraph {
    pub fn new(datastore: SurrealDatastore) -> Self {
        let metrics = datastore.metrics();
        Self { datastore, metrics }
    }

    fn validate_edge(&self, edge: &str) -> StorageResult<()> {
        if edge.is_empty()
            || !edge
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(StorageError::schema(format!(
                "invalid graph edge identifier '{edge}'"
            )));
        }
        Ok(())
    }

    fn ensure_properties(&self, tenant: &TenantId, properties: Value) -> StorageResult<Value> {
        let mut map = match properties {
            Value::Object(map) => map,
            Value::Null => serde_json::Map::new(),
            _ => {
                return Err(StorageError::schema(
                    "graph properties must be a JSON object",
                ))
            }
        };
        map.insert("tenant".into(), Value::String(tenant.0.clone()));
        Ok(Value::Object(map))
    }
}

#[async_trait]
impl Graph for SurrealGraph {
    async fn relate(
        &self,
        tenant: &TenantId,
        from: &str,
        edge: &str,
        to: &str,
        properties: Value,
    ) -> StorageResult<()> {
        self.validate_edge(edge)?;
        let mut params = NamedArgs::default();
        params.insert("tenant".into(), Value::String(tenant.0.clone()));
        params.insert("from".into(), Value::String(from.to_string()));
        params.insert("to".into(), Value::String(to.to_string()));
        params.insert("__kind".into(), Value::String("graph".into()));
        params.insert(
            "properties".into(),
            self.ensure_properties(tenant, properties)?,
        );
        let args = bind_tenant(params, tenant)?;

        let stmt =
            format!("RELATE type::thing($from) -> {edge} -> type::thing($to) CONTENT $properties");

        let start = Instant::now();
        let _response = self.datastore.pool().run_raw(&stmt, &args).await?;
        let latency = start.elapsed();
        self.metrics
            .record_request(metrics_labels(tenant, edge, "graph", None), 0, 0, latency);
        Ok(())
    }

    async fn out<T: Entity + Send + Sync>(
        &self,
        tenant: &TenantId,
        from: &str,
        edge: &str,
        limit: usize,
    ) -> StorageResult<Vec<T>> {
        self.validate_edge(edge)?;
        let mut params = NamedArgs::default();
        params.insert("tenant".into(), Value::String(tenant.0.clone()));
        params.insert("from".into(), Value::String(from.to_string()));
        params.insert(
            "limit".into(),
            Value::Number(serde_json::Number::from(limit as u64)),
        );
        params.insert("__kind".into(), Value::String("graph".into()));
        let args = bind_tenant(params, tenant)?;

        let stmt = format!(
            "SELECT out.* FROM type::thing($from) -> {edge} WHERE tenant = $tenant LIMIT $limit"
        );

        let start = Instant::now();
        let mut response = self.datastore.pool().run_raw(&stmt, &args).await?;
        let latency = start.elapsed();
        let items: Vec<T> = response.take(0).map_err(map_surreal_error)?;
        let rows = items.len() as u64;
        self.metrics.record_request(
            metrics_labels(tenant, edge, "graph", None),
            rows,
            0,
            latency,
        );
        Ok(items)
    }

    async fn r#in<T: Entity + Send + Sync>(
        &self,
        tenant: &TenantId,
        to: &str,
        edge: &str,
        limit: usize,
    ) -> StorageResult<Vec<T>> {
        self.validate_edge(edge)?;
        let mut params = NamedArgs::default();
        params.insert("tenant".into(), Value::String(tenant.0.clone()));
        params.insert("to".into(), Value::String(to.to_string()));
        params.insert(
            "limit".into(),
            Value::Number(serde_json::Number::from(limit as u64)),
        );
        params.insert("__kind".into(), Value::String("graph".into()));
        let args = bind_tenant(params, tenant)?;

        let stmt = format!(
            "SELECT in.* FROM type::thing($to) <- {edge} WHERE tenant = $tenant LIMIT $limit"
        );

        let start = Instant::now();
        let mut response = self.datastore.pool().run_raw(&stmt, &args).await?;
        let latency = start.elapsed();
        let items: Vec<T> = response.take(0).map_err(map_surreal_error)?;
        let rows = items.len() as u64;
        self.metrics.record_request(
            metrics_labels(tenant, edge, "graph", None),
            rows,
            0,
            latency,
        );
        Ok(items)
    }
}
