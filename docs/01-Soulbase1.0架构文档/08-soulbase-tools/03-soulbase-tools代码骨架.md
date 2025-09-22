下面是 **SB-08-RIS：`soulbase-tools` 最小可运行骨架**。
 它与 SB-08（规约）& SB-08-TD（设计）逐条对齐：提供 **Tool Manifest / Registry / Preflight / Invoker** 的可编译实现、**Manifest→Sandbox ExecOp 映射**、最小的 **JSON-Schema 校验**、与 **Auth / Sandbox / Errors / Observe** 的对接占位；并附带**单测**覆盖“注册→预检→执行→证据”的主路径与幂等缓存。

> 放到 `soul-base/crates/soulbase-tools/` 后直接 `cargo check && cargo test`。
>  说明：为保证零外部依赖，示例执行依赖我们在 SB-06 中的 `soulbase-sandbox` 最小执行器（FS 只读、NET 白名单模拟响应），与在 SB-04 中的 `AuthFacade::minimal()`（本地允许规则 `attrs.allow=true`）。

------

## 目录结构

```
soul-base/
└─ crates/
   └─ soulbase-tools/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ manifest.rs
      │  ├─ registry.rs
      │  ├─ preflight.rs
      │  ├─ mapping.rs
      │  ├─ invoker.rs
      │  ├─ errors.rs
      │  ├─ events.rs
      │  ├─ observe.rs
      │  └─ prelude.rs
      └─ tests/
         └─ basic.rs
```

------

## Cargo.toml

```toml
[package]
name = "soulbase-tools"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "Tool SDK / Manifest / Registry / Invocation Protocol for the Soul platform"
repository = "https://example.com/soul-base"

[features]
default = ["schema_json"]
schema_json = ["schemars", "jsonschema"]
tenant_scoped_registry = []
idempotency_store_redis = []     # 预留（RIS 中仅内存）

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
schemars = { version = "0.8", optional = true, features = ["serde_json"] }
jsonschema = { version = "0.17", optional = true, features = ["draft2020-12"] }
async-trait = "0.1"
thiserror = "1"
parking_lot = "0.12"
ahash = "0.8"
chrono = "0.4"

# 平台内依赖
sb-types = { path = "../sb-types", version = "1.0.0" }
soulbase-errors = { path = "../soulbase-errors", version = "1.0.0" }
soulbase-auth = { path = "../soulbase-auth", version = "1.0.0" }
soulbase-sandbox = { path = "../soulbase-sandbox", version = "1.0.0" }

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
tempfile = "3"
```

------

## src/lib.rs

```rust
pub mod manifest;
pub mod registry;
pub mod preflight;
pub mod mapping;
pub mod invoker;
pub mod errors;
pub mod events;
pub mod observe;
pub mod prelude;

pub use registry::{InMemoryRegistry, ToolRegistry};
pub use invoker::{InvokerImpl, Invoker};
```

------

## src/manifest.rs

```rust
use serde::{Serialize, Deserialize};
use schemars::schema::RootSchema;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolId(pub String); // "<group>.<pkg>.<name>"

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SafetyClass { Low, Medium, High }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SideEffect { None, Read, Write, Network, Filesystem, Browser, Process }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConsentPolicy {
    pub required: bool,
    #[serde(default)]
    pub max_ttl_ms: Option<u64>,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityDecl {
    pub domain: String,     // "fs" | "net.http" | ...
    pub action: String,     // "read" | "get" | ...
    pub resource: String,   // 路径/域名/工具名（前缀或模板）
    #[serde(default)]
    pub attrs: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolManifest {
    pub id: ToolId,
    pub version: String,                // SemVer 字符串（RIS 简化）
    pub display_name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,

    pub input_schema: RootSchema,
    pub output_schema: RootSchema,

    #[serde(default)]
    pub scopes: Vec<sb_types::Scope>,
    #[serde(default)]
    pub capabilities: Vec<CapabilityDecl>,
    pub side_effect: SideEffect,
    pub safety_class: SafetyClass,
    pub consent: ConsentPolicy,

    pub limits: Limits,
    pub idempotency: IdempoKind,
    pub concurrency: ConcurrencyKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdempoKind { Keyed, None }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConcurrencyKind { Serial, Parallel }
```

------

## src/registry.rs

