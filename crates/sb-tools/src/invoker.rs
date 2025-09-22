use crate::errors::{ToolError, ToolResult};
use crate::events::{NoopToolEventSink, ToolEventSink, ToolInvokeBegin, ToolInvokeEnd};
use crate::manifest::{ConcurrencyKind, IdempoKind, ToolManifest};
use crate::observe::{NoopToolMetrics, ToolMetrics};
use crate::preflight::{PreflightPlan, ToolCall};
use ahash::AHashMap;
use async_trait::async_trait;
use parking_lot::Mutex;
use sb_auth::prelude::Obligation;
use sb_sandbox::exec::{fs::FsExecutor, net::NetExecutor, tmp::TmpExecutor};
use sb_sandbox::prelude::{
    Budget, CapabilityKind, DefaultPolicyGuard, DefaultProfileBuilder, EvidenceEvent,
    ExecuteRequest, NoopBudgetMeter, NoopEvidenceSink, Sandbox, SandboxExecutor,
};
use sb_types::prelude::Id;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Instant;

#[cfg(feature = "schema-json")]
use jsonschema::{Draft, JSONSchema};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvokeStatus {
    Ok,
    Denied,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InvokeResult {
    pub status: InvokeStatus,
    pub error_code: Option<String>,
    pub output: Option<Value>,
    pub evidence_ref: Option<Id>,
}

impl InvokeResult {
    pub fn ok(output: Value, evidence_ref: Option<Id>) -> Self {
        Self {
            status: InvokeStatus::Ok,
            error_code: None,
            output: Some(output),
            evidence_ref,
        }
    }

    pub fn denied(reason: &str) -> Self {
        Self {
            status: InvokeStatus::Denied,
            error_code: Some(reason.to_string()),
            output: None,
            evidence_ref: None,
        }
    }

    pub fn error(code: &str) -> Self {
        Self {
            status: InvokeStatus::Error,
            error_code: Some(code.to_string()),
            output: None,
            evidence_ref: None,
        }
    }
}

pub struct InvokeRequest {
    pub plan: PreflightPlan,
    pub call: ToolCall,
}

#[async_trait]
pub trait IdempotencyStore: Send + Sync {
    async fn get(&self, key: &str) -> Option<InvokeResult>;
    async fn put(&self, key: &str, value: &InvokeResult);
}

pub struct InMemoryIdempotencyStore {
    inner: Mutex<AHashMap<String, InvokeResult>>,
}

impl InMemoryIdempotencyStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(AHashMap::new()),
        }
    }
}

#[async_trait]
impl IdempotencyStore for InMemoryIdempotencyStore {
    async fn get(&self, key: &str) -> Option<InvokeResult> {
        self.inner.lock().get(key).cloned()
    }

    async fn put(&self, key: &str, value: &InvokeResult) {
        self.inner.lock().insert(key.to_string(), value.clone());
    }
}

pub type DefaultSandbox = Sandbox<DefaultProfileBuilder, DefaultPolicyGuard, NoopBudgetMeter>;

pub struct InvokerConfig {
    pub sandbox: Arc<DefaultSandbox>,
    pub idempotency: Arc<dyn IdempotencyStore>,
    pub events: Arc<dyn ToolEventSink>,
    pub metrics: Arc<dyn ToolMetrics>,
}

impl InvokerConfig {
    pub fn with_sandbox(sandbox: Arc<DefaultSandbox>) -> Self {
        Self {
            sandbox,
            idempotency: Arc::new(InMemoryIdempotencyStore::new()),
            events: Arc::new(NoopToolEventSink::default()),
            metrics: Arc::new(NoopToolMetrics::default()),
        }
    }
}

pub struct InvokerImpl {
    config: InvokerConfig,
    concurrency: Arc<Mutex<AHashMap<String, usize>>>,
}

impl InvokerImpl {
    pub fn new(config: InvokerConfig) -> Self {
        Self {
            config,
            concurrency: Arc::new(Mutex::new(AHashMap::new())),
        }
    }

