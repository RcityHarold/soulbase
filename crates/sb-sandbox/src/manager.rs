use crate::budget::{BudgetMeter, NoopBudgetMeter};
use crate::config::PolicyConfig;
use crate::errors::SandboxError;
use crate::evidence::{digest_value, EvidenceBuilder, EvidenceEvent, EvidenceStatus};
use crate::exec::{ExecCtx, ExecOp, ExecResult, ExecUsage, NoopCancelToken, SandboxExecutor};
use crate::guard::PolicyGuard;
use crate::model::{Budget, Capability, CapabilityKind, Grant, Profile, SafetyClass, ToolManifest};
use crate::observe::{EvidenceSink, NoopEvidenceSink};
use crate::profile::ProfileBuilder;
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::Utc;
use sb_types::prelude::Id;
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use url::Url;

#[async_trait]
pub trait RevocationWatcher: Send + Sync {
    async fn is_revoked(&self, grant: &Grant) -> Result<bool, SandboxError>;
}

#[derive(Default)]
pub struct NoopRevocationWatcher;

#[async_trait]
impl RevocationWatcher for NoopRevocationWatcher {
    async fn is_revoked(&self, _grant: &Grant) -> Result<bool, SandboxError> {
        Ok(false)
    }
}

#[derive(Clone, Debug)]
pub struct ExecutionOutcome {
    pub begin: EvidenceEvent,
    pub end: EvidenceEvent,
    pub result: ExecResult,
}

#[derive(Clone, Debug)]
pub struct ExecuteRequest {
    pub grant: Grant,
    pub manifest: ToolManifest,
    pub policy: PolicyConfig,
    pub op: ExecOp,
    pub envelope_id: Id,
}

pub struct Sandbox<B, G, M> {
    profile_builder: Arc<B>,
    guard: Arc<G>,
    meter: Arc<M>,
    executors: HashMap<CapabilityKind, Arc<dyn SandboxExecutor>>,
    evidence_sink: Arc<dyn EvidenceSink>,
    revocation: Arc<dyn RevocationWatcher>,
}

impl<B, G, M> Sandbox<B, G, M>
where
    B: ProfileBuilder + 'static,
    G: PolicyGuard + 'static,
    M: BudgetMeter + 'static,
{
    pub fn new(builder: B, guard: G, meter: M) -> Self {
        Self {
            profile_builder: Arc::new(builder),
            guard: Arc::new(guard),
            meter: Arc::new(meter),
            executors: HashMap::new(),
            evidence_sink: Arc::new(NoopEvidenceSink::default()),
            revocation: Arc::new(NoopRevocationWatcher::default()),
        }
    }

    pub fn with_executor(
        mut self,
        kind: CapabilityKind,
        executor: Arc<dyn SandboxExecutor>,
    ) -> Self {
        self.executors.insert(kind, executor);
        self
    }

    pub fn with_evidence_sink(mut self, sink: Arc<dyn EvidenceSink>) -> Self {
        self.evidence_sink = sink;
        self
    }

    pub fn with_revocation_watcher(mut self, watcher: Arc<dyn RevocationWatcher>) -> Self {
        self.revocation = watcher;
        self
    }

    pub async fn execute(&self, request: ExecuteRequest) -> Result<ExecutionOutcome, SandboxError> {
        ensure_grant_active(&request.grant)?;
        if self.revocation.is_revoked(&request.grant).await? {
            return Err(SandboxError::permission_denied("grant has been revoked"));
        }

        let profile = self
            .profile_builder
            .build(&request.grant, &request.manifest, &request.policy)
            .await?;
        let capability = select_capability(&profile, &request.op)?;
        ensure_consent(&request.grant, &capability, &request.op, &profile)?;
        self.guard.validate(&profile, &capability).await?;

        let executor = self
            .executors
            .get(&capability.kind())
            .ok_or_else(|| SandboxError::capability_missing("executor not registered"))?
            .clone();

        let reservation = estimate_budget(&request.op)?;
        self.meter.reserve(&reservation).await?;

        let input_value = serde_json::to_value(&request.op).unwrap_or(serde_json::Value::Null);
        let input_digest = Some(digest_value(&input_value));

        let builder = EvidenceBuilder::new(
            profile.clone(),
            capability.clone(),
            request.envelope_id.clone(),
        );
        let begin_event = builder.begin(input_digest.clone());
        self.evidence_sink.emit(begin_event.clone()).await;

        let cancel = NoopCancelToken::default();
        let ctx = ExecCtx {
            profile: &profile,
            cancel: &cancel,
        };

        let exec_result = match executor.execute(&ctx, request.op.clone()).await {
            Ok(result) => result,
            Err(err) => {
                self.meter.rollback(&reservation).await;
                let public = err.to_public();
                let failure =
                    ExecResult::failure(public.code.to_string(), Some(public.message.to_string()));
                let status = map_error_status(&public.code);
                let end_event = builder.end(
                    status,
                    Some(public.code.to_string()),
                    input_digest.clone(),
                    None,
                    Vec::new(),
                    Budget::default(),
                );
                self.evidence_sink.emit(end_event.clone()).await;
                return Ok(ExecutionOutcome {
                    begin: begin_event,
                    end: end_event,
                    result: failure,
                });
            }
        };

        ensure_usage_within_limits(&profile, &exec_result.usage)?;

        let usage_budget: Budget = (&exec_result.usage).into();
        self.meter.commit(&usage_budget).await;

        let outputs_digest = Some(digest_value(&exec_result.out));
        let status = if exec_result.ok {
            EvidenceStatus::Ok
        } else {
            EvidenceStatus::Error
        };

        let end_event = builder.end(
            status,
            exec_result.code.clone(),
            input_digest,
            outputs_digest,
            exec_result.side_effects.clone(),
            usage_budget,
        );
        self.evidence_sink.emit(end_event.clone()).await;

        Ok(ExecutionOutcome {
            begin: begin_event,
            end: end_event,
            result: exec_result,
        })
    }
}

