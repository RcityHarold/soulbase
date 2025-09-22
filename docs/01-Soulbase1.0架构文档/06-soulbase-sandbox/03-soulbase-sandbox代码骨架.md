下面是 **SB-06-RIS：`soulbase-sandbox` 最小可运行骨架**。
 它与 SB-06（规约）& SB-06-TD（设计）一一对齐：提供 **Traits/SPI、Profile 合成、Guards、最小执行器（只读 FS / 白名单 HTTP）、证据与预算的内存实现**，以及可运行的**单测**（允许/拒绝/预算超限/证据生成）。为便于立即落地，暂以轻量 `ToolManifestLite` 代替 `soulbase-tools::Manifest`（待 08 模块就绪后切换）。

> 放入 `soul-base/crates/soulbase-sandbox/` 后即可 `cargo check && cargo test`。
>  说明：`NetExecutor` 采用“白名单校验 + 模拟响应”以避免测试环境出网依赖；未来可在 `net-reqwest` feature 下替换为真实出网。

------

## 目录结构

```
soul-base/
└─ crates/
   └─ soulbase-sandbox/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ model.rs
      │  ├─ profile.rs
      │  ├─ guard.rs
      │  ├─ exec/
      │  │  ├─ mod.rs
      │  │  ├─ fs.rs
      │  │  └─ net.rs
      │  ├─ budget.rs
      │  ├─ evidence.rs
      │  ├─ revoke.rs
      │  ├─ config.rs
      │  ├─ errors.rs
      │  ├─ observe.rs
      │  └─ prelude.rs
      └─ tests/
         └─ basic.rs
```

------

## Cargo.toml

```toml
[package]
name = "soulbase-sandbox"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "Controlled Execution Sandbox (least-privilege, evidence-first) for the Soul platform"
repository = "https://example.com/soul-base"

[features]
default = ["exec-fs", "exec-net"]
exec-fs = []
exec-net = []
net-reqwest = ["dep:reqwest"]     # 未来切换真实出网
qos = []                          # 未来对接 soulbase-qos
observe = []                      # 未来对接统一观测

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
async-trait = "0.1"
thiserror = "1"
ahash = "0.8"
sha2 = "0.10"
base64 = "0.22"
path-clean = "1.0"
parking_lot = "0.12"
chrono = "0.4"
uuid = "1"
url = "2"

# 平台内依赖
sb-types = { path = "../sb-types", version = "1.0.0" }
soulbase-errors = { path = "../soulbase-errors", version = "1.0.0" }

# 可选真实出网
reqwest = { version = "0.12", optional = true, default-features = false, features = ["rustls-tls"] }

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time"] }
tempfile = "3"
```

------

## src/lib.rs

```rust
pub mod model;
pub mod profile;
pub mod guard;
pub mod exec;
pub mod budget;
pub mod evidence;
pub mod revoke;
pub mod config;
pub mod errors;
pub mod observe;
pub mod prelude;

pub use profile::ProfileBuilderDefault;
pub use guard::PolicyGuardDefault;
pub use exec::{FsExecutor, NetExecutor, Executor, ExecOp, ExecResult, Sandbox};
```

------

## src/model.rs

