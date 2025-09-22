use crate::model::{Capability, Limits, Mappings, SafetyClass, SideEffect, Whitelists};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PolicyConfig {
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub safety_class: SafetyClass,
    #[serde(default)]
    pub side_effects: Vec<SideEffect>,
    #[serde(default)]
    pub limits: Option<Limits>,
    #[serde(default)]
    pub whitelists: Option<Whitelists>,
    #[serde(default)]
    pub mappings: Option<Mappings>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub defaults: PolicyDefaults,
    #[serde(default)]
    pub policy_hash: Option<String>,
    #[serde(default)]
    pub config_version: Option<String>,
    #[serde(default)]
    pub config_hash: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PolicyDefaults {
    #[serde(default = "default_timeout_opt")]
    pub timeout_ms: Option<u64>,
}

const fn default_timeout_opt() -> Option<u64> {
    Some(30_000)
}