impl<B, G> Sandbox<B, G, NoopBudgetMeter>
where
    B: ProfileBuilder + 'static,
    G: PolicyGuard + 'static,
{
    pub fn with_noop_meter(builder: B, guard: G) -> Self {
        Self::new(builder, guard, NoopBudgetMeter::default())
    }
}

fn select_capability(profile: &Profile, op: &ExecOp) -> Result<Capability, SandboxError> {
    match op {
        ExecOp::FsRead { path, .. } => find_fs_capability(profile, CapabilityKind::FsRead, path),
        ExecOp::FsWrite { path, .. } => find_fs_capability(profile, CapabilityKind::FsWrite, path),
        ExecOp::FsList { path } => find_fs_capability(profile, CapabilityKind::FsList, path),
        ExecOp::NetHttp { url, method, .. } => find_net_capability(profile, url, method),
        ExecOp::BrowserNav { .. } | ExecOp::BrowserScreenshot { .. } => {
            find_first(profile, CapabilityKind::BrowserUse)
        }
        ExecOp::ProcExec { tool, .. } => find_proc_capability(profile, tool),
        ExecOp::TmpAlloc { .. } => find_first(profile, CapabilityKind::TmpUse),
    }
}

fn find_first(profile: &Profile, kind: CapabilityKind) -> Result<Capability, SandboxError> {
    profile
        .capabilities
        .iter()
        .find(|cap| cap.kind() == kind)
        .cloned()
        .ok_or_else(|| SandboxError::capability_missing("required capability not granted"))
}

fn find_fs_capability(
    profile: &Profile,
    kind: CapabilityKind,
    path: &str,
) -> Result<Capability, SandboxError> {
    let normalized = crate::guard::normalize_path(path);
    profile
        .capabilities
        .iter()
        .filter(|cap| cap.kind() == kind)
        .find(|cap| match cap {
            Capability::FsRead { path }
            | Capability::FsWrite { path, .. }
            | Capability::FsList { path } => {
                normalized.starts_with(&crate::guard::normalize_path(path))
            }
            _ => false,
        })
        .cloned()
        .ok_or_else(|| {
            SandboxError::capability_missing("filesystem path not covered by capability")
        })
}