```rust
use crate::manifest::*;
use crate::errors::ToolError;
use parking_lot::RwLock;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolState { Registered, Enabled, Paused, Deprecated }

#[derive(Clone, Debug)]
pub struct RegistryRecord {
    pub manifest: ToolManifest,
    pub state: ToolState,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug)]
pub struct AvailableSpec {
    pub manifest: ToolManifest,
    pub policy_hash: String,
    pub enabled: bool,
    pub visible_to_llm: bool,
    pub safety_class: SafetyClass,
    pub side_effect: SideEffect,
}

#[derive(Clone, Debug, Default)]
pub struct ListFilter {
    pub tags: Vec<String>,
    pub text: Option<String>,
}

#[async_trait::async_trait]
pub trait ToolRegistry: Send + Sync {
    async fn register(&self, m: ToolManifest) -> Result<(), ToolError>;
    async fn update(&self, m: ToolManifest) -> Result<(), ToolError>;
    async fn set_state(&self, id: &ToolId, state: ToolState) -> Result<(), ToolError>;
    async fn get(&self, id: &ToolId, _tenant: &sb_types::TenantId) -> Option<AvailableSpec>;
    async fn list(&self, _tenant: &sb_types::TenantId, filter: ListFilter) -> Vec<AvailableSpec>;
}

pub struct InMemoryRegistry {
    inner: RwLock<HashMap<String, RegistryRecord>>,
}

impl InMemoryRegistry {
    pub fn new() -> Self { Self { inner: RwLock::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl ToolRegistry for InMemoryRegistry {
    async fn register(&self, m: ToolManifest) -> Result<(), ToolError> {
        // 简单校验（RIS）：Schema 可序列化
        let _ = serde_json::to_value(&m.input_schema).map_err(|e| ToolError::schema(&format!("input schema: {e}")))?;
        let _ = serde_json::to_value(&m.output_schema).map_err(|e| ToolError::schema(&format!("output schema: {e}")))?;
        let mut g = self.inner.write();
        let now = chrono::Utc::now().timestamp_millis();
        if g.contains_key(&m.id.0) { return Err(ToolError::policy("manifest already exists")); }
        g.insert(m.id.0.clone(), RegistryRecord { manifest: m, state: ToolState::Enabled, created_at: now, updated_at: now });
        Ok(())
    }

    async fn update(&self, m: ToolManifest) -> Result<(), ToolError> {
        let mut g = self.inner.write();
        let now = chrono::Utc::now().timestamp_millis();
        let rec = g.get_mut(&m.id.0).ok_or_else(|| ToolError::policy("tool not found"))?;
        rec.manifest = m;
        rec.updated_at = now;
        Ok(())
    }

    async fn set_state(&self, id: &ToolId, state: ToolState) -> Result<(), ToolError> {
        let mut g = self.inner.write();
        let rec = g.get_mut(&id.0).ok_or_else(|| ToolError::policy("tool not found"))?;
        rec.state = state;
        rec.updated_at = chrono::Utc::now().timestamp_millis();
        Ok(())
    }

    async fn get(&self, id: &ToolId, _tenant: &sb_types::TenantId) -> Option<AvailableSpec> {
        let g = self.inner.read();
        let r = g.get(&id.0)?.clone();
        Some(AvailableSpec {
            manifest: r.manifest,
            policy_hash: "v1".into(),
            enabled: matches!(r.state, ToolState::Enabled),
            visible_to_llm: true,
            safety_class: SafetyClass::Low,
            side_effect: SideEffect::Read,
        })
    }

    async fn list(&self, _tenant: &sb_types::TenantId, filter: ListFilter) -> Vec<AvailableSpec> {
        let g = self.inner.read();
        g.values().filter_map(|r| {
            if let Some(q) = &filter.text {
                if !r.manifest.display_name.contains(q) && !r.manifest.description.contains(q) { return None; }
            }
            Some(AvailableSpec {
                manifest: r.manifest.clone(),
                policy_hash: "v1".into(),
                enabled: matches!(r.state, ToolState::Enabled),
                visible_to_llm: true,
                safety_class: r.manifest.safety_class,
                side_effect: r.manifest.side_effect,
            })
        }).collect()
    }
}
```

------

## src/preflight.rs

