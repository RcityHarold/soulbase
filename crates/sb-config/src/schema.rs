use std::collections::HashMap;
use std::sync::RwLock;

use serde_json::Value;

use crate::{
    errors::ConfigError,
    model::{KeyPath, NamespaceId, ReloadClass},
};

#[cfg(feature = "schema_json")]
pub type SchemaDoc = schemars::schema::RootSchema;

#[cfg(not(feature = "schema_json"))]
pub type SchemaDoc = serde_json::Value;

#[derive(Clone, Debug)]
pub struct FieldMeta {
    pub reload: ReloadClass,
    pub sensitive: bool,
    pub default_value: Option<Value>,
    pub description: Option<String>,
}

#[derive(Clone, Debug)]
pub struct NamespaceView {
    pub schema: Option<SchemaDoc>,
    pub fields: HashMap<KeyPath, FieldMeta>,
}

pub trait SchemaRegistry: Send + Sync {
    fn register_namespace(
        &self,
        namespace: NamespaceId,
        schema: Option<SchemaDoc>,
        fields: HashMap<KeyPath, FieldMeta>,
    ) -> Result<(), ConfigError>;

    fn get_namespace(&self, ns: &NamespaceId) -> Option<NamespaceView>;

    fn namespaces(&self) -> Vec<NamespaceId>;

    fn field_meta(&self, ns: &NamespaceId, key: &KeyPath) -> Option<FieldMeta>;
}

#[derive(Default)]
pub struct InMemorySchemaRegistry {
    inner: RwLock<HashMap<NamespaceId, NamespaceView>>,
}

impl InMemorySchemaRegistry {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SchemaRegistry for InMemorySchemaRegistry {
    fn register_namespace(
        &self,
        namespace: NamespaceId,
        schema: Option<SchemaDoc>,
        fields: HashMap<KeyPath, FieldMeta>,
    ) -> Result<(), ConfigError> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| crate::errors::schema_invalid("schema", "registry poisoned"))?;
        guard.insert(namespace, NamespaceView { schema, fields });
        Ok(())
    }

    fn get_namespace(&self, ns: &NamespaceId) -> Option<NamespaceView> {
        let guard = self.inner.read().ok()?;
        guard.get(ns).cloned()
    }

    fn namespaces(&self) -> Vec<NamespaceId> {
        self.inner
            .read()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn field_meta(&self, ns: &NamespaceId, key: &KeyPath) -> Option<FieldMeta> {
        let guard = self.inner.read().ok()?;
        guard.get(ns).and_then(|view| view.fields.get(key)).cloned()
    }
}
