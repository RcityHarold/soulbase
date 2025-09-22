use crate::errors::{ToolError, ToolResult};
use crate::manifest::{IdempoKind, SafetyClass, ToolId, ToolManifest};
use crate::mapping::{manifest_to_capabilities, plan_exec_ops};
use crate::observe::{NoopToolMetrics, ToolMetrics};
use crate::registry::{AvailableSpec, ToolRegistry};
use async_trait::async_trait;
use chrono::Utc;
use sb_auth::prelude::Obligation;
use sb_config::prelude::ConfigSnapshot;
use sb_errors::prelude::codes;
use sb_sandbox::prelude::{
    DefaultPolicyGuard, DefaultProfileBuilder, ExecOp, Grant, PolicyConfig, PolicyDefaults,
    PolicyGuard, Profile, ProfileBuilder, SafetyClass as SandboxSafety,
    SideEffect as SandboxSideEffect, ToolManifest as SandboxManifest, Whitelists,
};
use sb_types::prelude::{Consent, Id, Subject, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;

#[cfg(feature = "schema-json")]
use jsonschema::{Draft, JSONSchema};
#[cfg(feature = "schema-json")]
use schemars::schema::RootSchema;
#[cfg(not(feature = "schema-json"))]
pub type RootSchema = serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolOrigin {
    Llm,
    Api,
    System,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool_id: ToolId,
    pub call_id: Id,
    pub actor: Subject,
    pub tenant: TenantId,
    pub origin: ToolOrigin,
    pub args: Value,
    #[serde(default)]
    pub consent: Option<Consent>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigFingerprint {
    pub version: Option<String>,
    pub hash: Option<String>,
}

impl ConfigFingerprint {
    pub fn is_empty(&self) -> bool {
        self.version.is_none() && self.hash.is_none()
    }
}

impl From<&ConfigSnapshot> for ConfigFingerprint {
    fn from(snapshot: &ConfigSnapshot) -> Self {
        let meta = snapshot.metadata();
        Self {
            version: Some(meta.version.0.clone()).filter(|s| !s.is_empty()),
            hash: Some(meta.checksum.0.clone()).filter(|s| !s.is_empty()),
        }
    }
}

#[async_trait]
pub trait ConfigProvider: Send + Sync {
    async fn current(&self) -> Option<ConfigFingerprint>;
}

#[derive(Default)]
pub struct NoopConfigProvider;

#[async_trait]
impl ConfigProvider for NoopConfigProvider {
    async fn current(&self) -> Option<ConfigFingerprint> {
        None
    }
}

pub struct StaticConfigProvider {
    fingerprint: Option<ConfigFingerprint>,
}

impl StaticConfigProvider {
    pub fn new(fingerprint: Option<ConfigFingerprint>) -> Self {
        Self { fingerprint }
    }

    pub fn from_snapshot(snapshot: &ConfigSnapshot) -> Self {
        Self {
            fingerprint: Some(ConfigFingerprint::from(snapshot)),
        }
    }
}

#[async_trait]
impl ConfigProvider for StaticConfigProvider {
    async fn current(&self) -> Option<ConfigFingerprint> {
        self.fingerprint.clone()
    }
}

#[derive(Clone, Debug)]
pub struct PreflightPlan {
    pub spec: AvailableSpec,
    pub sandbox_manifest: SandboxManifest,
    pub grant: Grant,
    pub policy: PolicyConfig,
    pub profile: Profile,
    pub obligations: Vec<Obligation>,
    pub budget_snapshot: Value,
    pub planned_ops: Vec<ExecOp>,
    pub config_version: Option<String>,
    pub config_hash: Option<String>,
}

impl PreflightPlan {
    pub fn profile_hash(&self) -> &str {
        &self.profile.profile_hash
    }

    pub fn planned_ops(&self) -> &[ExecOp] {
        &self.planned_ops
    }
}

#[derive(Clone, Debug)]
pub struct PreflightOutput {
    pub allow: bool,
    pub reason: Option<String>,
    pub error_code: Option<&'static str>,
    pub plan: Option<PreflightPlan>,
}

impl PreflightOutput {
    fn denied_with_code(reason: impl Into<String>, code: &'static str) -> Self {
        Self {
            allow: false,
            reason: Some(reason.into()),
            error_code: Some(code),
            plan: None,
        }
    }

    fn allowed(plan: PreflightPlan) -> Self {
        Self {
            allow: true,
            reason: None,
            error_code: None,
            plan: Some(plan),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AuthDecision {
    pub allow: bool,
    pub obligations: Vec<Obligation>,
    pub reason: Option<String>,
    pub error_code: Option<&'static str>,
}

#[async_trait]
pub trait AuthProvider: Send + Sync {
    async fn authorize(&self, call: &ToolCall, spec: &AvailableSpec) -> ToolResult<AuthDecision>;
}

pub struct AllowAllAuth;

#[async_trait]
impl AuthProvider for AllowAllAuth {
    async fn authorize(&self, _call: &ToolCall, _spec: &AvailableSpec) -> ToolResult<AuthDecision> {
        Ok(AuthDecision {
            allow: true,
            obligations: Vec::new(),
            reason: None,
            error_code: None,
        })
    }
}

pub struct PreflightService<R: ToolRegistry, A: AuthProvider> {
    registry: Arc<R>,
    auth: Arc<A>,
    config: Arc<dyn ConfigProvider>,
    metrics: Arc<dyn ToolMetrics>,
}

impl<R: ToolRegistry, A: AuthProvider> PreflightService<R, A> {
    pub fn new(registry: Arc<R>, auth: Arc<A>) -> Self {
        Self {
            registry,
            auth,
            config: Arc::new(NoopConfigProvider::default()),
            metrics: Arc::new(NoopToolMetrics::default()),
        }
    }

    pub fn with_config_provider(mut self, provider: Arc<dyn ConfigProvider>) -> Self {
        self.config = provider;
        self
    }

    pub fn with_metrics(mut self, metrics: Arc<dyn ToolMetrics>) -> Self {
        self.metrics = metrics;
        self
    }

    pub async fn preflight(&self, call: &ToolCall) -> ToolResult<PreflightOutput> {
        let mut spec = match self.registry.get(&call.tool_id, &call.tenant).await {
            Some(spec) if spec.enabled => spec,
            Some(_) => {
                self.metrics.record_preflight(
                    &call.tenant,
                    &call.tool_id,
                    call.origin,
                    false,
                    Some(codes::POLICY_DENY_TOOL.0),
                );
                return Ok(PreflightOutput::denied_with_code(
                    "tool disabled",
                    codes::POLICY_DENY_TOOL.0,
                ));
            }
            None => {
                self.metrics.record_preflight(
                    &call.tenant,
                    &call.tool_id,
                    call.origin,
                    false,
                    Some(codes::POLICY_DENY_TOOL.0),
                );
                return Ok(PreflightOutput::denied_with_code(
                    "tool not available",
                    codes::POLICY_DENY_TOOL.0,
                ));
            }
        };

        if matches!(spec.manifest.idempotency, IdempoKind::Keyed) && call.idempotency_key.is_none()
        {
            self.metrics.record_preflight(
                &call.tenant,
                &call.tool_id,
                call.origin,
                false,
                Some(codes::POLICY_DENY_TOOL.0),
            );
            return Ok(PreflightOutput::denied_with_code(
                "idempotency key required",
                codes::POLICY_DENY_TOOL.0,
            ));
        }

        if matches!(call.origin, ToolOrigin::Llm) && !spec.visible_to_llm {
            self.metrics.record_preflight(
                &call.tenant,
                &call.tool_id,
                call.origin,
                false,
                Some(codes::POLICY_DENY_TOOL.0),
            );
            return Ok(PreflightOutput::denied_with_code(
                "tool not visible to LLM",
                codes::POLICY_DENY_TOOL.0,
            ));
        }

        validate_args(&spec.manifest.input_schema, &call.args)?;

        if let Err(deny) = ensure_consent(&spec.manifest, call) {
            self.metrics.record_preflight(
                &call.tenant,
                &call.tool_id,
                call.origin,
                false,
                Some(deny.code),
            );
            return Ok(PreflightOutput::denied_with_code(deny.reason, deny.code));
        }

        let AuthDecision {
            allow,
            obligations,
            reason,
            error_code,
        } = self.auth.authorize(call, &spec).await?;
        if !allow {
            let code = error_code.unwrap_or(codes::AUTH_FORBIDDEN.0);
            self.metrics.record_preflight(
                &call.tenant,
                &call.tool_id,
                call.origin,
                false,
                Some(code),
            );
            return Ok(PreflightOutput::denied_with_code(
                reason.unwrap_or_else(|| "denied".into()),
                code,
            ));
        }

        let planned_ops = plan_exec_ops(&spec.manifest, &call.args)?;
        if planned_ops.is_empty() {
            return Err(ToolError::invalid_manifest(
                "no executable operations planned",
            ));
        }

        let fingerprint = self.config.current().await.unwrap_or_default();
        if !fingerprint.is_empty() {
            self.registry
                .update_config_fingerprint(
                    &spec.manifest.id,
                    fingerprint.version.clone(),
                    fingerprint.hash.clone(),
                )
                .await?;
            spec.config_version = fingerprint.version.clone();
            spec.config_hash = fingerprint.hash.clone();
        }

        let sandbox_manifest = to_sandbox_manifest(&spec.manifest);
        let grant = build_grant(call, &spec.manifest);
        let policy = build_policy_config(&spec, &fingerprint);

        let profile = DefaultProfileBuilder::default()
            .build(&grant, &sandbox_manifest, &policy)
            .await
            .map_err(|err| ToolError::sandbox_blocked(err.to_public().message))?;

        let guard = DefaultPolicyGuard::default();
        for capability in manifest_to_capabilities(&spec.manifest) {
            guard
                .validate(&profile, &capability)
                .await
                .map_err(|err| ToolError::sandbox_blocked(err.to_public().message))?;
        }

        let plan = PreflightPlan {
            spec,
            sandbox_manifest,
            grant,
            policy,
            profile,
            obligations,
            budget_snapshot: budget_snapshot(&call.args),
            planned_ops,
            config_version: fingerprint.version,
            config_hash: fingerprint.hash,
        };

        self.metrics
            .record_preflight(&call.tenant, &call.tool_id, call.origin, true, None);

        Ok(PreflightOutput::allowed(plan))
    }
}

struct ConsentFailure {
    reason: String,
    code: &'static str,
}

fn ensure_consent(manifest: &ToolManifest, call: &ToolCall) -> Result<(), ConsentFailure> {
    if !manifest.consent.required {
        return Ok(());
    }

    let consent = call.consent.as_ref().ok_or(ConsentFailure {
        reason: "consent required".into(),
        code: codes::AUTH_FORBIDDEN.0,
    })?;

    if let Some(exp) = consent.expires_at {
        let now_ms = Utc::now().timestamp_millis();
        if exp.as_millis() < now_ms {
            return Err(ConsentFailure {
                reason: "consent expired".into(),
                code: codes::AUTH_FORBIDDEN.0,
            });
        }
        if let Some(max_ttl) = manifest.consent.max_ttl_ms {
            if exp.as_millis() - now_ms > max_ttl as i64 {
                return Err(ConsentFailure {
                    reason: "consent ttl exceeds policy".into(),
                    code: codes::AUTH_FORBIDDEN.0,
                });
            }
        }
    }

    if !manifest.scopes.is_empty() {
        let required: HashSet<(&str, &str)> = manifest
            .scopes
            .iter()
            .map(|s| (s.resource.as_str(), s.action.as_str()))
            .collect();
        let granted: HashSet<(&str, &str)> = consent
            .scopes
            .iter()
            .map(|s| (s.resource.as_str(), s.action.as_str()))
            .collect();
        if !required.is_subset(&granted) {
            return Err(ConsentFailure {
                reason: "consent scopes insufficient".into(),
                code: codes::AUTH_FORBIDDEN.0,
            });
        }
    }

    Ok(())
}

fn validate_args(schema: &RootSchema, value: &Value) -> ToolResult<()> {
    #[cfg(feature = "schema-json")]
    {
        let schema_json = serde_json::to_value(schema)
            .map_err(|err| ToolError::schema(format!("serialize input schema failed: {err}")))?;
        let compiled = JSONSchema::options()
            .with_draft(Draft::Draft202012)
            .compile(&schema_json)
            .map_err(|err| ToolError::schema(format!("compile input schema failed: {err}")))?;
        if let Err(errors) = compiled.validate(value) {
            let messages: Vec<String> = errors.map(|err| err.to_string()).collect();
            let joined = messages.join("; ");
            return Err(ToolError::schema(format!(
                "input schema validation failed: {joined}"
            )));
        };
    }
    Ok(())
}

fn build_grant(call: &ToolCall, manifest: &ToolManifest) -> Grant {
    Grant {
        tenant: call.tenant.clone(),
        subject_id: call.actor.subject_id.clone(),
        tool_name: manifest.id.0.clone(),
        call_id: call.call_id.clone(),
        capabilities: manifest_to_capabilities(manifest),
        expires_at: 0,
        budget: sb_sandbox::prelude::Budget {
            calls: 1,
            bytes_out: manifest.limits.max_bytes_out,
            bytes_in: manifest.limits.max_bytes_in,
            cpu_ms: 0,
            gpu_ms: 0,
            file_count: manifest.limits.max_files,
        },
        decision_fingerprint: "tool-preflight".into(),
        consent: call.consent.clone(),
    }
}

fn build_policy_config(spec: &AvailableSpec, fingerprint: &ConfigFingerprint) -> PolicyConfig {
    PolicyConfig {
        capabilities: manifest_to_capabilities(&spec.manifest),
        safety_class: to_sandbox_safety(spec.manifest.safety_class),
        side_effects: vec![to_sandbox_side(spec.manifest.side_effect)],
        limits: Some(sb_sandbox::prelude::Limits {
            max_bytes_in: Some(spec.manifest.limits.max_bytes_in),
            max_bytes_out: Some(spec.manifest.limits.max_bytes_out),
            max_files: Some(spec.manifest.limits.max_files),
            max_depth: Some(spec.manifest.limits.max_depth),
            max_concurrency: Some(spec.manifest.limits.max_concurrency),
        }),
        whitelists: build_whitelists(&spec.manifest),
        mappings: None,
        timeout_ms: Some(spec.manifest.limits.timeout_ms),
        defaults: PolicyDefaults::default(),
        policy_hash: Some(spec.policy_hash.clone()),
        config_version: fingerprint.version.clone(),
        config_hash: fingerprint.hash.clone(),
    }
}

fn to_sandbox_manifest(manifest: &ToolManifest) -> SandboxManifest {
    SandboxManifest {
        name: manifest.id.0.clone(),
        version: manifest.version.to_string(),
        capabilities: manifest_to_capabilities(manifest),
        safety: to_sandbox_safety(manifest.safety_class),
        side_effects: vec![to_sandbox_side(manifest.side_effect)],
        limits: Some(sb_sandbox::prelude::Limits {
            max_bytes_in: Some(manifest.limits.max_bytes_in),
            max_bytes_out: Some(manifest.limits.max_bytes_out),
            max_files: Some(manifest.limits.max_files),
            max_depth: Some(manifest.limits.max_depth),
            max_concurrency: Some(manifest.limits.max_concurrency),
        }),
        whitelists: build_whitelists(manifest),
        mappings: None,
        timeout_ms: Some(manifest.limits.timeout_ms),
        metadata: manifest.metadata.clone(),
    }
}

fn to_sandbox_safety(safety: SafetyClass) -> SandboxSafety {
    match safety {
        SafetyClass::Low => SandboxSafety::Low,
        SafetyClass::Medium => SandboxSafety::Medium,
        SafetyClass::High => SandboxSafety::High,
    }
}

fn to_sandbox_side(side: crate::manifest::SideEffect) -> SandboxSideEffect {
    match side {
        crate::manifest::SideEffect::None => SandboxSideEffect::None,
        crate::manifest::SideEffect::Read => SandboxSideEffect::Read,
        crate::manifest::SideEffect::Write => SandboxSideEffect::Write,
        crate::manifest::SideEffect::Network => SandboxSideEffect::Network,
        crate::manifest::SideEffect::Filesystem => SandboxSideEffect::Filesystem,
        crate::manifest::SideEffect::Browser => SandboxSideEffect::Browser,
        crate::manifest::SideEffect::Process => SandboxSideEffect::Process,
    }
}

fn build_whitelists(manifest: &ToolManifest) -> Option<Whitelists> {
    let mut whitelist = Whitelists::default();
    let mut populated = false;
    for cap in &manifest.capabilities {
        match cap.domain.as_str() {
            "net.http" => {
                whitelist.domains.push(cap.resource.clone());
                whitelist.methods.push(cap.action.to_uppercase());
                populated = true;
            }
            "fs" => {
                whitelist.paths.push(cap.resource.clone());
                populated = true;
            }
            "proc" => {
                whitelist.tools.push(cap.resource.clone());
                populated = true;
            }
            _ => {}
        }
    }
    if populated {
        Some(whitelist)
    } else {
        None
    }
}

fn budget_snapshot(args: &Value) -> Value {
    json!({
        "args_size_bytes": serde_json::to_vec(args).map(|b| b.len()).unwrap_or(0),
        "timestamp_ms": Utc::now().timestamp_millis(),
    })
}