```rust
use crate::{manifest::*, registry::*, mapping::plan_ops, errors::ToolError};
use soulbase_auth::prelude::*;
use soulbase_sandbox::prelude::*;
use sb_types::prelude::*;
use jsonschema::{JSONSchema};
use schemars::schema::RootSchema;

#[derive(Clone, Debug)]
pub enum ToolOrigin { Llm, Api, System }

#[derive(Clone, Debug)]
pub struct ToolCall {
    pub tool_id: ToolId,
    pub call_id: Id,
    pub actor: Subject,
    pub tenant: TenantId,
    pub origin: ToolOrigin,
    pub args: serde_json::Value,
    pub consent: Option<Consent>,
    pub idempotency_key: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct PreflightOutput {
    pub allow: bool,
    pub reason: Option<String>,
    pub profile_hash: Option<String>,
    pub obligations: Vec<Obligation>,
    pub budget_snapshot: serde_json::Value,
}

pub struct Preflight<'a> {
    pub registry: &'a dyn ToolRegistry,
    pub auth: &'a AuthFacade,
    pub policy: PolicyConfig,
    pub guard: &'a dyn PolicyGuard,
}

impl<'a> Preflight<'a> {
    fn validate_json(schema: &RootSchema, v: &serde_json::Value) -> Result<(), ToolError> {
        let sv = serde_json::to_value(schema).map_err(|e| ToolError::schema(&format!("schema serialize: {e}")))?;
        let compiled = JSONSchema::options().with_draft(jsonschema::Draft::Draft202012).compile(&sv)
            .map_err(|e| ToolError::schema(&format!("schema compile: {e}")))?;
        compiled.validate(v).map_err(|errs| {
            let msg = errs.map(|e| e.to_string()).collect::<Vec<_>>().join("; ");
            ToolError::schema(&msg)
        })?;
        Ok(())
    }

    pub async fn run(&self, call: &ToolCall) -> Result<PreflightOutput, ToolError> {
        // 1) 发现
        let spec = self.registry.get(&call.tool_id, &call.tenant).await
            .ok_or_else(|| ToolError::policy("tool not found"))?;
        if !spec.enabled { return Ok(PreflightOutput { allow:false, reason:Some("tool disabled".into()), ..Default::default() }); }

        // 2) 输入校验
        Self::validate_json(&spec.manifest.input_schema, &call.args)?;

        // 3) 授权+配额（最小实现：attrs.allow=true 通过 LocalAuthorizer）
        let attrs = serde_json::json!({"allow": true, "cost": 1});
        let _decision = self.auth.authorize(
            AuthnInput::BearerJwt(format!("{}@{}", call.actor.subject_id.0, call.tenant.0)), // RIS: 使用 stub
            ResourceUrn(format!("soul:tool:{}", spec.manifest.id.0)),
            Action::Invoke,
            attrs,
            call.consent.clone(),
            None,
        ).await.map_err(|e| ToolError::forbidden(&e.into_inner().to_public().message))?;

        // 4) Profile 合成（RIS：以 Manifest ∩ PolicyConfig 构造）
        let grant = soulbase_sandbox::prelude::Grant {
            tenant: call.tenant.clone(),
            subject_id: call.actor.subject_id.clone(),
            tool_name: spec.manifest.id.0.clone(),
            call_id: call.call_id.clone(),
            capabilities: spec.manifest.capabilities.iter().map(|c| {
                match (c.domain.as_str(), c.action.as_str()) {
                    ("net.http","get") => Capability::NetHttp { host: c.resource.clone(), port: None, scheme: Some("https".into()), methods: vec!["GET".into(), "HEAD".into()] },
                    ("fs","read") => Capability::FsRead { path: c.resource.clone() },
                    _ => Capability::TmpUse,
                }
            }).collect(),
            expires_at: chrono::Utc::now().timestamp_millis() + 60_000,
            budget: Budget { calls: 10, bytes_in: 1_048_576, bytes_out: 1_048_576, ..Default::default() },
            decision_key_fingerprint: "dk".into(),
        };

        let profile = ProfileBuilderDefault.build(&grant, &to_lite_manifest(&spec.manifest), &self.policy).await
            .map_err(|e| ToolError::policy(&e.into_inner().to_public().message))?;

        // 5) Guard 预检（基于映射出的计划操作）
        let ops = plan_ops(&spec.manifest, &call.args)?;
        for op in &ops { self.guard.validate(&profile, op).await.map_err(|e| ToolError::sandbox(&e.into_inner().to_public().message))?; }

        Ok(PreflightOutput {
            allow: true,
            reason: None,
            profile_hash: Some(profile.profile_hash.clone()),
            obligations: vec![],
            budget_snapshot: serde_json::json!({"calls": grant.budget.calls, "bytes_in": grant.budget.bytes_in}),
        })
    }
}

fn to_lite_manifest(m: &ToolManifest) -> soulbase_sandbox::prelude::ToolManifestLite {
    soulbase_sandbox::prelude::ToolManifestLite {
        name: m.id.0.clone(),
        permissions: m.capabilities.iter().map(|c| {
            match (c.domain.as_str(), c.action.as_str()) {
                ("net.http","get") => Capability::NetHttp { host: c.resource.clone(), port: None, scheme: Some("https".into()), methods: vec!["GET".into(), "HEAD".into()] },
                ("fs","read") => Capability::FsRead { path: c.resource.clone() },
                _ => Capability::TmpUse,
            }
        }).collect(),
        safety_class: match m.safety_class { SafetyClass::Low=>soulbase_sandbox::prelude::SafetyClass::Low, SafetyClass::Medium=>soulbase_sandbox::prelude::SafetyClass::Medium, SafetyClass::High=>soulbase_sandbox::prelude::SafetyClass::High },
        side_effect: match m.side_effect {
            SideEffect::None=>soulbase_sandbox::prelude::SideEffect::None,
            SideEffect::Read=>soulbase_sandbox::prelude::SideEffect::Read,
            SideEffect::Write=>soulbase_sandbox::prelude::SideEffect::Write,
            SideEffect::Network=>soulbase_sandbox::prelude::SideEffect::Network,
            SideEffect::Filesystem=>soulbase_sandbox::prelude::SideEffect::Filesystem,
            SideEffect::Browser=>soulbase_sandbox::prelude::SideEffect::Read,
            SideEffect::Process=>soulbase_sandbox::prelude::SideEffect::Process,
        },
    }
}
```

