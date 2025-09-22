#![cfg(feature = "surreal")]

use crate::errors::{StorageError, StorageResult};
use crate::spi::query::NamedArgs;
use sb_types::prelude::TenantId;

pub fn bind_tenant(mut params: NamedArgs, tenant: &TenantId) -> StorageResult<NamedArgs> {
    if let Some(existing) = params.get("tenant") {
        match existing.as_str() {
            Some(value) if value == tenant.0 => {}
            Some(value) => {
                return Err(StorageError::schema(format!(
                    "tenant guard mismatch: expected '{}', got '{}'",
                    tenant.0, value
                )))
            }
            None => {
                return Err(StorageError::schema(
                    "tenant parameter must be a string value",
                ))
            }
        }
    }

    params.insert("tenant".into(), serde_json::Value::String(tenant.0.clone()));
    Ok(params)
}

pub fn filtered_bindings(params: &NamedArgs) -> serde_json::Value {
    let mut map = serde_json::Map::with_capacity(params.len());
    for (k, v) in params {
        if k.starts_with("__") {
            continue;
        }
        map.insert(k.clone(), v.clone());
    }
    serde_json::Value::Object(map)
}