    pub async fn invoke(&self, request: InvokeRequest) -> ToolResult<InvokeResult> {
        let plan = &request.plan;
        let manifest = &plan.spec.manifest;
        let started_at = Instant::now();

        let idempotency_key = match manifest.idempotency {
            IdempoKind::Keyed => request.call.idempotency_key.clone(),
            IdempoKind::None => None,
        };
        if let Some(key) = idempotency_key.as_ref() {
            if let Some(hit) = self.config.idempotency.get(key).await {
                return Ok(hit);
            }
        }

        let _lock = if matches!(manifest.concurrency, ConcurrencyKind::Serial) {
            Some(ConcurrencyGuard::lock(
                Arc::clone(&self.concurrency),
                &manifest.id.0,
                &request.call.tenant.0,
            ))
        } else {
            None
        };

        let planned_ops = plan.planned_ops();
        if planned_ops.is_empty() {
            return Err(ToolError::invalid_manifest("no exec operations derived"));
        }

        let args_digest = digest_json(&request.call.args);
        let begin_event = ToolInvokeBegin {
            envelope_id: request.call.call_id.clone(),
            tenant: request.call.tenant.clone(),
            subject_id: request.call.actor.subject_id.clone(),
            tool_id: manifest.id.clone(),
            tool_version: manifest.version.to_string(),
            call_id: request.call.call_id.clone(),
            origin: request.call.origin,
            safety: manifest.safety_class,
            side_effect: manifest.side_effect,
            profile_hash: plan.profile_hash().to_string(),
            policy_hash: Some(plan.policy.policy_hash.clone().unwrap_or_default())
                .filter(|s| !s.is_empty()),
            config_version: plan.config_version.clone(),
            config_hash: plan.config_hash.clone(),
            args_digest: args_digest.clone(),
        };
        self.config.events.on_invoke_begin(begin_event).await;

        let mut last_output = Value::Null;
        let mut has_output = false;
        let mut status = InvokeStatus::Ok;
        let mut error_code: Option<String> = None;
        let mut failure: Option<ToolError> = None;
        let mut budget_used = Budget::default();
        let mut total_side_effects = Vec::new();
        let mut output_digest: Option<String> = None;
        let mut duration_ms: i64 = 0;

        for (idx, op) in planned_ops.iter().cloned().enumerate() {
            let envelope = Id::from(format!("{}#{}", request.call.call_id.as_str(), idx));
            let execution = self
                .config
                .sandbox
                .execute(ExecuteRequest {
                    grant: plan.grant.clone(),
                    manifest: plan.sandbox_manifest.clone(),
                    policy: plan.policy.clone(),
                    op,
                    envelope_id: envelope,
                })
                .await;

            let outcome = match execution {
                Ok(outcome) => outcome,
                Err(err) => {
                    let public = err.to_public();
                    status = InvokeStatus::Error;
                    error_code = Some(public.code.to_string());
                    failure = Some(ToolError::execution_failed(public.message));
                    break;
                }
            };

            if let EvidenceEvent::End(end) = &outcome.end {
                budget_used.add_assign(&end.budget_used);
                total_side_effects.extend(end.side_effects.clone());
                duration_ms += end.duration_ms;
                if let Some(digest) = &end.outputs_digest {
                    output_digest = Some(format!("{}:{}", digest.algo, digest.b64));
                }
            }

            if !outcome.result.ok {
                status = InvokeStatus::Error;
                error_code = outcome.result.code.clone();
                failure = Some(ToolError::execution_failed(
                    outcome
                        .result
                        .message
                        .clone()
                        .unwrap_or_else(|| "tool execution failed".into()),
                ));
                last_output = outcome.result.out;
                has_output = true;
                break;
            }

            last_output = outcome.result.out;
            has_output = true;
        }

        let mut output = if has_output { last_output } else { Value::Null };

        if status == InvokeStatus::Ok {
            if let Err(err) = apply_obligations(&mut output, &plan.obligations) {
                let public = err.to_public();
                status = InvokeStatus::Error;
                error_code = Some(public.code.to_string());
                failure = Some(err);
                output = Value::Null;
            } else if let Err(err) = validate_output(&plan.spec.manifest, &output) {
                let public = err.to_public();
                status = InvokeStatus::Error;
                error_code = Some(public.code.to_string());
                failure = Some(err);
                output = Value::Null;
            }
        }

        let final_output_digest = if status == InvokeStatus::Ok {
            output_digest.or_else(|| Some(digest_json(&output)))
        } else {
            output_digest
        };

        let duration = started_at.elapsed();
        let end_event = ToolInvokeEnd {
            envelope_id: request.call.call_id.clone(),
            tenant: request.call.tenant.clone(),
            subject_id: request.call.actor.subject_id.clone(),
            tool_id: manifest.id.clone(),
            tool_version: manifest.version.to_string(),
            call_id: request.call.call_id.clone(),
            origin: request.call.origin,
            status,
            error_code: error_code.clone(),
            profile_hash: plan.profile_hash().to_string(),
            policy_hash: Some(plan.policy.policy_hash.clone().unwrap_or_default())
                .filter(|s| !s.is_empty()),
            config_version: plan.config_version.clone(),
            config_hash: plan.config_hash.clone(),
            args_digest,
            output_digest: final_output_digest.clone(),
            side_effects_digest: if total_side_effects.is_empty() {
                None
            } else {
                let value = serde_json::to_value(&total_side_effects).unwrap_or(Value::Null);
                Some(digest_json_value(&value))
            },
            budget_calls: budget_used.calls,
            budget_bytes_in: budget_used.bytes_in,
            budget_bytes_out: budget_used.bytes_out,
            budget_cpu_ms: budget_used.cpu_ms,
            budget_gpu_ms: budget_used.gpu_ms,
            budget_file_count: budget_used.file_count,
            duration_ms: if duration_ms > 0 {
                duration_ms
            } else {
                duration.as_millis() as i64
            },
        };
        self.config.events.on_invoke_end(end_event).await;

        self.config.metrics.record_invocation(
            &request.call.tenant,
            &manifest.id,
            request.call.origin,
            status,
            error_code.as_deref(),
            duration,
        );
        self.config.metrics.record_budget(
            &request.call.tenant,
            &manifest.id,
            request.call.origin,
            budget_used.bytes_in,
            budget_used.bytes_out,
        );

        match failure {
            Some(err) => Err(err),
            None => {
                let result = InvokeResult::ok(output, Some(request.call.call_id.clone()));
                if let Some(key) = idempotency_key {
                    self.config.idempotency.put(&key, &result).await;
                }
                Ok(result)
            }
        }
    }
}

