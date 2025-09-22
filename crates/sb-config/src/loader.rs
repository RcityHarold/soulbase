use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use sb_errors::prelude::codes;
use sb_types::prelude::Timestamp;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::errors::ConfigError;
use crate::events::ConfigUpdateEvent;
use crate::model::{
    Checksum, ConfigMap, ConfigValue, KeyPath, NamespaceId, ReloadClass, SnapshotMetadata,
    SnapshotVersion,
};
use crate::schema::SchemaRegistry;
use crate::secrets::SecretResolver;
use crate::snapshot::ConfigSnapshot;
use crate::source::{merge_maps, merge_value, Source};
use crate::validate::Validator;

#[derive(Clone)]
pub struct Loader {
    pub sources: Vec<Arc<dyn Source>>, // 顺序决定覆盖关系
    pub secrets: Vec<Arc<dyn SecretResolver>>,
    pub validator: Arc<dyn Validator>,
    pub schema_registry: Arc<dyn SchemaRegistry>,
}

impl Loader {
    pub async fn load_once(&self) -> Result<ConfigSnapshot, ConfigError> {
        let (snapshot, _) = self.load_with_prev(None).await?;
        Ok(snapshot)
    }

    pub async fn load_with_prev(
        &self,
        previous: Option<&ConfigSnapshot>,
    ) -> Result<(ConfigSnapshot, ConfigUpdateEvent), ConfigError> {
        let mut merged = ConfigMap::new();

        for source in &self.sources {
            let snap = source.load().await?;
            merge_maps(&mut merged, &snap.map);
        }

        apply_defaults(&mut merged, self.schema_registry.as_ref());
        enforce_schema(&merged, self.schema_registry.as_ref())?;

        let mut working = merged.clone();
        for resolver in &self.secrets {
            resolver.resolve(&mut working).await?;
        }

        self.validator
            .validate(&NamespaceId("root".to_string()), &working)
            .await?;

        let issued_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let checksum = compute_checksum(&working);
        let reload_summary = build_reload_summary(self.schema_registry.as_ref());

        let metadata = SnapshotMetadata {
            version: SnapshotVersion(format!("v{}", issued_at_ms)),
            checksum: checksum.clone(),
            issued_at_epoch_ms: issued_at_ms,
            reload_summary,
        };

        let snapshot = ConfigSnapshot::new(working, metadata.clone());
        let changed = compute_changed_keys(previous, &snapshot);

        let event = ConfigUpdateEvent {
            from_version: previous.map(|snap| snap.metadata().version.clone()),
            to_version: metadata.version.clone(),
            checksum,
            changed_keys: changed,
            issued_at: Timestamp(issued_at_ms),
        };

        Ok((snapshot, event))
    }
}

fn apply_defaults(map: &mut ConfigMap, registry: &dyn SchemaRegistry) {
    for ns in registry.namespaces() {
        if let Some(view) = registry.get_namespace(&ns) {
            for (relative_key, meta) in &view.fields {
                if let Some(default) = &meta.default_value {
                    let full = compose_full_path(&ns, relative_key);
                    if !path_exists(map, &full) {
                        merge_value(map, &full, default.clone());
                    }
                }
            }
        }
    }
}

fn enforce_schema(map: &ConfigMap, registry: &dyn SchemaRegistry) -> Result<(), ConfigError> {
    let mut flattened = Vec::new();
    collect_keys("", map, &mut flattened);
    for key in flattened {
        let (namespace, relative) = split_namespace(&key)?;
        let ns_id = NamespaceId(namespace.to_string());
        let view = registry.get_namespace(&ns_id).ok_or_else(|| {
            ConfigError::from(
                ConfigError::builder(codes::SCHEMA_VALIDATION_FAILED)
                    .user_msg("Configuration namespace not registered.")
                    .dev_msg(format!("unknown namespace: {}", namespace))
                    .build(),
            )
        })?;

        if relative.is_empty() {
            continue;
        }

        let field_key = KeyPath(relative.to_string());
        if !view.fields.contains_key(&field_key) {
            return Err(ConfigError::from(
                ConfigError::builder(codes::SCHEMA_VALIDATION_FAILED)
                    .user_msg("Configuration contains unknown key.")
                    .dev_msg(format!("unknown key: {}", key))
                    .build(),
            ));
        }
    }
    Ok(())
}

