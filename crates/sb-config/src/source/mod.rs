use crate::{
    errors::ConfigError,
    model::{ConfigMap, ProvenanceEntry},
};
use async_trait::async_trait;

pub mod cli;
pub mod env;
pub mod file;

#[derive(Clone, Debug)]
pub struct SourceSnapshot {
    pub map: ConfigMap,
    pub provenance: crate::model::Provenance,
}

#[async_trait]
pub trait Source: Send + Sync {
    fn id(&self) -> &'static str;
    async fn load(&self) -> Result<SourceSnapshot, ConfigError>;
}

pub(crate) fn merge_value(target: &mut ConfigMap, key: &str, value: serde_json::Value) {
    let segments: Vec<&str> = key.split('.').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return;
    }
    insert_segments(target, &segments, value);
}

fn insert_segments(root: &mut ConfigMap, segments: &[&str], value: serde_json::Value) {
    if segments.len() == 1 {
        root.insert(segments[0].to_owned(), value);
        return;
    }

    let entry = root
        .entry(segments[0].to_owned())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));

    if let serde_json::Value::Object(map) = entry {
        insert_segments(map, &segments[1..], value);
    } else {
        let mut new_map = serde_json::Map::new();
        insert_segments(&mut new_map, &segments[1..], value);
        *entry = serde_json::Value::Object(new_map);
    }
}

pub(crate) fn merge_maps(base: &mut ConfigMap, overlay: &ConfigMap) {
    for (key, value) in overlay {
        match (base.get_mut(key), value) {
            (Some(serde_json::Value::Object(base_obj)), serde_json::Value::Object(overlay_obj)) => {
                merge_maps(base_obj, overlay_obj);
            }
            _ => {
                base.insert(key.clone(), value.clone());
            }
        }
    }
}

pub(crate) fn provenance_entry(source: &str, layer: &str) -> ProvenanceEntry {
    ProvenanceEntry {
        source: source.to_string(),
        version: None,
        layer: Some(layer.to_string()),
    }
}
