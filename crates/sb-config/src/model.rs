use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NamespaceId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyPath(pub String);

pub type ConfigValue = serde_json::Value;
pub type ConfigMap = serde_json::Map<String, ConfigValue>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReloadClass {
    BootOnly,
    HotReloadSafe,
    HotReloadRisky,
}

impl ReloadClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReloadClass::BootOnly => "boot_only",
            ReloadClass::HotReloadSafe => "hot_reload_safe",
            ReloadClass::HotReloadRisky => "hot_reload_risky",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProvenanceEntry {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Provenance(pub Vec<ProvenanceEntry>);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SnapshotVersion(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Checksum(pub String);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    pub version: SnapshotVersion,
    pub checksum: Checksum,
    pub issued_at_epoch_ms: i64,
    pub reload_summary: HashMap<KeyPath, ReloadClass>,
}