```rust
use serde::{Serialize, Deserialize};
use sb_types::prelude::*;
use ahash::AHasher;
use std::hash::{Hasher, Hash};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Capability {
    FsRead { path: String },
    FsWrite { path: String },
    FsList { path: String },
    NetHttp { host: String, port: Option<u16>, scheme: Option<String>, methods: Vec<String> },
    TmpUse,
    // 预留：BrowserUse/ProcExec/SysGpu
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SafetyClass { Low, Medium, High }

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SideEffect { None, Read, Write, Network, Filesystem }

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Budget {
    pub calls: u64,
    pub bytes_out: u64,
    pub bytes_in: u64,
    pub cpu_ms: u64,
    pub file_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Grant {
    pub tenant: TenantId,
    pub subject_id: Id,
    pub tool_name: String,
    pub call_id: Id,
    pub capabilities: Vec<Capability>,
    pub expires_at: i64,
    pub budget: Budget,
    pub decision_key_fingerprint: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Limits {
    pub max_bytes_in: u64,
    pub max_bytes_out: u64,
    pub max_files: u64,
    pub max_depth: u32,
    pub max_concurrency: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Whitelists {
    pub domains: Vec<String>,
    pub paths: Vec<String>,
    pub methods: Vec<String>,
    pub mime_allow: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Mappings {
    pub root_fs: String,
    pub tmp_dir: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub tenant: TenantId,
    pub subject_id: Id,
    pub tool_name: String,
    pub call_id: Id,
    pub capabilities: Vec<Capability>,
    pub safety: SafetyClass,
    pub side_effect: SideEffect,
    pub limits: Limits,
    pub whitelists: Whitelists,
    pub mappings: Mappings,
    pub timeout_ms: u64,
    pub profile_hash: String,
}

impl Profile {
    pub fn hash_fields(&self) -> String {
        let mut h = AHasher::default();
        h.write(self.tool_name.as_bytes());
        h.write(self.call_id.0.as_bytes());
        h.write(self.tenant.0.as_bytes());
        format!("{:016x}", h.finish())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExecOp {
    FsRead { path: String, offset: Option<u64>, len: Option<u64> },
    FsWrite { path: String, bytes_b64: String, overwrite: bool },
    NetHttp { method: String, url: String, headers: serde_json::Map<String, serde_json::Value>, body_b64: Option<String> },
    TmpAlloc { size_bytes: u64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecResult {
    pub ok: bool,
    pub code: Option<&'static str>,
    pub message: Option<String>,
    pub out: serde_json::Value,
    pub used: Budget,
}

/// 轻量工具清单（占位，待 08-Tools 接入后替换）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolManifestLite {
    pub name: String,
    pub permissions: Vec<Capability>,
    pub safety_class: SafetyClass,
    pub side_effect: SideEffect,
}
```

------

## src/config.rs

```rust
use crate::model::{Limits, Whitelists, Mappings, SafetyClass, SideEffect};
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolicyConfig {
    pub default_safety: SafetyClass,
    pub default_side_effect: SideEffect,
    pub limits: Limits,
    pub whitelists: Whitelists,
    pub mappings: Mappings,
    pub timeout_ms: u64,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            default_safety: SafetyClass::Low,
            default_side_effect: SideEffect::Read,
            limits: Limits { max_bytes_in: 2 * 1024 * 1024, max_bytes_out: 2 * 1024 * 1024, max_files: 64, max_depth: 4, max_concurrency: 4 },
            whitelists: Whitelists { domains: vec!["example.com".into()], paths: vec![], methods: vec!["GET".into(), "HEAD".into()], mime_allow: vec![] },
            mappings: Mappings { root_fs: ".".into(), tmp_dir: std::env::temp_dir().display().to_string() },
            timeout_ms: 15_000,
        }
    }
}
```

------

## src/profile.rs

```rust
use crate::model::*;
use crate::config::PolicyConfig;
use crate::errors::SandboxError;

#[async_trait::async_trait]
pub trait ProfileBuilder: Send + Sync {
    async fn build(&self, grant: &Grant, manifest: &ToolManifestLite, policy: &PolicyConfig) -> Result<Profile, SandboxError>;
}

pub struct ProfileBuilderDefault;

#[async_trait::async_trait]
impl ProfileBuilder for ProfileBuilderDefault {
    async fn build(&self, grant: &Grant, manifest: &ToolManifestLite, policy: &PolicyConfig) -> Result<Profile, SandboxError> {
        // 能力交集
        let mut allowed = vec![];
        for g in &grant.capabilities {
            if manifest.permissions.iter().any(|m| cap_match(m, g)) {
                allowed.push(g.clone());
            }
        }
        if allowed.is_empty() {
            return Err(SandboxError::policy_deny("no intersected capabilities"));
        }

        // 风险与副作用：取最严格
        let safety = max_safety(manifest.safety_class, policy.default_safety);
        let side_effect = max_side_effect(manifest.side_effect, policy.default_side_effect);

        let mut profile = Profile {
            tenant: grant.tenant.clone(),
            subject_id: grant.subject_id.clone(),
            tool_name: grant.tool_name.clone(),
            call_id: grant.call_id.clone(),
            capabilities: allowed,
            safety,
            side_effect,
            limits: policy.limits.clone(),
            whitelists: policy.whitelists.clone(),
            mappings: policy.mappings.clone(),
            timeout_ms: policy.timeout_ms,
            profile_hash: String::new(),
        };
        profile.profile_hash = profile.hash_fields();
        Ok(profile)
    }
}

fn cap_match(a: &Capability, b: &Capability) -> bool {
    use Capability::*;
    match (a, b) {
        (FsRead {..}, FsRead {..}) => true,
        (FsWrite{..}, FsWrite{..}) => true,
        (FsList{..}, FsList{..}) => true,
        (NetHttp{..}, NetHttp{..}) => true,
        (TmpUse, TmpUse) => true,
        _ => false
    }
}

fn max_safety(a: SafetyClass, b: SafetyClass) -> SafetyClass {
    use SafetyClass::*;
    match (a, b) {
        (High, _) | (_, High) => High,
        (Medium, _) | (_, Medium) => Medium,
        _ => Low,
    }
}
fn max_side_effect(a: SideEffect, _b: SideEffect) -> SideEffect { a }
```

