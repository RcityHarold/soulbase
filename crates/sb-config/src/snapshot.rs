use crate::model::{Checksum, ConfigMap, ConfigValue, KeyPath, SnapshotMetadata};
use serde::de::DeserializeOwned;

#[derive(Clone, Debug)]
pub struct ConfigSnapshot {
    data: ConfigValue,
    metadata: SnapshotMetadata,
}

impl ConfigSnapshot {
    pub fn new(map: ConfigMap, metadata: SnapshotMetadata) -> Self {
        Self {
            data: ConfigValue::Object(map),
            metadata,
        }
    }

    pub fn metadata(&self) -> &SnapshotMetadata {
        &self.metadata
    }

    pub fn checksum(&self) -> &Checksum {
        &self.metadata.checksum
    }

    pub fn get_raw(&self, path: &KeyPath) -> Option<&ConfigValue> {
        let segments: Vec<&str> = path.0.split('.').filter(|s| !s.is_empty()).collect();
        if segments.is_empty() {
            return None;
        }
        let mut cursor = &self.data;
        for segment in segments {
            match cursor {
                ConfigValue::Object(map) => {
                    cursor = map.get(segment)?;
                }
                _ => return None,
            }
        }
        Some(cursor)
    }

    pub fn get<T>(&self, path: &KeyPath) -> Option<T>
    where
        T: DeserializeOwned,
    {
        self.get_raw(path)
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }

    pub fn iter_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        if let ConfigValue::Object(map) = &self.data {
            collect_keys("", map, &mut keys);
        }
        keys
    }

    pub(crate) fn root_value(&self) -> &ConfigValue {
        &self.data
    }
}

fn collect_keys(prefix: &str, map: &ConfigMap, keys: &mut Vec<String>) {
    for (key, value) in map {
        let next_prefix = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", prefix, key)
        };
        keys.push(next_prefix.clone());
        if let ConfigValue::Object(child) = value {
            collect_keys(&next_prefix, child, keys);
        }
    }
}