------

## src/mapping.rs

```rust
use crate::manifest::*;
use crate::errors::ToolError;
use soulbase_sandbox::prelude::ExecOp;

/// 从 Manifest + args 规划将要执行的受控操作（RIS：最小映射）
/// - net.http:get  → 需要 args.url (string)
/// - fs.read       → 需要 args.path (string)
pub fn plan_ops(m: &ToolManifest, args: &serde_json::Value) -> Result<Vec<ExecOp>, ToolError> {
    let mut ops = Vec::new();
    for cap in &m.capabilities {
        match (cap.domain.as_str(), cap.action.as_str()) {
            ("net.http", "get") => {
                let url = args.get("url").and_then(|v| v.as_str()).ok_or_else(|| ToolError::schema("missing args.url"))?;
                ops.push(ExecOp::NetHttp { method: "GET".into(), url: url.into(), headers: Default::default(), body_b64: None });
            }
            ("fs", "read") => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| ToolError::schema("missing args.path"))?;
                let len = args.get("len").and_then(|v| v.as_u64());
                ops.push(ExecOp::FsRead { path: path.into(), offset: None, len });
            }
            ("tmp", "use") => {
                let size = args.get("size_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
                ops.push(ExecOp::TmpAlloc { size_bytes: size });
            }
            _ => {}
        }
    }
    Ok(ops)
}
```

------

## src/invoker.rs

