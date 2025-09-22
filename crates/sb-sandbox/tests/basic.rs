use async_trait::async_trait;
use sb_sandbox::budget::BudgetMeter;
use sb_sandbox::config::PolicyConfig;
use sb_sandbox::exec::{ExecCtx, ExecOp, ExecResult, ExecUsage, SandboxExecutor};
use sb_sandbox::guard::{DefaultPolicyGuard, PolicyGuard};
use sb_sandbox::manager::{ExecuteRequest, Sandbox};
use sb_sandbox::model::{
    Budget, Capability, CapabilityKind, Grant, Limits, Mappings, SafetyClass, SideEffect,
    SideEffectRecord, ToolManifest, Whitelists,
};
use sb_sandbox::prelude::{
    DefaultProfileBuilder, EvidenceEvent, EvidenceStatus, ProfileBuilder, SandboxError,
};
use sb_types::prelude::{Id, TenantId};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;

fn tenant() -> TenantId {
    TenantId("tenant-A".into())
}

fn subject() -> Id {
    Id("subject-1".into())
}

fn call_id() -> Id {
    Id("call-1".into())
}

fn grant() -> Grant {
    Grant {
        tenant: tenant(),
        subject_id: subject(),
        tool_name: "fetcher".into(),
        call_id: call_id(),
        capabilities: vec![Capability::NetHttp {
            host: "example.com".into(),
            port: None,
            scheme: Some("https".into()),
            methods: vec!["GET".into()],
        }],
        expires_at: 0,
        budget: Budget {
            bytes_in: 1024,
            bytes_out: 2048,
            calls: 1,
            ..Budget::default()
        },
        decision_fingerprint: "fp".into(),
        consent: None,
    }
}

fn manifest() -> ToolManifest {
    ToolManifest {
        name: "fetcher".into(),
        version: "1.0.0".into(),
        capabilities: vec![Capability::NetHttp {
            host: "example.com".into(),
            port: None,
            scheme: Some("https".into()),
            methods: vec!["GET".into()],
        }],
        safety: SafetyClass::Medium,
        side_effects: vec![SideEffect::Network],
        limits: Some(Limits {
            max_bytes_in: Some(4096),
            max_bytes_out: Some(4096),
            max_files: None,
            max_depth: None,
            max_concurrency: None,
        }),
        whitelists: Some(Whitelists {
            domains: vec!["example.com".into()],
            paths: vec![],
            tools: vec![],
            mime_allow: vec![],
            methods: vec!["GET".into()],
        }),
        mappings: None,
        timeout_ms: Some(10_000),
        metadata: serde_json::Value::Null,
    }
}

fn policy() -> PolicyConfig {
    PolicyConfig {
        capabilities: vec![Capability::NetHttp {
            host: "example.com".into(),
            port: None,
            scheme: Some("https".into()),
            methods: vec!["GET".into()],
        }],
        safety_class: SafetyClass::High,
        side_effects: vec![SideEffect::Network],
        limits: None,
        whitelists: Some(Whitelists {
            domains: vec!["example.com".into()],
            paths: vec![],
            tools: vec![],
            mime_allow: vec![],
            methods: vec!["GET".into()],
        }),
        mappings: Some(Mappings {
            root_fs: Some("/sandbox".into()),
            tmp_dir: Some("/sandbox/tmp".into()),
        }),
        timeout_ms: Some(15_000),
        defaults: Default::default(),
        policy_hash: None,
        config_version: None,
        config_hash: None,
    }
}

#[test]
fn build_profile_intersection() {
    let rt = Runtime::new().unwrap();
    let profile = rt
        .block_on(DefaultProfileBuilder::default().build(&grant(), &manifest(), &policy()))
        .expect("profile");

    assert_eq!(profile.capabilities.len(), 1);
    assert_eq!(profile.safety, SafetyClass::High);
    assert!(profile.side_effects.contains(&SideEffect::Network));
    assert!(profile.timeout_ms <= 10_000);
    assert!(!profile.profile_hash.is_empty());
}

#[test]
fn guard_blocks_disallowed_domain() {
    let rt = Runtime::new().unwrap();
    let profile = rt
        .block_on(DefaultProfileBuilder::default().build(&grant(), &manifest(), &policy()))
        .expect("profile");
    let guard = DefaultPolicyGuard::default();

    rt.block_on(async {
        guard
            .validate(
                &profile,
                &Capability::NetHttp {
                    host: "malicious.com".into(),
                    port: None,
                    scheme: Some("https".into()),
                    methods: vec!["GET".into()],
                },
            )
            .await
            .expect_err("should fail");
    });
}

#[derive(Default, Clone)]
struct RecordingMeter {
    reserved: Arc<Mutex<Vec<Budget>>>,
    committed: Arc<Mutex<Vec<Budget>>>,
    rolled_back: Arc<Mutex<Vec<Budget>>>,
}

#[async_trait]
impl BudgetMeter for RecordingMeter {
    async fn reserve(&self, request: &Budget) -> Result<(), SandboxError> {
        self.reserved.lock().unwrap().push(request.clone());
        Ok(())
    }

    async fn commit(&self, used: &Budget) {
        self.committed.lock().unwrap().push(used.clone());
    }

    async fn rollback(&self, used: &Budget) {
        self.rolled_back.lock().unwrap().push(used.clone());
    }
}

#[derive(Default)]
struct TestNetExecutor;

#[async_trait]
impl SandboxExecutor for TestNetExecutor {
    fn kind(&self) -> CapabilityKind {
        CapabilityKind::NetHttp
    }

    async fn execute(&self, ctx: &ExecCtx<'_>, op: ExecOp) -> Result<ExecResult, SandboxError> {
        match op {
            ExecOp::NetHttp { method, url, .. } => Ok(ExecResult::success(
                json!({ "method": method, "url": url }),
                ExecUsage {
                    calls: 1,
                    ..ExecUsage::default()
                },
                vec![SideEffectRecord {
                    kind: SideEffect::Network,
                    meta: json!({
                        "method": "GET",
                        "url": url,
                        "policy_hash": ctx.profile.policy_hash.clone(),
                    }),
                }],
            )),
            _ => Err(SandboxError::policy_violation("unsupported op")),
        }
    }
}

#[test]
fn sandbox_executes_with_evidence() {
    let rt = Runtime::new().unwrap();
    let meter = RecordingMeter::default();
    let sandbox = Sandbox::new(
        DefaultProfileBuilder::default(),
        DefaultPolicyGuard::default(),
        meter.clone(),
    )
    .with_executor(
        CapabilityKind::NetHttp,
        Arc::new(TestNetExecutor::default()),
    );

    let request = ExecuteRequest {
        grant: grant(),
        manifest: manifest(),
        policy: policy(),
        op: ExecOp::NetHttp {
            method: "GET".into(),
            url: "https://example.com/path".into(),
            headers: json!({}),
            body_b64: None,
        },
        envelope_id: Id("env-1".into()),
    };

    let outcome = rt.block_on(sandbox.execute(request)).expect("execute");
    assert!(outcome.result.ok);
    match (&outcome.begin, &outcome.end) {
        (EvidenceEvent::Begin(begin), EvidenceEvent::End(end)) => {
            assert_eq!(begin.tool_name, "fetcher");
            assert_eq!(end.status, EvidenceStatus::Ok);
            assert_eq!(end.error_code, None);
            assert!(end.outputs_digest.is_some());
        }
        _ => panic!("unexpected evidence variants"),
    }

    assert!(!meter.reserved.lock().unwrap().is_empty());
    assert!(!meter.committed.lock().unwrap().is_empty());
}