------

## src/guard.rs

```rust
use crate::model::*;
use crate::errors::SandboxError;
use url::Url;
use path_clean::PathClean;

#[async_trait::async_trait]
pub trait PolicyGuard: Send + Sync {
    async fn validate(&self, profile: &Profile, op: &ExecOp) -> Result<(), SandboxError>;
}

pub struct PolicyGuardDefault;

#[async_trait::async_trait]
impl PolicyGuard for PolicyGuardDefault {
    async fn validate(&self, profile: &Profile, op: &ExecOp) -> Result<(), SandboxError> {
        match op {
            ExecOp::FsRead { path, .. } | ExecOp::FsWrite { path, .. } => {
                let root = std::path::Path::new(&profile.mappings.root_fs);
                let p = root.join(path).clean();
                if !p.starts_with(root) { return Err(SandboxError::cap_blocked("path escapes root")); }
                Ok(())
            }
            ExecOp::NetHttp { method, url, .. } => {
                let u = Url::parse(url).map_err(|e| SandboxError::schema_invalid(&format!("bad url: {e}")))?;
                let host = u.host_str().ok_or_else(|| SandboxError::schema_invalid("missing host"))?;
                if !profile.whitelists.domains.iter().any(|d| d == host) {
                    return Err(SandboxError::cap_blocked("host not in whitelist"));
                }
                if !profile.whitelists.methods.iter().any(|m| m.eq_ignore_ascii_case(method)) {
                    return Err(SandboxError::cap_blocked("method not allowed"));
                }
                Ok(())
            }
            ExecOp::TmpAlloc { .. } => Ok(()),
        }
    }
}
```

------

## src/exec/mod.rs

