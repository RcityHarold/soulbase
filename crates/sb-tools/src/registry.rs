use crate::errors::{ToolError, ToolResult};
use crate::manifest::{SafetyClass, SideEffect, ToolId, ToolManifest};
use parking_lot::RwLock;
use sb_types::prelude::TenantId;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolState {
    Registered,
    Enabled,
    Paused,
    Deprecated,
}

#[derive(Clone, Debug)]
pub struct RegistryRecord {
    pub manifest: ToolManifest,
    pub state: ToolState,
    pub created_at: i64,
    pub updated_at: i64,
    pub policy_hash: String,
    pub visible_to_llm: bool,
    pub config_version: Option<String>,
    pub config_hash: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AvailableSpec {
    pub manifest: ToolManifest,
    pub policy_hash: String,
    pub enabled: bool,
    pub visible_to_llm: bool,
    pub safety_class: SafetyClass,
    pub side_effect: SideEffect,
    pub config_version: Option<String>,
    pub config_hash: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ListFilter {
    pub tags: Vec<String>,
    pub safety_le: Option<SafetyClass>,
    pub side_effect_in: Vec<SideEffect>,
    pub text: Option<String>,
    pub visible_only: bool,
}

#[async_trait::async_trait]
pub trait ToolRegistry: Send + Sync {
    async fn register(&self, manifest: ToolManifest) -> ToolResult<()>;
    async fn update(&self, manifest: ToolManifest) -> ToolResult<()>;
    async fn set_state(&self, id: &ToolId, state: ToolState) -> ToolResult<()>;
    async fn update_policy(
        &self,
        id: &ToolId,
        policy_hash: Option<String>,
        visible_to_llm: Option<bool>,
    ) -> ToolResult<()>;
    async fn update_config_fingerprint(
        &self,
        id: &ToolId,
        version: Option<String>,
        hash: Option<String>,
    ) -> ToolResult<()>;
    async fn get(&self, id: &ToolId, tenant: &TenantId) -> Option<AvailableSpec>;
    async fn list(&self, tenant: &TenantId, filter: ListFilter) -> Vec<AvailableSpec>;
}

pub struct InMemoryRegistry {
    records: RwLock<HashMap<ToolId, RegistryRecord>>,
}

impl InMemoryRegistry {
    pub fn new() -> Self {
        Self {
            records: RwLock::new(HashMap::new()),
        }
    }

    fn convert(&self, record: &RegistryRecord) -> AvailableSpec {
        AvailableSpec {
            manifest: record.manifest.clone(),
            policy_hash: record.policy_hash.clone(),
            enabled: matches!(record.state, ToolState::Enabled),
            visible_to_llm: record.visible_to_llm
                && matches!(record.state, ToolState::Enabled)
                && !record.manifest.deprecated,
            safety_class: record.manifest.safety_class,
            side_effect: record.manifest.side_effect,
            config_version: record.config_version.clone(),
            config_hash: record.config_hash.clone(),
        }
    }
}

#[async_trait::async_trait]
impl ToolRegistry for InMemoryRegistry {
    async fn register(&self, manifest: ToolManifest) -> ToolResult<()> {
        manifest.validate()?;
        let mut map = self.records.write();
        if map.contains_key(&manifest.id) {
            return Err(ToolError::invalid_manifest("tool already exists"));
        }
        let now = chrono::Utc::now().timestamp_millis();
        map.insert(
            manifest.id.clone(),
            RegistryRecord {
                manifest,
                state: ToolState::Enabled,
                created_at: now,
                updated_at: now,
                policy_hash: "policy:default".into(),
                visible_to_llm: true,
                config_version: None,
                config_hash: None,
            },
        );
        Ok(())
    }

    async fn update(&self, manifest: ToolManifest) -> ToolResult<()> {
        manifest.validate()?;
        let mut map = self.records.write();
        let record = map
            .get_mut(&manifest.id)
            .ok_or_else(|| ToolError::not_found("tool not registered"))?;
        record.manifest = manifest;
        record.updated_at = chrono::Utc::now().timestamp_millis();
        Ok(())
    }

    async fn set_state(&self, id: &ToolId, state: ToolState) -> ToolResult<()> {
        let mut map = self.records.write();
        let record = map
            .get_mut(id)
            .ok_or_else(|| ToolError::not_found("tool not registered"))?;
        record.state = state;
        record.updated_at = chrono::Utc::now().timestamp_millis();
        Ok(())
    }

    async fn update_policy(
        &self,
        id: &ToolId,
        policy_hash: Option<String>,
        visible_to_llm: Option<bool>,
    ) -> ToolResult<()> {
        let mut map = self.records.write();
        let record = map
            .get_mut(id)
            .ok_or_else(|| ToolError::not_found("tool not registered"))?;
        if let Some(hash) = policy_hash {
            record.policy_hash = hash;
        }
        if let Some(visible) = visible_to_llm {
            record.visible_to_llm = visible;
        }
        record.updated_at = chrono::Utc::now().timestamp_millis();
        Ok(())
    }

    async fn update_config_fingerprint(
        &self,
        id: &ToolId,
        version: Option<String>,
        hash: Option<String>,
    ) -> ToolResult<()> {
        let mut map = self.records.write();
        let record = map
            .get_mut(id)
            .ok_or_else(|| ToolError::not_found("tool not registered"))?;
        record.config_version = version;
        record.config_hash = hash;
        record.updated_at = chrono::Utc::now().timestamp_millis();
        Ok(())
    }

    async fn get(&self, id: &ToolId, _tenant: &TenantId) -> Option<AvailableSpec> {
        let map = self.records.read();
        map.get(id).map(|record| self.convert(record))
    }

    async fn list(&self, _tenant: &TenantId, filter: ListFilter) -> Vec<AvailableSpec> {
        let map = self.records.read();
        map.values()
            .filter_map(|record| {
                let spec = self.convert(record);
                if !spec.enabled {
                    return None;
                }
                if filter.visible_only && !spec.visible_to_llm {
                    return None;
                }
                if let Some(safety) = filter.safety_le {
                    if spec.safety_class as u8 > safety as u8 {
                        return None;
                    }
                }
                if !filter.side_effect_in.is_empty()
                    && !filter.side_effect_in.contains(&spec.side_effect)
                {
                    return None;
                }
                if let Some(ref text) = filter.text {
                    if !spec.manifest.display_name.contains(text)
                        && !spec.manifest.description.contains(text)
                    {
                        return None;
                    }
                }
                if !filter.tags.is_empty()
                    && !filter
                        .tags
                        .iter()
                        .all(|tag| spec.manifest.tags.contains(tag))
                {
                    return None;
                }
                Some(spec)
            })
            .collect()
    }
}
