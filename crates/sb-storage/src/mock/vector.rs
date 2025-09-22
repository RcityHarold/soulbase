use std::sync::Arc;

use async_trait::async_trait;

use crate::errors::{StorageError, StorageResult};
use crate::model::Entity;
use crate::spi::repo::Repository;
use crate::spi::vector::VectorIndex;
use sb_types::prelude::TenantId;

use super::datastore::{MockDatastore, MockState};

pub struct InMemoryVector {
    state: Arc<MockState>,
}

impl InMemoryVector {
    pub fn new(ds: &MockDatastore) -> Self {
        Self { state: ds.state() }
    }

    fn fetch_entity<T: Entity>(&self, tenant: &TenantId, id: &str) -> StorageResult<T> {
        let tables = self.state.tables.read();
        let (table, _) = match id.split_once(':') {
            Some(pair) => pair,
            None => return Err(StorageError::schema("invalid id format")),
        };
        let table_map = tables
            .get(table)
            .ok_or_else(|| StorageError::not_found("vector target not found"))?;
        let value = table_map
            .get(id)
            .ok_or_else(|| StorageError::not_found("vector target not found"))?;
        if value.get("tenant") != Some(&serde_json::Value::String(tenant.0.clone())) {
            return Err(StorageError::schema("tenant mismatch"));
        }
        serde_json::from_value(value.clone())
            .map_err(|err| StorageError::schema(format!("deserialize entity failed: {err}")))
    }
}

#[async_trait]
impl VectorIndex for InMemoryVector {
    async fn upsert_vec(
        &self,
        tenant: &TenantId,
        id: &str,
        embedding: &[f32],
    ) -> StorageResult<()> {
        let mut vectors = self.state.vectors.write();
        vectors.insert(format!("{}::{tenant}", id), embedding.to_vec());
        Ok(())
    }

    async fn remove_vec(&self, _tenant: &TenantId, id: &str) -> StorageResult<()> {
        let mut vectors = self.state.vectors.write();
        vectors.retain(|key, _| !key.starts_with(id));
        Ok(())
    }

    async fn knn<T: Entity + Send + Sync>(
        &self,
        tenant: &TenantId,
        query: &[f32],
        k: usize,
        repo: Option<&dyn Repository<T>>,
    ) -> StorageResult<Vec<(T, f32)>> {
        let scored: Vec<(String, f32)> = {
            let vectors = self.state.vectors.read();
            let mut scored: Vec<(String, f32)> = Vec::new();
            for (key, embedding) in vectors.iter() {
                if !key.ends_with(&format!("::{tenant}")) {
                    continue;
                }
                let dist = l2_distance(query, embedding);
                let id = key.split("::").next().unwrap_or("").to_string();
                scored.push((id, dist));
            }
            scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            scored.truncate(k);
            scored
        };

        let mut results = Vec::new();
        for (id, score) in scored {
            let entity = if let Some(repo) = repo {
                repo.get(tenant, &id)
                    .await?
                    .ok_or_else(|| StorageError::not_found("entity not found"))?
            } else {
                self.fetch_entity::<T>(tenant, &id)?
            };
            results.push((entity, score));
        }
        Ok(results)
    }
}

fn l2_distance(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    let mut sum = 0.0f32;
    for i in 0..len {
        let diff = a[i] - b[i];
        sum += diff * diff;
    }
    sum.sqrt()
}