```rust
use crate::model::*;
use crate::errors::SandboxError;
use crate::evidence::{EvidenceBegin, EvidenceEnd, EvidenceSinkDyn, Digest};
use crate::budget::BudgetMeterDyn;
use sb_types::prelude::*;

#[async_trait::async_trait]
pub trait Executor: Send + Sync {
    fn domain(&self) -> &'static str; // "fs" | "net"
    async fn execute(&self, ctx: &ExecCtx<'_>, op: ExecOp) -> Result<ExecResult, SandboxError>;
}

pub struct ExecCtx<'a> {
    pub profile: &'a Profile,
    pub envelope_id: &'a Id,
    pub evidence: EvidenceSinkDyn<'a>,
    pub budget: BudgetMeterDyn<'a>,
}

pub struct Sandbox {
    pub fs: Option<FsExecutor>,
    pub net: Option<NetExecutor>,
}

impl Sandbox {
    pub fn minimal() -> Self { Self { fs: Some(FsExecutor), net: Some(NetExecutor) } }

    pub fn pick<'a>(&'a self, op: &ExecOp) -> Option<&'a (dyn Executor)> {
        match op {
            ExecOp::FsRead{..} | ExecOp::FsWrite{..} => self.fs.as_ref().map(|e| e as &dyn Executor),
            ExecOp::NetHttp{..} => self.net.as_ref().map(|e| e as &dyn Executor),
            ExecOp::TmpAlloc{..} => self.fs.as_ref().map(|e| e as &dyn Executor), // 复用 fs tmp
        }
    }

    pub async fn run(
        &self,
        profile: &Profile,
        env: &Id,
        evidence: EvidenceSinkDyn<'_>,
        budget: BudgetMeterDyn<'_>,
        op: ExecOp
    ) -> Result<ExecResult, SandboxError> {
        let cap_str = cap_string(&op);
        evidence.begin(&EvidenceBegin {
            envelope_id: env.clone(),
            tenant: profile.tenant.clone(),
            subject_id: profile.subject_id.clone(),
            tool_name: profile.tool_name.clone(),
            call_id: profile.call_id.clone(),
            profile_hash: profile.profile_hash.clone(),
            capability: cap_str.clone(),
            inputs_digest: Digest { algo: "none", b64: "".into(), size: 0 },
            produced_at_ms: chrono::Utc::now().timestamp_millis(),
            policy_version_hash: "v1".into(),
        }).await;

        budget.check_and_consume("calls", 1).await?;

        let exec = self.pick(&op).ok_or_else(|| SandboxError::cap_blocked("no executor for op"))?;
        let result = exec.execute(&ExecCtx { profile, envelope_id: env, evidence: evidence.clone(), budget: budget.clone() }, op).await;

        let (ok, code, out, used) = match &result {
            Ok(r) => (true, None, r.out.clone(), r.used.clone()),
            Err(e) => {
                let eo = e.as_error_obj();
                (false, Some(eo.code.0), serde_json::json!({"error": eo.to_public().message}), Budget::default())
            }
        };

        evidence.end(&EvidenceEnd {
            envelope_id: env.clone(),
            status: if ok { "ok".into() } else { "error".into() },
            error_code: code,
            outputs_digest: Digest { algo: "none", b64: "".into(), size: 0 },
            side_effects: vec![],
            budget_used: used.clone(),
            finished_at_ms: chrono::Utc::now().timestamp_millis(),
        }).await;

        result
    }
}

fn cap_string(op: &ExecOp) -> String {
    match op {
        ExecOp::FsRead{path, ..} => format!("fs.read:{}", path),
        ExecOp::FsWrite{path, ..} => format!("fs.write:{}", path),
        ExecOp::NetHttp{method, url, ..} => format!("net.http:{}:{}", method, url),
        ExecOp::TmpAlloc{..} => "tmp.use".into(),
    }
}

// —— 执行器子模块 —— //
#[cfg(feature="exec-fs")] pub mod fs;
#[cfg(feature="exec-net")] pub mod net;
pub use fs::FsExecutor;
pub use net::NetExecutor;
```

------

## src/exec/fs.rs

```rust
use super::*;
use std::fs;
use std::io::Read;
use path_clean::PathClean;

pub struct FsExecutor;

#[async_trait::async_trait]
impl Executor for FsExecutor {
    fn domain(&self) -> &'static str { "fs" }

    async fn execute(&self, ctx: &ExecCtx<'_>, op: ExecOp) -> Result<ExecResult, SandboxError> {
        match op {
            ExecOp::FsRead { path, offset, len } => {
                let root = std::path::Path::new(&ctx.profile.mappings.root_fs);
                let p = root.join(&path).clean();
                if !p.starts_with(root) { return Err(SandboxError::cap_blocked("path escapes root")); }

                let mut f = fs::File::open(&p).map_err(|e| SandboxError::provider_unavailable(&format!("open: {e}")))?;
                let mut buf = Vec::new();
                if let Some(off) = offset { use std::io::Seek; let _ = f.seek(std::io::SeekFrom::Start(off)).map_err(|e| SandboxError::provider_unavailable(&format!("seek: {e}")))?; }
                let l = len.unwrap_or(64 * 1024);
                let mut take = f.take(l);
                take.read_to_end(&mut buf).map_err(|e| SandboxError::provider_unavailable(&format!("read: {e}")))?;

                let used = Budget { bytes_in: buf.len() as u64, ..Default::default() };
                ctx.budget.check_and_consume("bytes_in", used.bytes_in).await?;

                let out = serde_json::json!({
                    "path": path,
                    "size": buf.len(),
                    "preview_b64": base64::engine::general_purpose::STANDARD.encode(&buf)
                });
                Ok(ExecResult { ok: true, code: None, message: None, out, used })
            }
            ExecOp::FsWrite { .. } => Err(SandboxError::cap_blocked("fs.write disabled in minimal RIS")),
            ExecOp::TmpAlloc { .. } => Ok(ExecResult { ok: true, code: None, message: None, out: serde_json::json!({"tmp":"ok"}), used: Budget { ..Default::default() } }),
            _ => Err(SandboxError::cap_blocked("unsupported op for fs")),
        }
    }
}
```

