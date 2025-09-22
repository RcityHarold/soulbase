use sb_types::prelude::TenantId;
use serde::{de::DeserializeOwned, Serialize};

pub trait Entity: Serialize + DeserializeOwned + Send + Sync + 'static {
    const TABLE: &'static str;
    type Key: ToString + Send + Sync;

    fn id(&self) -> &str;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next: Option<String>,
}

impl<T> Page<T> {
    pub fn empty() -> Self {
        Self {
            items: Vec::new(),
            next: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sort {
    pub field: String,
    pub asc: bool,
}

impl Sort {
    pub fn ascending(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            asc: true,
        }
    }

    pub fn descending(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            asc: false,
        }
    }
}

pub fn make_record_id(table: &str, tenant: &TenantId, suffix: &str) -> String {
    format!("{table}:{}_{}", tenant.0, suffix)
}