fn compute_checksum(map: &ConfigMap) -> Checksum {
    use sha2::{Digest, Sha256};

    let value = serde_json::Value::Object(map.clone());
    let mut hasher = Sha256::new();
    if let Ok(serialised) = serde_json::to_vec(&value) {
        hasher.update(serialised);
    }
    let digest = hasher.finalize();
    Checksum(STANDARD.encode(digest))
}

fn compose_full_path(ns: &NamespaceId, key: &KeyPath) -> String {
    if key.0.is_empty() {
        ns.0.clone()
    } else {
        format!("{}.{}", ns.0, key.0)
    }
}

fn path_exists(map: &ConfigMap, path: &str) -> bool {
    let mut cursor = map;
    let mut segments = path.split('.').filter(|s| !s.is_empty()).peekable();
    while let Some(segment) = segments.next() {
        match (segments.peek(), cursor.get(segment)) {
            (Some(_), Some(serde_json::Value::Object(child))) => {
                cursor = child;
            }
            (Some(_), Some(_)) => return false,
            (None, Some(_)) => return true,
            (_, None) => return false,
        }
    }
    false
}

fn collect_keys(prefix: &str, map: &ConfigMap, out: &mut Vec<String>) {
    for (key, value) in map {
        let next = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", prefix, key)
        };
        out.push(next.clone());
        if let ConfigValue::Object(child) = value {
            collect_keys(&next, child, out);
        }
    }
}

fn split_namespace(path: &str) -> Result<(&str, &str), ConfigError> {
    let mut parts = path.splitn(2, '.');
    let ns = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");
    if ns.is_empty() {
        return Err(ConfigError::from(
            ConfigError::builder(codes::SCHEMA_VALIDATION_FAILED)
                .user_msg("Configuration key missing namespace prefix.")
                .dev_msg(format!("invalid key: {}", path))
                .build(),
        ));
    }
    Ok((ns, rest))
}

fn build_reload_summary(registry: &dyn SchemaRegistry) -> HashMap<KeyPath, ReloadClass> {
    let mut summary = HashMap::new();
    for ns in registry.namespaces() {
        if let Some(view) = registry.get_namespace(&ns) {
            for (key, meta) in view.fields {
                let full = compose_full_path(&ns, &key);
                summary.insert(KeyPath(full), meta.reload);
            }
        }
    }
    summary
}

fn flatten_snapshot(snapshot: &ConfigSnapshot) -> HashMap<String, ConfigValue> {
    let mut map = HashMap::new();
    if let ConfigValue::Object(root) = snapshot.root_value() {
        flatten_map(root, "", &mut map);
    }
    map
}

fn flatten_map(value: &ConfigMap, prefix: &str, out: &mut HashMap<String, ConfigValue>) {
    for (key, val) in value {
        let next = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", prefix, key)
        };
        match val {
            ConfigValue::Object(child) => flatten_map(child, &next, out),
            _ => {
                out.insert(next, val.clone());
            }
        }
    }
}

fn compute_changed_keys(prev: Option<&ConfigSnapshot>, current: &ConfigSnapshot) -> Vec<KeyPath> {
    let current_flat = flatten_snapshot(current);
    let prev_flat = prev.map(flatten_snapshot).unwrap_or_default();

    let mut keys = HashSet::new();
    keys.extend(current_flat.keys().cloned());
    keys.extend(prev_flat.keys().cloned());

    let mut changed = Vec::new();
    for key in keys {
        match (prev_flat.get(&key), current_flat.get(&key)) {
            (Some(a), Some(b)) if a == b => {}
            (None, None) => {}
            _ => changed.push(KeyPath(key)),
        }
    }
    changed.sort_by(|a, b| a.0.cmp(&b.0));
    changed
}
