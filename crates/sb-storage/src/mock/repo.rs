use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::errors::{StorageError, StorageResult};
use crate::model::{Entity, Page, Sort};
use crate::spi::repo::Repository;
use sb_types::prelude::TenantId;

use super::datastore::{MockDatastore, MockState};

pub struct InMemoryRepository<T: Entity> {
    state: Arc<MockState>,
    _marker: PhantomData<T>,
}

impl<T: Entity> InMemoryRepository<T> {
    pub fn new(ds: &MockDatastore) -> Self {
        Self {
            state: ds.state(),
            _marker: PhantomData,
        }
    }

    fn table<'a>(
        tables: &'a mut HashMap<String, HashMap<String, Value>>,
    ) -> &'a mut HashMap<String, Value> {
        tables
            .entry(T::TABLE.to_string())
            .or_insert_with(HashMap::new)
    }

    fn ensure_tenant(value: &Value, tenant: &TenantId) -> StorageResult<()> {
        match value.get("tenant") {
            Some(Value::String(t)) if t == &tenant.0 => Ok(()),
            _ => Err(StorageError::schema("tenant mismatch")),
        }
    }
}

#[async_trait]
impl<T: Entity> Repository<T> for InMemoryRepository<T> {
    async fn get(&self, tenant: &TenantId, id: &str) -> StorageResult<Option<T>> {
        let tables = self.state.tables.read();
        if let Some(table) = tables.get(T::TABLE) {
            if let Some(value) = table.get(id) {
                Self::ensure_tenant(value, tenant)?;
                let entity = serde_json::from_value(value.clone()).map_err(|err| {
                    StorageError::schema(format!("deserialize entity failed: {err}"))
                })?;
                return Ok(Some(entity));
            }
        }
        Ok(None)
    }

    async fn create(&self, tenant: &TenantId, doc: &T) -> StorageResult<T> {
        let mut tables = self.state.tables.write();
        let table = Self::table(&mut tables);
        if table.contains_key(doc.id()) {
            return Err(StorageError::conflict("record already exists"));
        }
        let value = serde_json::to_value(doc)
            .map_err(|err| StorageError::schema(format!("serialize entity failed: {err}")))?;
        Self::ensure_tenant(&value, tenant)?;
        table.insert(doc.id().to_string(), value.clone());
        let entity = serde_json::from_value(value)
            .map_err(|err| StorageError::schema(format!("deserialize entity failed: {err}")))?;
        Ok(entity)
    }

    async fn upsert(
        &self,
        tenant: &TenantId,
        id: &str,
        patch: Value,
        expected_version: Option<u64>,
    ) -> StorageResult<T> {
        let mut tables = self.state.tables.write();
        let table = Self::table(&mut tables);
        let mut base = table
            .get(id)
            .cloned()
            .ok_or_else(|| StorageError::not_found("record not found"))?;
        Self::ensure_tenant(&base, tenant)?;

        if let Some(ver) = expected_version {
            let current = base
                .get("ver")
                .and_then(Value::as_u64)
                .ok_or_else(|| StorageError::schema("record missing version field"))?;
            if current != ver {
                return Err(StorageError::conflict("version mismatch"));
            }
        }

        let patch_obj = patch
            .as_object()
            .cloned()
            .ok_or_else(|| StorageError::schema("patch must be an object"))?;
        let base_obj = base
            .as_object_mut()
            .ok_or_else(|| StorageError::schema("stored record corrupted"))?;
        for (k, v) in patch_obj {
            base_obj.insert(k, v);
        }

        table.insert(id.to_string(), Value::Object(base_obj.clone()));
        let entity = serde_json::from_value(Value::Object(base_obj.clone()))
            .map_err(|err| StorageError::schema(format!("deserialize entity failed: {err}")))?;
        Ok(entity)
    }

    async fn delete(&self, tenant: &TenantId, id: &str) -> StorageResult<()> {
        let mut tables = self.state.tables.write();
        if let Some(table) = tables.get_mut(T::TABLE) {
            if let Some(existing) = table.get(id) {
                Self::ensure_tenant(existing, tenant)?;
                table.remove(id);
            }
        }
        Ok(())
    }

    async fn select(
        &self,
        tenant: &TenantId,
        filter: Value,
        sorts: Option<Vec<Sort>>,
        limit: usize,
        _cursor: Option<String>,
    ) -> StorageResult<Page<T>> {
        let tables = self.state.tables.read();
        let table = match tables.get(T::TABLE) {
            Some(table) => table,
            None => return Ok(Page::empty()),
        };

        let filter_map = filter.as_object().cloned().unwrap_or_default();

        let mut items: Vec<Value> = table
            .values()
            .filter(|value| Self::ensure_tenant(value, tenant).is_ok())
            .filter(|value| {
                filter_map.iter().all(|(key, expected)| {
                    value.get(key).cloned().unwrap_or(Value::Null) == *expected
                })
            })
            .cloned()
            .collect();

        if let Some(sorts) = sorts {
            for sort in sorts.iter().rev() {
                items.sort_by(|a, b| {
                    let field_a = a.get(&sort.field).cloned().unwrap_or(Value::Null);
                    let field_b = b.get(&sort.field).cloned().unwrap_or(Value::Null);
                    let ord = json_cmp(&field_a, &field_b);
                    if sort.asc {
                        ord
                    } else {
                        ord.reverse()
                    }
                });
            }
        }

        if items.len() > limit {
            items.truncate(limit);
        }

        let mut mapped = Vec::with_capacity(items.len());
        for value in items {
            let entity = serde_json::from_value(value)
                .map_err(|err| StorageError::schema(format!("deserialize entity failed: {err}")))?;
            mapped.push(entity);
        }

        Ok(Page {
            items: mapped,
            next: None,
        })
    }
}

fn json_cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::String(sa), Value::String(sb)) => sa.cmp(sb),
        (Value::Number(na), Value::Number(nb)) => {
            let lhs = na.as_f64().unwrap_or_default();
            let rhs = nb.as_f64().unwrap_or_default();
            lhs.partial_cmp(&rhs).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Value::Bool(ba), Value::Bool(bb)) => ba.cmp(bb),
        _ => std::cmp::Ordering::Equal,
    }
}
