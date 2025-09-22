use async_trait::async_trait;

use crate::errors::{StorageError, StorageResult};
use crate::model::{Entity, Page};
use crate::spi::search::Search;
use sb_types::prelude::TenantId;

use super::datastore::MockDatastore;

pub struct InMemorySearch {
    datastore: MockDatastore,
}

impl InMemorySearch {
    pub fn new(datastore: &MockDatastore) -> Self {
        Self {
            datastore: datastore.clone(),
        }
    }
}

#[async_trait]
impl Search for InMemorySearch {
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

        let mut matches = Vec::new();
        let tables = self.datastore.state.tables.read();
        if let Some(table) = tables.get(T::TABLE) {
            for value in table.values() {
                if value.get("tenant") != Some(&serde_json::Value::String(tenant.0.clone())) {
                    continue;
                }
                let text = value
                    .get("title")
                    .or_else(|| value.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if text.contains(query) {
                    let entity = serde_json::from_value(value.clone()).map_err(|err| {
                        StorageError::schema(format!("deserialize entity failed: {err}"))
                    })?;
                    matches.push(entity);
                }
            }
        }
        if matches.len() > limit {
            matches.truncate(limit);
        }
        Ok(Page {
            items: matches,
            next: None,
        })
    }
}
