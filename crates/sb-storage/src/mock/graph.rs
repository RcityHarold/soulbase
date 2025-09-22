use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::errors::{StorageError, StorageResult};
use crate::model::Entity;
use crate::spi::graph::Graph;
use sb_types::prelude::TenantId;

use super::datastore::{EdgeRecord, MockDatastore, MockState};

pub struct InMemoryGraph {
    state: Arc<MockState>,
}

impl InMemoryGraph {
    pub fn new(ds: &MockDatastore) -> Self {
        Self { state: ds.state() }
    }

    fn split_id(id: &str) -> (&str, &str) {
        match id.split_once(':') {
            Some(pair) => pair,
            None => ("", id),
        }
    }

    fn fetch_entity<T: Entity>(state: &MockState, tenant: &TenantId, id: &str) -> StorageResult<T> {
        let (table, _) = Self::split_id(id);
        if table.is_empty() {
            return Err(StorageError::schema("invalid id format"));
        }
        let tables = state.tables.read();
        let table_map = tables
            .get(table)
            .ok_or_else(|| StorageError::not_found("record table not found"))?;
        let value = table_map
            .get(id)
            .ok_or_else(|| StorageError::not_found("record not found"))?;
        if value.get("tenant") != Some(&Value::String(tenant.0.clone())) {
            return Err(StorageError::schema("tenant mismatch in graph"));
        }
        serde_json::from_value(value.clone())
            .map_err(|err| StorageError::schema(format!("deserialize entity failed: {err}")))
    }
}

#[async_trait]
impl Graph for InMemoryGraph {
    async fn relate(
        &self,
        tenant: &TenantId,
        from: &str,
        edge: &str,
        to: &str,
        properties: Value,
    ) -> StorageResult<()> {
        // validate both nodes exist and belong to tenant
        let _from: Value;
        {
            let tables = self.state.tables.read();
            let (table, _) = Self::split_id(from);
            let table_map = tables
                .get(table)
                .ok_or_else(|| StorageError::not_found("source node not found"))?;
            let value = table_map
                .get(from)
                .ok_or_else(|| StorageError::not_found("source node not found"))?;
            if value.get("tenant") != Some(&Value::String(tenant.0.clone())) {
                return Err(StorageError::schema("tenant mismatch for source"));
            }
            _from = value.clone();
        }

        // ensure destination exists and tenant matches
        {
            let tables = self.state.tables.read();
            let (table, _) = Self::split_id(to);
            let table_map = tables
                .get(table)
                .ok_or_else(|| StorageError::not_found("target node not found"))?;
            let value = table_map
                .get(to)
                .ok_or_else(|| StorageError::not_found("target node not found"))?;
            if value.get("tenant") != Some(&Value::String(tenant.0.clone())) {
                return Err(StorageError::schema("tenant mismatch for target"));
            }
        }

        self.state.edges.write().push(EdgeRecord {
            tenant: tenant.0.clone(),
            from: from.to_string(),
            edge: edge.to_string(),
            to: to.to_string(),
            _properties: properties,
        });
        Ok(())
    }

    async fn out<T: Entity + Send + Sync>(
        &self,
        tenant: &TenantId,
        from: &str,
        edge: &str,
        limit: usize,
    ) -> StorageResult<Vec<T>> {
        let edges = self.state.edges.read();
        let mut results = Vec::new();
        for record in edges
            .iter()
            .filter(|e| e.tenant == tenant.0 && e.from == from && e.edge == edge)
        {
            let entity = Self::fetch_entity::<T>(&self.state, tenant, &record.to)?;
            results.push(entity);
            if results.len() >= limit {
                break;
            }
        }
        Ok(results)
    }

    async fn r#in<T: Entity + Send + Sync>(
        &self,
        tenant: &TenantId,
        to: &str,
        edge: &str,
        limit: usize,
    ) -> StorageResult<Vec<T>> {
        let edges = self.state.edges.read();
        let mut results = Vec::new();
        for record in edges
            .iter()
            .filter(|e| e.tenant == tenant.0 && e.to == to && e.edge == edge)
        {
            let entity = Self::fetch_entity::<T>(&self.state, tenant, &record.from)?;
            results.push(entity);
            if results.len() >= limit {
                break;
            }
        }
        Ok(results)
    }
}