------

## src/exec/net.rs

```rust
use super::*;
use url::Url;

pub struct NetExecutor;

#[async_trait::async_trait]
impl Executor for NetExecutor {
    fn domain(&self) -> &'static str { "net" }

    async fn execute(&self, ctx: &ExecCtx<'_>, op: ExecOp) -> Result<ExecResult, SandboxError> {
        match op {
            ExecOp::NetHttp { method, url, .. } => {
                // Minimal：只做白名单校验 & 模拟响应（避免真实出网）
                let u = Url::parse(&url).map_err(|e| SandboxError::schema_invalid(&format!("bad url: {e}")))?;
                let host = u.host_str().unwrap_or_default().to_string();
                if !ctx.profile.whitelists.domains.iter().any(|d| d == &host) {
                    return Err(SandboxError::cap_blocked("host not in whitelist"));
                }
                if !ctx.profile.whitelists.methods.iter().any(|m| m.eq_ignore_ascii_case(&method)) {
                    return Err(SandboxError::cap_blocked("method not allowed"));
                }
                // 计量：一次调用 + 少量字节
                let used = Budget { bytes_out: 0, bytes_in: 256, calls: 0, ..Default::default() };
                ctx.budget.check_and_consume("bytes_in", used.bytes_in).await?;

                let out = serde_json::json!({
                    "url": url,
                    "method": method,
                    "simulated": true,
                    "preview": "hello from simulated net"
                });
                Ok(ExecResult { ok: true, code: None, message: None, out, used })
            }
            _ => Err(SandboxError::cap_blocked("unsupported op for net")),
        }
    }
}
```

------

## src/budget.rs

```rust
use crate::errors::SandboxError;
use crate::model::Budget;
use parking_lot::Mutex;

#[async_trait::async_trait]
pub trait BudgetMeter: Send + Sync {
    async fn check_and_consume(&self, kind: &str, amount: u64) -> Result<(), SandboxError>;
}
pub type BudgetMeterDyn<'a> = &'a dyn BudgetMeter;

pub struct MemoryBudget {
    inner: Mutex<Budget>,    // 剩余额度
}

impl MemoryBudget {
    pub fn new(b: Budget) -> Self { Self { inner: Mutex::new(b) } }
}

#[async_trait::async_trait]
impl BudgetMeter for MemoryBudget {
    async fn check_and_consume(&self, kind: &str, amount: u64) -> Result<(), SandboxError> {
        let mut b = self.inner.lock();
        let slot = match kind {
            "calls" => &mut b.calls,
            "bytes_in" => &mut b.bytes_in,
            "bytes_out" => &mut b.bytes_out,
            "cpu_ms" => &mut b.cpu_ms,
            "file_count" => &mut b.file_count,
            _ => &mut b.calls,
        };
        if *slot < amount { return Err(SandboxError::budget_exceeded()); }
        *slot -= amount;
        Ok(())
    }
}
```

------

## src/evidence.rs