```rust
use crate::{registry::*, manifest::*, preflight::*, mapping::plan_ops, errors::ToolError};
use soulbase_sandbox::prelude::*;
use soulbase_auth::prelude::*;
use sb_types::prelude::*;
use std::collections::HashMap;
use parking_lot::Mutex;

#[derive(Clone, Debug)]
pub enum InvokeStatus { Ok, Denied, Error }

#[derive(Clone, Debug)]
pub struct InvokeRequest {
    pub spec: AvailableSpec,
    pub call: ToolCall,
    pub profile_hash: String,
    pub obligations: Vec<Obligation>,
}

#[derive(Clone, Debug)]
pub struct InvokeResult {
    pub status: InvokeStatus,
    pub error_code: Option<&'static str>,
    pub output: Option<serde_json::Value>,
    pub evidence_ref: Option<Id>,
}

#[async_trait::async_trait]
pub trait Invoker: Send + Sync {
    async fn preflight(&self, call: &ToolCall) -> Result<PreflightOutput, ToolError>;
    async fn invoke(&self, req: InvokeRequest) -> Result<InvokeResult, ToolError>;
}

pub struct IdemStoreMemory {
    inner: Mutex<HashMap<(String,String,String), serde_json::Value>>, // (tenant, tool_id, idem_key) -> output
}
impl IdemStoreMemory { pub fn new() -> Self { Self { inner: Mutex::new(HashMap::new()) } } }

pub struct InvokerImpl<R: ToolRegistry> {
    pub registry: R,
    pub auth: AuthFacade,
    pub sandbox: Sandbox,
    pub guard: Box<dyn PolicyGuard>,
    pub policy: PolicyConfig,
    pub idem: IdemStoreMemory,
}

#[async_trait::async_trait]
impl<R: ToolRegistry + Send + Sync> Invoker for InvokerImpl<R> {
    async fn preflight(&self, call: &ToolCall) -> Result<PreflightOutput, ToolError> {
        let pre = Preflight {
            registry: &self.registry,
            auth: &self.auth,
            policy: self.policy.clone(),
            guard: self.guard.as_ref(),
        };
        pre.run(call).await
    }

    async fn invoke(&self, req: InvokeRequest) -> Result<InvokeResult, ToolError> {
        // 幂等命中
        if let Some(key) = &req.call.idempotency_key {
            let mut g = self.idem.inner.lock();
            if let Some(v) = g.get(&(req.call.tenant.0.clone(), req.spec.manifest.id.0.clone(), key.clone())).cloned() {
                return Ok(InvokeResult { status: InvokeStatus::Ok, error_code: None, output: Some(v), evidence_ref: None });
            }
        }

        // Profile（RIS：重建一次，实际可使用 preflight 的 hash 查缓存）
        let grant = soulbase_sandbox::prelude::Grant {
            tenant: req.call.tenant.clone(),
            subject_id: req.call.actor.subject_id.clone(),
            tool_name: req.spec.manifest.id.0.clone(),
            call_id: req.call.call_id.clone(),
            capabilities: req.spec.manifest.capabilities.iter().map(|c| {
                match (c.domain.as_str(), c.action.as_str()) {
                    ("net.http","get") => Capability::NetHttp { host: c.resource.clone(), port: None, scheme: Some("https".into()), methods: vec!["GET".into(), "HEAD".into()] },
                    ("fs","read") => Capability::FsRead { path: c.resource.clone() },
                    _ => Capability::TmpUse,
                }
            }).collect(),
            expires_at: chrono::Utc::now().timestamp_millis() + 60_000,
            budget: Budget { calls: 10, bytes_in: 1_048_576, bytes_out: 1_048_576, ..Default::default() },
            decision_key_fingerprint: "dk".into(),
        };
        let profile = ProfileBuilderDefault.build(&grant, &to_lite_manifest(&req.spec.manifest), &self.policy).await
            .map_err(|e| ToolError::policy(&e.into_inner().to_public().message))?;

        // 计划操作与 Guard
        let ops = plan_ops(&req.spec.manifest, &req.call.args)?;
        for op in &ops { self.guard.validate(&profile, op).await.map_err(|e| ToolError::sandbox(&e.into_inner().to_public().message))?; }

        // Evidence sink / budget
        let evidence = MemoryEvidence::new();
        let budget = MemoryBudget::new(grant.budget.clone());
        let env_id = Id(format!("env_{}", req.call.call_id.0));

        // 执行（顺序执行）
        let mut outputs = Vec::<serde_json::Value>::new();
        for op in ops {
            let res = self.sandbox.run(&profile, &env_id, &evidence, &budget, op).await;
            match res {
                Ok(ok) => outputs.push(ok.out),
                Err(e) => {
                    let eo = e.into_inner();
                    return Ok(InvokeResult { status: InvokeStatus::Error, error_code: Some(eo.code.0), output: None, evidence_ref: Some(env_id) });
                }
            }
        }

        // 聚合输出 & 校验（RIS：若有 output_schema，做 JSON-Schema 校验）
        let output = if outputs.len() == 1 { outputs.remove(0) } else { serde_json::json!({ "results": outputs }) };
        validate_out_schema(&req.spec.manifest.output_schema, &output)?;

        // 幂等写入
        if let Some(key) = &req.call.idempotency_key {
            let mut g = self.idem.inner.lock();
            g.insert((req.call.tenant.0.clone(), req.spec.manifest.id.0.clone(), key.clone()), output.clone());
        }

        Ok(InvokeResult { status: InvokeStatus::Ok, error_code: None, output: Some(output), evidence_ref: Some(env_id) })
    }
}

fn to_lite_manifest(m: &ToolManifest) -> soulbase_sandbox::prelude::ToolManifestLite {
    soulbase_sandbox::prelude::ToolManifestLite {
        name: m.id.0.clone(),
        permissions: m.capabilities.iter().map(|c| {
            match (c.domain.as_str(), c.action.as_str()) {
                ("net.http","get") => Capability::NetHttp { host: c.resource.clone(), port: None, scheme: Some("https".into()), methods: vec!["GET".into(), "HEAD".into()] },
                ("fs","read") => Capability::FsRead { path: c.resource.clone() },
                _ => Capability::TmpUse,
            }
        }).collect(),
        safety_class: match m.safety_class { SafetyClass::Low=>soulbase_sandbox::prelude::SafetyClass::Low, SafetyClass::Medium=>soulbase_sandbox::prelude::SafetyClass::Medium, SafetyClass::High=>soulbase_sandbox::prelude::SafetyClass::High },
        side_effect: match m.side_effect {
            SideEffect::None=>soulbase_sandbox::prelude::SideEffect::None,
            SideEffect::Read=>soulbase_sandbox::prelude::SideEffect::Read,
            SideEffect::Write=>soulbase_sandbox::prelude::SideEffect::Write,
            SideEffect::Network=>soulbase_sandbox::prelude::SideEffect::Network,
            SideEffect::Filesystem=>soulbase_sandbox::prelude::SideEffect::Filesystem,
            SideEffect::Browser=>soulbase_sandbox::prelude::SideEffect::Read,
            SideEffect::Process=>soulbase_sandbox::prelude::SideEffect::Process,
        },
    }
}

fn validate_out_schema(schema: &schemars::schema::RootSchema, v: &serde_json::Value) -> Result<(), ToolError> {
    let sv = serde_json::to_value(schema).map_err(|e| ToolError::schema(&format!("schema serialize: {e}")))?;
    let compiled = jsonschema::JSONSchema::options().with_draft(jsonschema::Draft::Draft202012).compile(&sv)
        .map_err(|e| ToolError::schema(&format!("schema compile: {e}")))?;
    compiled.validate(v).map_err(|errs| {
        let msg = errs.map(|e| e.to_string()).collect::<Vec<_>>().join("; ");
        ToolError::schema(&msg)
    })?;
    Ok(())
}
```