fn find_net_capability(
    profile: &Profile,
    url_str: &str,
    method: &str,
) -> Result<Capability, SandboxError> {
    let parsed = Url::parse(url_str).map_err(|_| SandboxError::policy_violation("invalid URL"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| SandboxError::policy_violation("URL missing host"))?
        .to_string();
    let scheme = parsed.scheme().to_string();
    let port = parsed.port();
    let method_upper = method.to_uppercase();

    profile
        .capabilities
        .iter()
        .filter(|cap| matches!(cap, Capability::NetHttp { .. }))
        .find(|cap| match cap {
            Capability::NetHttp {
                host: allowed_host,
                port: allowed_port,
                scheme: allowed_scheme,
                methods,
            } => {
                if !methods.is_empty()
                    && !methods
                        .iter()
                        .any(|m| m.eq_ignore_ascii_case(&method_upper))
                {
                    return false;
                }
                if let Some(expected_scheme) = allowed_scheme {
                    if !expected_scheme.eq_ignore_ascii_case(&scheme) {
                        return false;
                    }
                }
                if let Some(expected_port) = allowed_port {
                    if Some(*expected_port) != port {
                        return false;
                    }
                }
                host.ends_with(allowed_host)
            }
            _ => false,
        })
        .cloned()
        .ok_or_else(|| SandboxError::capability_missing("network host not allowed"))
}

fn find_proc_capability(profile: &Profile, tool: &str) -> Result<Capability, SandboxError> {
    profile
        .capabilities
        .iter()
        .filter(|cap| matches!(cap, Capability::ProcExec { .. }))
        .find(|cap| match cap {
            Capability::ProcExec { tool: allowed_tool } => allowed_tool == tool,
            _ => false,
        })
        .cloned()
        .ok_or_else(|| SandboxError::capability_missing("process tool not allowed"))
}

fn estimate_budget(op: &ExecOp) -> Result<Budget, SandboxError> {
    Ok(match op {
        ExecOp::FsRead { len, .. } => Budget {
            calls: 1,
            bytes_in: len.unwrap_or(0),
            ..Budget::default()
        },
        ExecOp::FsWrite { bytes_b64, .. } => Budget {
            calls: 1,
            bytes_out: decoded_len(bytes_b64)?,
            ..Budget::default()
        },
        ExecOp::FsList { .. } => Budget {
            calls: 1,
            ..Budget::default()
        },
        ExecOp::NetHttp { body_b64, .. } => Budget {
            calls: 1,
            bytes_out: match body_b64 {
                Some(body) => decoded_len(body)?,
                None => 0,
            },
            ..Budget::default()
        },
        ExecOp::BrowserNav { .. } | ExecOp::BrowserScreenshot { .. } => Budget {
            calls: 1,
            ..Budget::default()
        },
        ExecOp::ProcExec { .. } => Budget {
            calls: 1,
            ..Budget::default()
        },
        ExecOp::TmpAlloc { size_bytes } => Budget {
            calls: 1,
            bytes_out: *size_bytes,
            ..Budget::default()
        },
    })
}

fn ensure_grant_active(grant: &Grant) -> Result<(), SandboxError> {
    if grant.expires_at > 0 {
        let now = Utc::now().timestamp_millis();
        if grant.expires_at <= now {
            return Err(SandboxError::permission_denied("grant expired"));
        }
    }
    Ok(())
}

fn ensure_consent(
    grant: &Grant,
    capability: &Capability,
    op: &ExecOp,
    profile: &Profile,
) -> Result<(), SandboxError> {
    if !requires_consent(capability, op, profile) {
        return Ok(());
    }
    let consent = grant
        .consent
        .as_ref()
        .ok_or_else(|| SandboxError::permission_denied("consent required"))?;
    if let Some(expires_at) = consent.expires_at {
        if expires_at.as_millis() <= Utc::now().timestamp_millis() {
            return Err(SandboxError::permission_denied("consent expired"));
        }
    }
    Ok(())
}

fn requires_consent(capability: &Capability, op: &ExecOp, profile: &Profile) -> bool {
    match capability {
        Capability::FsWrite { .. } | Capability::ProcExec { .. } => true,
        Capability::NetHttp { .. } => match op {
            ExecOp::NetHttp { method, .. } => {
                let upper = method.to_uppercase();
                !(upper == "GET" || upper == "HEAD")
            }
            _ => false,
        },
        Capability::TmpUse | Capability::FsRead { .. } | Capability::FsList { .. } => false,
        Capability::BrowserUse { .. } | Capability::SysGpu { .. } => {
            matches!(profile.safety, SafetyClass::High)
        }
    }
}

fn ensure_usage_within_limits(profile: &Profile, usage: &ExecUsage) -> Result<(), SandboxError> {
    if let Some(max) = profile.limits.max_bytes_in {
        if usage.bytes_in > max {
            return Err(SandboxError::policy_violation("bytes_in exceeds limit"));
        }
    }
    if let Some(max) = profile.limits.max_bytes_out {
        if usage.bytes_out > max {
            return Err(SandboxError::policy_violation("bytes_out exceeds limit"));
        }
    }
    if let Some(max) = profile.limits.max_files {
        if usage.file_count > max {
            return Err(SandboxError::policy_violation("file count exceeds limit"));
        }
    }
    Ok(())
}

fn map_error_status(code: &str) -> EvidenceStatus {
    if code.starts_with("AUTH.")
        || code.starts_with("SANDBOX.PERMISSION_DENIED")
        || code.starts_with("POLICY.")
    {
        EvidenceStatus::Denied
    } else {
        EvidenceStatus::Error
    }
}

fn decoded_len(data_b64: &str) -> Result<u64, SandboxError> {
    let bytes = BASE64
        .decode(data_b64)
        .map_err(|_| SandboxError::policy_violation("invalid base64 payload"))?;
    Ok(bytes.len() as u64)
}

impl From<&ExecUsage> for Budget {
    fn from(usage: &ExecUsage) -> Self {
        Budget {
            calls: usage.calls,
            bytes_in: usage.bytes_in,
            bytes_out: usage.bytes_out,
            cpu_ms: usage.cpu_ms,
            file_count: usage.file_count,
            ..Budget::default()
        }
    }
}
