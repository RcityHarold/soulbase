use crate::errors::{ToolError, ToolResult};
use sb_types::prelude::Scope;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

#[cfg(feature = "schema-json")]
use jsonschema::{Draft, JSONSchema};
#[cfg(feature = "schema-json")]
pub type RootSchema = schemars::schema::RootSchema;
#[cfg(not(feature = "schema-json"))]
pub type RootSchema = serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolId(pub String);

impl ToolId {
    pub fn validate(&self) -> ToolResult<()> {
        if self.0.is_empty() {
            return Err(ToolError::invalid_manifest("tool id must not be empty"));
        }
        if !self
            .0
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        {
            return Err(ToolError::invalid_manifest(
                "tool id must contain only [a-zA-Z0-9._-]",
            ));
        }
        if self.0.matches('.').count() < 2 {
            return Err(ToolError::invalid_manifest(
                "tool id must follow <group>.<pkg>.<name>",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SafetyClass {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SideEffect {
    None,
    Read,
    Write,
    Network,
    Filesystem,
    Browser,
    Process,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConsentPolicy {
    pub required: bool,
    #[serde(default)]
    pub max_ttl_ms: Option<u64>,
    #[serde(default)]
    pub scope_hint: Vec<Scope>,
}

impl Default for ConsentPolicy {
    fn default() -> Self {
        Self {
            required: false,
            max_ttl_ms: None,
            scope_hint: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Limits {
    pub timeout_ms: u64,
    pub max_bytes_in: u64,
    pub max_bytes_out: u64,
    pub max_files: u64,
    pub max_depth: u32,
    pub max_concurrency: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,
            max_bytes_in: 2 * 1024 * 1024,
            max_bytes_out: 2 * 1024 * 1024,
            max_files: 8,
            max_depth: 4,
            max_concurrency: 1,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityDecl {
    pub domain: String,
    pub action: String,
    pub resource: String,
    #[serde(default)]
    pub attrs: Value,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CompatMatrix {
    #[serde(default)]
    pub llm_models_allow: Vec<String>,
    #[serde(default)]
    pub platform_min: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdempoKind {
    Keyed,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConcurrencyKind {
    Serial,
    Parallel,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolManifest {
    pub id: ToolId,
    pub version: Version,
    pub display_name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,

    pub input_schema: RootSchema,
    pub output_schema: RootSchema,

    #[serde(default)]
    pub scopes: Vec<Scope>,
    #[serde(default)]
    pub capabilities: Vec<CapabilityDecl>,
    pub side_effect: SideEffect,
    pub safety_class: SafetyClass,
    #[serde(default)]
    pub consent: ConsentPolicy,

    #[serde(default)]
    pub limits: Limits,
    pub idempotency: IdempoKind,
    pub concurrency: ConcurrencyKind,

    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub compat: CompatMatrix,
    #[serde(default)]
    pub deprecated: bool,
}

impl ToolManifest {
    pub fn validate(&self) -> ToolResult<()> {
        self.id.validate()?;
        if matches!(self.safety_class, SafetyClass::Low)
            && matches!(self.side_effect, SideEffect::Write | SideEffect::Process)
        {
            return Err(ToolError::invalid_manifest(
                "write/process side effect requires safety>=Medium",
            ));
        }
        if matches!(self.safety_class, SafetyClass::High) && !self.consent.required {
            return Err(ToolError::invalid_manifest(
                "safety=High tools must require consent",
            ));
        }
        if self.capabilities.is_empty() {
            return Err(ToolError::invalid_manifest(
                "capabilities must not be empty",
            ));
        }
        self.validate_capability_scope_alignment()?;
        self.validate_schema(&self.input_schema, "input_schema")?;
        self.validate_schema(&self.output_schema, "output_schema")?;
        Ok(())
    }

    fn validate_capability_scope_alignment(&self) -> ToolResult<()> {
        let cap_domains: HashSet<&str> = self
            .capabilities
            .iter()
            .map(|c| c.domain.as_str())
            .collect();
        if cap_domains.contains("fs") {
            let has_write_cap = self
                .capabilities
                .iter()
                .any(|c| c.domain == "fs" && c.action.contains("write"));
            if has_write_cap && !self.scopes.iter().any(|s| s.action == "write") {
                return Err(ToolError::invalid_manifest(
                    "fs write capability requires write scope",
                ));
            }
        }
        Ok(())
    }

    fn validate_schema(&self, schema: &RootSchema, _label: &str) -> ToolResult<()> {
        #[cfg(feature = "schema-json")]
        {
            let compiled = compile_schema(schema)?;
            let _ = compiled.is_valid(&json!({}));
        }
        #[cfg(not(feature = "schema-json"))]
        {
            if schema.is_null() {
                return Err(ToolError::invalid_manifest(format!(
                    "{label} must not be null"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(feature = "schema-json")]
fn compile_schema(schema: &RootSchema) -> ToolResult<JSONSchema> {
    let value = serde_json::to_value(schema)
        .map_err(|err| ToolError::invalid_manifest(format!("serialize schema failed: {err}")))?;
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&value)
        .map_err(|err| ToolError::invalid_manifest(format!("compile schema failed: {err}")))
}