struct ConcurrencyGuard {
    key: String,
    map: Arc<Mutex<AHashMap<String, usize>>>,
}

impl ConcurrencyGuard {
    fn lock(map: Arc<Mutex<AHashMap<String, usize>>>, tool_id: &str, tenant: &str) -> Self {
        let key = format!("{}::{}", tool_id, tenant);
        {
            let mut guard = map.lock();
            *guard.entry(key.clone()).or_insert(0) += 1;
        }
        Self { key, map }
    }
}

impl Drop for ConcurrencyGuard {
    fn drop(&mut self) {
        let mut guard = self.map.lock();
        if let Some(entry) = guard.get_mut(&self.key) {
            if *entry <= 1 {
                guard.remove(&self.key);
            } else {
                *entry -= 1;
            }
        }
    }
}

fn validate_output(manifest: &ToolManifest, value: &Value) -> ToolResult<()> {
    #[cfg(feature = "schema-json")]
    {
        let schema_json = serde_json::to_value(&manifest.output_schema)
            .map_err(|err| ToolError::schema(format!("serialize output schema failed: {err}")))?;
        let compiled = JSONSchema::options()
            .with_draft(Draft::Draft202012)
            .compile(&schema_json)
            .map_err(|err| ToolError::schema(format!("compile output schema failed: {err}")))?;
        if let Err(errors) = compiled.validate(value) {
            let messages: Vec<String> = errors.map(|err| err.to_string()).collect();
            let joined = messages.join("; ");
            return Err(ToolError::schema(format!(
                "output schema validation failed: {joined}"
            )));
        };
    }
    Ok(())
}

fn apply_obligations(value: &mut Value, obligations: &[Obligation]) -> ToolResult<()> {
    for obligation in obligations {
        match obligation.kind.as_str() {
            "mask_fields" => {
                if let Some(paths) = obligation.params.as_array() {
                    for path in paths.iter().filter_map(|p| p.as_str()) {
                        if let Some(target) = value.pointer_mut(path) {
                            *target = Value::String("***".into());
                        }
                    }
                }
            }
            "drop_fields" => {
                if let Some(paths) = obligation.params.as_array() {
                    for path in paths.iter().filter_map(|p| p.as_str()) {
                        if let Some(target) = value.pointer_mut(path) {
                            *target = Value::Null;
                        }
                    }
                }
            }
            _ => {
                // Unknown obligations are ignored to maintain forward compatibility.
            }
        }
    }
    Ok(())
}

fn digest_json(value: &Value) -> String {
    digest_json_value(value)
}

fn digest_json_value(value: &Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn default_sandbox_with_executors() -> Arc<DefaultSandbox> {
    let sandbox = Sandbox::new(
        DefaultProfileBuilder::default(),
        DefaultPolicyGuard::default(),
        NoopBudgetMeter::default(),
    )
    .with_executor(
        CapabilityKind::NetHttp,
        Arc::new(NetExecutor::default()) as Arc<dyn SandboxExecutor>,
    )
    .with_executor(
        CapabilityKind::FsRead,
        Arc::new(FsExecutor::default()) as Arc<dyn SandboxExecutor>,
    )
    .with_executor(
        CapabilityKind::TmpUse,
        Arc::new(TmpExecutor::default()) as Arc<dyn SandboxExecutor>,
    )
    .with_evidence_sink(Arc::new(NoopEvidenceSink::default()));
    Arc::new(sandbox)
}