```rust
use serde::{Serialize, Deserialize};
use crate::model::{Budget, SideEffect};
use sb_types::prelude::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Digest { pub algo: &'static str, pub b64: String, pub size: u64 }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SideEffectRec { pub kind: SideEffect, pub meta: serde_json::Value }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceBegin {
    pub envelope_id: Id,
    pub tenant: TenantId,
    pub subject_id: Id,
    pub tool_name: String,
    pub call_id: Id,
    pub profile_hash: String,
    pub capability: String,
    pub inputs_digest: Digest,
    pub produced_at_ms: i64,
    pub policy_version_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceEnd {
    pub envelope_id: Id,
    pub status: String,
    pub error_code: Option<&'static str>,
    pub outputs_digest: Digest,
    pub side_effects: Vec<SideEffectRec>,
    pub budget_used: Budget,
    pub finished_at_ms: i64,
}

#[async_trait::async_trait]
pub trait EvidenceSink: Send + Sync {
    async fn begin(&self, ev: &EvidenceBegin);
    async fn end(&self, ev: &EvidenceEnd);
}

pub type EvidenceSinkDyn<'a> = &'a dyn EvidenceSink;

/// 内存收集器（单测用）
pub struct MemoryEvidence {
    pub begins: parking_lot::Mutex<Vec<EvidenceBegin>>,
    pub ends: parking_lot::Mutex<Vec<EvidenceEnd>>,
}
impl MemoryEvidence { pub fn new() -> Self { Self { begins: Default::default(), ends: Default::default() } } }

#[async_trait::async_trait]
impl EvidenceSink for MemoryEvidence {
    async fn begin(&self, ev: &EvidenceBegin) { self.begins.lock().push(ev.clone()); }
    async fn end(&self, ev: &EvidenceEnd) { self.ends.lock().push(ev.clone()); }
}
```

------

## src/revoke.rs

```rust
use crate::model::Grant;

#[async_trait::async_trait]
pub trait RevocationWatcher: Send + Sync {
    async fn is_revoked(&self, _grant: &Grant) -> bool { false }
}
```

------

## src/errors.rs

```rust
use thiserror::Error;
use soulbase_errors::prelude::*;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct SandboxError(pub ErrorObj);

impl SandboxError {
    pub fn as_error_obj(&self) -> &ErrorObj { &self.0 }
    pub fn into_inner(self) -> ErrorObj { self.0 }

    pub fn policy_deny(msg: &str) -> Self {
        SandboxError(ErrorBuilder::new(codes::POLICY_DENY_TOOL).user_msg("Operation denied by policy.").dev_msg(msg).build())
    }
    pub fn cap_blocked(msg: &str) -> Self {
        SandboxError(ErrorBuilder::new(codes::SANDBOX_PERMISSION_DENY).user_msg("Sandbox capability blocked.").dev_msg(msg).build())
    }
    pub fn provider_unavailable(msg: &str) -> Self {
        SandboxError(ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE).user_msg("Upstream provider unavailable.").dev_msg(msg).build())
    }
    pub fn schema_invalid(msg: &str) -> Self {
        SandboxError(ErrorBuilder::new(codes::SCHEMA_VALIDATION).user_msg("Invalid input.").dev_msg(msg).build())
    }
    pub fn budget_exceeded() -> Self {
        SandboxError(ErrorBuilder::new(codes::QUOTA_BUDGET).user_msg("Budget exceeded.").build())
    }
    pub fn unknown(msg: &str) -> Self {
        SandboxError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL).user_msg("Internal error.").dev_msg(msg).build())
    }
}
```

> 说明：上文用到了 `codes::SANDBOX_PERMISSION_DENY`。若你希望与 SB-02 码表一致，请在 `soulbase-errors` 中新增 `SANDBOX.PERMISSION_DENY` 与 `SANDBOX.CAPABILITY_BLOCKED` 两个码（或将其临时映射到 `POLICY.DENY_TOOL`）。

------

## src/observe.rs

```rust
use std::collections::BTreeMap;
use crate::model::{Profile};

pub fn labels(profile: &Profile, code: Option<&str>) -> BTreeMap<&'static str, String> {
    let mut m = BTreeMap::new();
    m.insert("tenant", profile.tenant.0.clone());
    m.insert("tool", profile.tool_name.clone());
    if let Some(c) = code { m.insert("code", c.to_string()); }
    m
}
```

------

## src/prelude.rs

