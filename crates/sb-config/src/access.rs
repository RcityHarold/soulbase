use crate::model::{ConfigMap, KeyPath, NamespaceId};
use crate::snapshot::ConfigSnapshot;

pub fn namespace_view(snapshot: &ConfigSnapshot, namespace: &NamespaceId) -> Option<ConfigMap> {
    snapshot
        .get_raw(&KeyPath(namespace.0.clone()))
        .and_then(|value| match value {
            serde_json::Value::Object(map) => Some(map.clone()),
            _ => None,
        })
}

pub fn feature_flag(snapshot: &ConfigSnapshot, key: &KeyPath) -> Option<bool> {
    snapshot.get::<bool>(key)
}
