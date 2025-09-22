use sb_sandbox::prelude::{CapabilityKind, SandboxExecutor};
use sb_tools::prelude::*;
use sb_types::prelude::{Id, Subject, SubjectKind, TenantId};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct HttpInput {
    url: String,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct HttpOutput {
    method: String,
    url: String,
}

fn sample_manifest() -> ToolManifest {
    ToolManifest {
        id: ToolId("net.echo.get".into()),
        version: Version::parse("1.0.0").unwrap(),
        display_name: "HTTP Echo".into(),
        description: "Echoes back request metadata".into(),
        tags: vec!["http".into()],
        input_schema: schemars::schema_for!(HttpInput),
        output_schema: schemars::schema_for!(HttpOutput),
        scopes: vec![],
        capabilities: vec![CapabilityDecl {
            domain: "net.http".into(),
            action: "get".into(),
            resource: "example.com".into(),
            attrs: json!({}),
        }],
        side_effect: SideEffect::Read,
        safety_class: SafetyClass::Low,
        consent: ConsentPolicy::default(),
        limits: Limits {
            timeout_ms: 5_000,
            max_bytes_in: 64 * 1024,
            max_bytes_out: 64 * 1024,
            max_files: 0,
            max_depth: 0,
            max_concurrency: 4,
        },
        idempotency: IdempoKind::Keyed,
        concurrency: ConcurrencyKind::Serial,
        metadata: json!({"category":"demo"}),
        compat: Default::default(),
        deprecated: false,
    }
}

fn sample_call() -> ToolCall {
    let tenant = TenantId("tenant-a".into());
    ToolCall {
        tool_id: ToolId("net.echo.get".into()),
        call_id: Id::from("call-1"),
        actor: Subject::new(SubjectKind::Service, Id::from("svc-1"), tenant.clone()),
        tenant,
        origin: ToolOrigin::Llm,
        args: json!({"url": "https://example.com/test"}),
        consent: None,
        idempotency_key: Some("key-123".into()),
    }
}

fn setup_registry(manifest: ToolManifest) -> Arc<InMemoryRegistry> {
    let registry = Arc::new(InMemoryRegistry::new());
    futures::executor::block_on(registry.register(manifest)).expect("register");
    registry
}

#[tokio::test]
async fn register_preflight_invoke_flow() {
    let manifest = sample_manifest();
    let registry = setup_registry(manifest.clone());
    let auth = Arc::new(AllowAllAuth);
    let preflight = PreflightService::new(registry.clone(), auth);

    let call = sample_call();
    let preflight_output = preflight.preflight(&call).await.expect("preflight");
    assert!(preflight_output.allow);
    let plan = preflight_output.plan.expect("plan present");
    assert_eq!(plan.spec.manifest.display_name, "HTTP Echo");

    let sandbox = default_sandbox_with_executors();
    let config = InvokerConfig::with_sandbox(sandbox);
    let invoker = InvokerImpl::new(config);

    let invoke_req = InvokeRequest {
        plan: plan.clone(),
        call: call.clone(),
    };
    let result = invoker.invoke(invoke_req).await.expect("invoke");
    assert!(matches!(result.status, InvokeStatus::Ok));
    let output = result.output.expect("output");
    assert_eq!(output["method"], "GET");
    assert_eq!(output["url"], "https://example.com/test");

    // Second call with same idempotency key should hit cache
    let cached = invoker
        .invoke(InvokeRequest { plan: plan, call })
        .await
        .expect("cached invoke");
    assert!(matches!(cached.status, InvokeStatus::Ok));
}