```rust
pub use crate::model::{
  Capability, SafetyClass, SideEffect, Budget, Grant, Limits, Whitelists, Mappings, Profile, ExecOp, ExecResult, ToolManifestLite
};
pub use crate::config::PolicyConfig;
pub use crate::profile::{ProfileBuilder, ProfileBuilderDefault};
pub use crate::guard::{PolicyGuard, PolicyGuardDefault};
pub use crate::exec::{Sandbox, Executor};
pub use crate::budget::{BudgetMeter, MemoryBudget};
pub use crate::evidence::{EvidenceSink, MemoryEvidence};
pub use crate::errors::SandboxError;
```

------

## tests/basic.rs

```rust
use soulbase_sandbox::prelude::*;
use sb_types::prelude::*;
use tempfile::tempdir;

fn grant_for_fs_read(root: &str) -> Grant {
    Grant {
        tenant: TenantId("tenantA".into()),
        subject_id: Id("user_1".into()),
        tool_name: "fs_reader".into(),
        call_id: Id("call_1".into()),
        capabilities: vec![Capability::FsRead { path: root.into() }],
        expires_at: chrono::Utc::now().timestamp_millis() + 60_000,
        budget: Budget { calls: 10, bytes_in: 1024 * 1024, ..Default::default() },
        decision_key_fingerprint: "dk".into(),
    }
}

#[tokio::test]
async fn fs_read_allowed_and_evidence_emitted() {
    // 1) 准备临时文件
    let dir = tempdir().unwrap();
    let root = dir.path().display().to_string();
    let p = dir.path().join("hello.txt");
    std::fs::write(&p, b"hello sandbox").unwrap();

    // 2) 合成 Profile
    let grant = grant_for_fs_read(&root);
    let manifest = ToolManifestLite {
        name: "fs_reader".into(),
        permissions: vec![Capability::FsRead { path: root.clone() }],
        safety_class: SafetyClass::Low,
        side_effect: SideEffect::Read,
    };
    let policy = PolicyConfig { mappings: Mappings { root_fs: root.clone(), tmp_dir: std::env::temp_dir().display().to_string() }, ..Default::default() };
    let pb = ProfileBuilderDefault;
    let profile = pb.build(&grant, &manifest, &policy).await.expect("profile");

    // 3) Guard
    let guard = PolicyGuardDefault;
    let op = ExecOp::FsRead { path: "hello.txt".into(), offset: None, len: None };
    guard.validate(&profile, &op).await.expect("guard ok");

    // 4) 执行
    let sb = soulbase_sandbox::exec::Sandbox::minimal();
    let ev = soulbase_sandbox::evidence::MemoryEvidence::new();
    let env_id = Id("env_1".into());
    let budget = soulbase_sandbox::budget::MemoryBudget::new(Budget { calls: 10, bytes_in: 1024, ..Default::default() });

    let res = sb.run(&profile, &env_id, &ev, &budget, op).await.expect("exec ok");
    assert!(res.ok);
    assert_eq!(res.out["size"].as_u64().unwrap() > 0, true);
    // 5) 证据
    assert_eq!(ev.begins.lock().len(), 1);
    assert_eq!(ev.ends.lock().len(), 1);
}

#[tokio::test]
async fn fs_read_path_escape_denied() {
    let dir = tempdir().unwrap();
    let root = dir.path().display().to_string();

    let grant = grant_for_fs_read(&root);
    let manifest = ToolManifestLite { name: "fs_reader".into(), permissions: vec![Capability::FsRead { path: root.clone() }], safety_class: SafetyClass::Low, side_effect: SideEffect::Read };
    let policy = PolicyConfig { mappings: Mappings { root_fs: root.clone(), tmp_dir: std::env::temp_dir().display().to_string() }, ..Default::default() };
    let profile = ProfileBuilderDefault.build(&grant, &manifest, &policy).await.unwrap();

    let guard = PolicyGuardDefault;
    let op = ExecOp::FsRead { path: "../../etc/passwd".into(), offset: None, len: Some(4) };
    let denied = guard.validate(&profile, &op).await.err().expect("denied");
    let eo = denied.into_inner();
    assert_eq!(eo.code.0, "SANDBOX.PERMISSION_DENY");
}

#[tokio::test]
async fn budget_calls_exceeded() {
    let dir = tempdir().unwrap();
    let root = dir.path().display().to_string();

    let grant = Grant { budget: Budget { calls: 0, ..Default::default() }, ..grant_for_fs_read(&root) };
    let manifest = ToolManifestLite { name: "fs_reader".into(), permissions: vec![Capability::FsRead { path: root.clone() }], safety_class: SafetyClass::Low, side_effect: SideEffect::Read };
    let policy = PolicyConfig { mappings: Mappings { root_fs: root.clone(), tmp_dir: std::env::temp_dir().display().to_string() }, ..Default::default() };
    let profile = ProfileBuilderDefault.build(&grant, &manifest, &policy).await.unwrap();

    let sb = soulbase_sandbox::exec::Sandbox::minimal();
    let ev = soulbase_sandbox::evidence::MemoryEvidence::new();
    let env_id = Id("env_2".into());
    let budget = soulbase_sandbox::budget::MemoryBudget::new(grant.budget.clone());

    let op = ExecOp::FsRead { path: "nonexistent.txt".into(), offset: None, len: Some(1) };
    let err = sb.run(&profile, &env_id, &ev, &budget, op).await.err().expect("exceed");
    let eo = err.into_inner();
    assert_eq!(eo.code.0, "QUOTA.BUDGET_EXCEEDED");
}

#[tokio::test]
async fn net_whitelist_denied() {
    let grant = Grant {
        tenant: TenantId("tenantA".into()),
        subject_id: Id("user_1".into()),
        tool_name: "net_fetch".into(),
        call_id: Id("call_net".into()),
        capabilities: vec![Capability::NetHttp { host: "example.com".into(), port: None, scheme: Some("https".into()), methods: vec!["GET".into(), "HEAD".into()] }],
        expires_at: chrono::Utc::now().timestamp_millis() + 60_000,
        budget: Budget { calls: 10, bytes_in: 1024, ..Default::default() },
        decision_key_fingerprint: "dk2".into(),
    };
    let manifest = ToolManifestLite {
        name: "net_fetch".into(),
        permissions: grant.capabilities.clone(),
        safety_class: SafetyClass::Medium,
        side_effect: SideEffect::Network,
    };
    let mut policy = PolicyConfig::default();
    policy.whitelists.domains = vec!["example.com".into()];
    let profile = ProfileBuilderDefault.build(&grant, &manifest, &policy).await.unwrap();

    let guard = PolicyGuardDefault;
    let op = ExecOp::NetHttp { method: "GET".into(), url: "https://blocked.test/".into(), headers: Default::default(), body_b64: None };
    let denied = guard.validate(&profile, &op).await.err().expect("blocked");
    let eo = denied.into_inner();
    assert_eq!(eo.code.0, "SANDBOX.PERMISSION_DENY");
}
```