------

## src/errors.rs

```rust
use thiserror::Error;
use soulbase_errors::prelude::*;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct ToolError(pub ErrorObj);

impl ToolError {
    pub fn into_inner(self) -> ErrorObj { self.0 }
    pub fn schema(msg: &str) -> Self {
        ToolError(ErrorBuilder::new(codes::SCHEMA_VALIDATION).user_msg("Invalid tool schema or arguments.").dev_msg(msg).build())
    }
    pub fn policy(msg: &str) -> Self {
        ToolError(ErrorBuilder::new(codes::POLICY_DENY_TOOL).user_msg("Tool operation denied by policy.").dev_msg(msg).build())
    }
    pub fn forbidden(msg: &str) -> Self {
        ToolError(ErrorBuilder::new(codes::AUTH_FORBIDDEN).user_msg("Forbidden.").dev_msg(msg).build())
    }
    pub fn quota(msg: &str) -> Self {
        ToolError(ErrorBuilder::new(codes::QUOTA_BUDGET).user_msg("Budget exceeded.").dev_msg(msg).build())
    }
    pub fn sandbox(msg: &str) -> Self {
        // 若你的错误码表已有 SANDBOX.CAPABILITY_BLOCKED，可替换为该码
        ToolError(ErrorBuilder::new(codes::POLICY_DENY_TOOL).user_msg("Sandbox capability blocked.").dev_msg(msg).build())
    }
    pub fn provider(msg: &str) -> Self {
        ToolError(ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE).user_msg("Upstream provider unavailable.").dev_msg(msg).build())
    }
    pub fn unknown(msg: &str) -> Self {
        ToolError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL).user_msg("Internal error.").dev_msg(msg).build())
    }
}
```

------