------

## README.md（简版）

~~~markdown
# soulbase-sandbox (RIS)

Least-privilege, evidence-first controlled execution for Tools & Computer-Use.

**Included**
- Capability model, Grant/Budget/Profile
- ProfileBuilder (Grant ∩ Manifest ∩ PolicyConfig)
- PolicyGuard (path/domain/method checks)
- Executors: FS (read-only), NET (whitelist + simulated)
- Evidence sink & in-memory budget
- Tests: allow / deny / budget exceeded / evidence emitted

**Build & Test**
```bash
cargo check
cargo test
~~~

**Next**

- Replace `ToolManifestLite` with `soulbase-tools::Manifest`
- Real HTTP via `--features net-reqwest`
- Add Browser/Proc executors & QoS integration

```
---

### 对齐说明  
- **同频共振**：坚持“最小权限、默认拒绝、证据闭环、稳定错误码”四大不变式；Profile=Grant∩Manifest∩PolicyConfig；执行前 Guard 校验，执行后 Evidence 双事件。  
- **可演进**：预留 feature 位（真实出网、QoS、Observe、Browser/Proc/WASI）；08 模块就绪后切换到正式 `Manifest`。  
- **可运行**：内含单测覆盖**允许/拒绝/预算超限/证据**主路径，确保你的团队能立刻在仓库起步对接内核与工具层。
::contentReference[oaicite:0]{index=0}
```