## src/events.rs

```rust
use serde::{Serialize, Deserialize};
use crate::manifest::ToolId;
use sb_types::prelude::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolRegistered {
    pub tool_id: ToolId,
    pub tenant: TenantId,
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolInvokeBegin {
    pub envelope_id: Id,
    pub tenant: TenantId,
    pub subject_id: Id,
    pub tool_id: ToolId,
    pub call_id: Id,
    pub profile_hash: String,
    pub args_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolInvokeEnd {
    pub envelope_id: Id,
    pub status: String, // "ok"|"denied"|"error"
    pub error_code: Option<&'static str>,
    pub budget_used_bytes_in: u64,
    pub budget_used_bytes_out: u64,
    pub output_digest: String,
}
```

------

## src/observe.rs

```rust
use std::collections::BTreeMap;
use crate::manifest::ToolId;

pub fn labels(tenant: &str, tool: &ToolId, code: Option<&str>) -> BTreeMap<&'static str, String> {
    let mut m = BTreeMap::new();
    m.insert("tenant", tenant.to_string());
    m.insert("tool_id", tool.0.clone());
    if let Some(c) = code { m.insert("code", c.to_string()); }
    m
}
```

------

## src/prelude.rs

```rust
pub use crate::manifest::{ToolId, ToolManifest, CapabilityDecl, SafetyClass, SideEffect, ConsentPolicy, Limits, IdempoKind, ConcurrencyKind};
pub use crate::registry::{ToolRegistry, InMemoryRegistry, ToolState, AvailableSpec, ListFilter};
pub use crate::preflight::{ToolCall, ToolOrigin, PreflightOutput, Preflight};
pub use crate::invoker::{Invoker, InvokerImpl, InvokeRequest, InvokeResult, InvokeStatus};
pub use crate::errors::ToolError;
```

------

## tests/basic.rs

```rust
use soulbase_tools::prelude::*;
use soulbase_auth::prelude::*;
use soulbase_sandbox::prelude::*;
use sb_types::prelude::*;
use schemars::{schema_for, JsonSchema};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, JsonSchema)]
struct NetInput { url: String }
#[derive(Serialize, Deserialize, JsonSchema)]
struct NetOutput { url: String, simulated: bool }

fn manifest_net_get() -> ToolManifest {
    ToolManifest {
        id: ToolId("net.http.get".into()),
        version: "1.0.0".into(),
        display_name: "HTTP GET".into(),
        description: "Fetch a URL via sandboxed GET".into(),
        tags: vec!["net".into(),"http".into()],
        input_schema: schema_for!(NetInput),
        output_schema: schema_for!(NetOutput),
        scopes: vec![],
        capabilities: vec![CapabilityDecl { domain:"net.http".into(), action:"get".into(), resource:"example.com".into(), attrs: serde_json::json!({}) }],
        side_effect: SideEffect::Network,
        safety_class: SafetyClass::Medium,
        consent: ConsentPolicy { required: false, max_ttl_ms: None },
        limits: Limits { timeout_ms: 10_000, max_bytes_in: 2_000_000, max_bytes_out: 1_000_000, max_files: 0, max_depth: 2, max_concurrency: 2 },
        idempotency: IdempoKind::Keyed,
        concurrency: ConcurrencyKind::Parallel,
    }
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct FsInput { path: String }
#[derive(Serialize, Deserialize, JsonSchema)]
struct FsOutput { size: u64 }

fn manifest_fs_read(root: &str) -> ToolManifest {
    ToolManifest {
        id: ToolId("fs.read".into()),
        version: "1.0.0".into(),
        display_name: "FS Read".into(),
        description: "Read file via sandbox".into(),
        tags: vec!["fs".into()],
        input_schema: schema_for!(FsInput),
        output_schema: schema_for!(FsOutput),
        scopes: vec![],
        capabilities: vec![CapabilityDecl { domain:"fs".into(), action:"read".into(), resource:root.into(), attrs: serde_json::json!({}) }],
        side_effect: SideEffect::Read,
        safety_class: SafetyClass::Low,
        consent: ConsentPolicy { required: false, max_ttl_ms: None },
        limits: Limits { timeout_ms: 10_000, max_bytes_in: 1_000_000, max_bytes_out: 0, max_files: 0, max_depth: 2, max_concurrency: 1 },
        idempotency: IdempoKind::
```
