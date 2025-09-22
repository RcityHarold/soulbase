下面是 **SB-02-RIS：`soulbase-errors` 最小可运行骨架**。放到 `soul-base/crates/soulbase-errors/` 下即可 `cargo check && cargo test`。如与 `sb-types` 同一 workspace，请按最后的 workspace 示例添加 members。

------

## 目录

```
soul-base/
└─ crates/
   └─ soulbase-errors/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ kind.rs
      │  ├─ retry.rs
      │  ├─ severity.rs
      │  ├─ code.rs
      │  ├─ model.rs
      │  ├─ render.rs
      │  ├─ mapping_http.rs
      │  ├─ mapping_grpc.rs
      │  ├─ wrap.rs
      │  ├─ labels.rs
      │  └─ prelude.rs
      └─ tests/
         └─ basic.rs
```

------

## `Cargo.toml`

```toml
[package]
name = "soulbase-errors"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "Unified error domain & stable error codes for the Soul platform."
repository = "https://example.com/soul-base"

[features]
http = ["dep:http"]
grpc = ["dep:tonic"]
wrap-reqwest = ["dep:reqwest"]
wrap-sqlx = ["dep:sqlx"]
wrap-llm = []  # 占位，按需扩展

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
once_cell = "1"

# 映射可选依赖
http = { version = "1", optional = true }
tonic = { version = "0.12", optional = true }

# 仅在 wrap-* feature 时编译
reqwest = { version = "0.12", optional = true, default-features = false, features = ["rustls-tls"] }
sqlx = { version = "0.7", optional = true, default-features = false, features = ["runtime-tokio", "macros"] }

# 需要 sb-types（TraceContext 等）
sb-types = { path = "../sb-types", version = "1.0.0" }

[dev-dependencies]
serde_json = "1"
```

> 如路径不同，请调整 `sb-types` 的 `path` 或改用 `workspace = true`。

------

## `src/lib.rs`

```rust
pub mod kind;
pub mod retry;
pub mod severity;
pub mod code;
pub mod model;
pub mod render;
#[cfg(feature = "http")] pub mod mapping_http;
#[cfg(feature = "grpc")] pub mod mapping_grpc;
pub mod wrap;
pub mod labels;
pub mod prelude;
```

------

## `src/kind.rs`

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ErrorKind {
    Auth, Quota, Schema, PolicyDeny, Sandbox,
    Provider, Storage, Timeout, Conflict, NotFound, Precondition,
    Serialization, Network, RateLimit, QosBudgetExceeded,
    ToolError, LlmError, A2AError, Unknown,
}

impl ErrorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Auth => "Auth",
            ErrorKind::Quota => "Quota",
            ErrorKind::Schema => "Schema",
            ErrorKind::PolicyDeny => "PolicyDeny",
            ErrorKind::Sandbox => "Sandbox",
            ErrorKind::Provider => "Provider",
            ErrorKind::Storage => "Storage",
            ErrorKind::Timeout => "Timeout",
            ErrorKind::Conflict => "Conflict",
            ErrorKind::NotFound => "NotFound",
            ErrorKind::Precondition => "Precondition",
            ErrorKind::Serialization => "Serialization",
            ErrorKind::Network => "Network",
            ErrorKind::RateLimit => "RateLimit",
            ErrorKind::QosBudgetExceeded => "QosBudgetExceeded",
            ErrorKind::ToolError => "ToolError",
            ErrorKind::LlmError => "LlmError",
            ErrorKind::A2AError => "A2AError",
            ErrorKind::Unknown => "Unknown",
        }
    }
}
```

------

## `src/retry.rs`

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RetryClass { None, Transient, Permanent }

impl RetryClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            RetryClass::None => "none",
            RetryClass::Transient => "transient",
            RetryClass::Permanent => "permanent",
        }
    }
}
```

------

## `src/severity.rs`

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Severity { Info, Warn, Error, Critical }

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Error => "error",
            Severity::Critical => "critical",
        }
    }
}
```

------

## `src/code.rs`

```rust
use crate::{kind::ErrorKind, retry::RetryClass, severity::Severity};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use serde::{Serialize, Serializer, Deserialize, Deserializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ErrorCode(pub &'static str);

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.serialize_str(self.0) }
}
impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(ErrorCode(Box::leak(s.into_boxed_str())))
    }
}

#[derive(Clone, Debug)]
pub struct CodeSpec {
    pub code: ErrorCode,
    pub kind: ErrorKind,
    pub http_status: u16,
    pub grpc_status: Option<i32>,
    pub retryable: RetryClass,
    pub severity: Severity,
    pub default_user_msg: &'static str,
}

pub mod codes {
    use super::ErrorCode;
    pub const AUTH_UNAUTHENTICATED: ErrorCode = ErrorCode("AUTH.UNAUTHENTICATED");
    pub const AUTH_FORBIDDEN:      ErrorCode = ErrorCode("AUTH.FORBIDDEN");
    pub const SCHEMA_VALIDATION:   ErrorCode = ErrorCode("SCHEMA.VALIDATION_FAILED");
    pub const QUOTA_RATELIMIT:     ErrorCode = ErrorCode("QUOTA.RATE_LIMITED");
    pub const QUOTA_BUDGET:        ErrorCode = ErrorCode("QUOTA.BUDGET_EXCEEDED");
    pub const POLICY_DENY_TOOL:    ErrorCode = ErrorCode("POLICY.DENY_TOOL");
    pub const LLM_TIMEOUT:         ErrorCode = ErrorCode("LLM.TIMEOUT");
    pub const LLM_CONTEXT_OVERFLOW:ErrorCode = ErrorCode("LLM.CONTEXT_OVERFLOW");
    pub const PROVIDER_UNAVAILABLE:ErrorCode = ErrorCode("PROVIDER.UNAVAILABLE");
    pub const STORAGE_NOT_FOUND:   ErrorCode = ErrorCode("STORAGE.NOT_FOUND");
    pub const UNKNOWN_INTERNAL:    ErrorCode = ErrorCode("UNKNOWN.INTERNAL");
}

pub static REGISTRY: Lazy<HashMap<&'static str, CodeSpec>> = Lazy::new(|| {
    use codes::*;
    use tonic::Code;

    let mut m = HashMap::new();
    let mut add = |s: CodeSpec| {
        if m.insert(s.code.0, s).is_some() { panic!("duplicate error code: {}", s.code.0); }
    };

    add(CodeSpec { code: AUTH_UNAUTHENTICATED, kind: ErrorKind::Auth,
        http_status: 401, grpc_status: Some(Code::Unauthenticated as i32),
        retryable: RetryClass::Permanent, severity: Severity::Warn,
        default_user_msg: "Please sign in." });

    add(CodeSpec { code: AUTH_FORBIDDEN, kind: ErrorKind::Auth,
        http_status: 403, grpc_status: Some(Code::PermissionDenied as i32),
        retryable: RetryClass::Permanent, severity: Severity::Warn,
        default_user_msg: "You don't have permission to perform this action." });

    add(CodeSpec { code: SCHEMA_VALIDATION, kind: ErrorKind::Schema,
        http_status: 422, grpc_status: Some(Code::InvalidArgument as i32),
        retryable: RetryClass::Permanent, severity: Severity::Warn,
        default_user_msg: "Your request is invalid. Please check inputs." });

    add(CodeSpec { code: QUOTA_RATELIMIT, kind: ErrorKind::RateLimit,
        http_status: 429, grpc_status: Some(Code::ResourceExhausted as i32),
        retryable: RetryClass::Transient, severity: Severity::Warn,
        default_user_msg: "Too many requests. Please retry later." });

    add(CodeSpec { code: QUOTA_BUDGET, kind: ErrorKind::QosBudgetExceeded,
        http_status: 429, grpc_status: Some(Code::ResourceExhausted as i32),
        retryable: RetryClass::Permanent, severity: Severity::Warn,
        default_user_msg: "Budget exceeded." });

    add(CodeSpec { code: POLICY_DENY_TOOL, kind: ErrorKind::PolicyDeny,
        http_status: 403, grpc_status: Some(Code::PermissionDenied as i32),
        retryable: RetryClass::Permanent, severity: Severity::Warn,
        default_user_msg: "Tool usage is not allowed by policy." });

    add(CodeSpec { code: LLM_TIMEOUT, kind: ErrorKind::LlmError,
        http_status: 503, grpc_status: Some(Code::Unavailable as i32),
        retryable: RetryClass::Transient, severity: Severity::Error,
        default_user_msg: "The model did not respond in time. Please try again." });

    add(CodeSpec { code: LLM_CONTEXT_OVERFLOW, kind: ErrorKind::LlmError,
        http_status: 400, grpc_status: Some(Code::OutOfRange as i32),
        retryable: RetryClass::Permanent, severity: Severity::Warn,
        default_user_msg: "Input is too long for the model." });

    add(CodeSpec { code: PROVIDER_UNAVAILABLE, kind: ErrorKind::Provider,
        http_status: 503, grpc_status: Some(Code::Unavailable as i32),
        retryable: RetryClass::Transient, severity: Severity::Error,
        default_user_msg: "Upstream provider is unavailable. Please retry later." });

    add(CodeSpec { code: STORAGE_NOT_FOUND, kind: ErrorKind::NotFound,
        http_status: 404, grpc_status: Some(Code::NotFound as i32),
        retryable: RetryClass::Permanent, severity: Severity::Info,
        default_user_msg: "Resource not found." });

    add(CodeSpec { code: UNKNOWN_INTERNAL, kind: ErrorKind::Unknown,
        http_status: 500, grpc_status: Some(Code::Unknown as i32),
        retryable: RetryClass::Transient, severity: Severity::Critical,
        default_user_msg: "Internal error. Please retry later." });

    m
});

pub fn spec_of(code: ErrorCode) -> &'static CodeSpec {
    REGISTRY.get(code.0).expect("unregistered ErrorCode")
}
```

------

## `src/model.rs`

```rust
use serde::{Serialize, Deserialize};
use serde_json::{Map, Value};
use crate::{code::{ErrorCode, spec_of}, kind::ErrorKind, retry::RetryClass, severity::Severity};
use sb_types::trace::TraceContext;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CauseEntry {
    pub code: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Map<String, Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorObj {
    pub code: ErrorCode,
    pub kind: ErrorKind,
    pub message_user: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_dev: Option<String>,
    pub http_status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grpc_status: Option<i32>,
    pub retryable: RetryClass,
    pub severity: Severity,
    #[serde(default)]
    pub meta: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause_chain: Option<Vec<CauseEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<TraceContext>,
}

pub struct ErrorBuilder {
    code: ErrorCode,
    message_user: Option<String>,
    message_dev: Option<String>,
    meta: Map<String, Value>,
    cause_chain: Vec<CauseEntry>,
    correlation_id: Option<String>,
    trace: Option<TraceContext>,
}

impl ErrorBuilder {
    pub fn new(code: ErrorCode) -> Self {
        Self {
            code, message_user: None, message_dev: None,
            meta: Map::new(), cause_chain: vec![],
            correlation_id: None, trace: None
        }
    }
    pub fn user_msg(mut self, s: impl Into<String>) -> Self { self.message_user = Some(s.into()); self }
    pub fn dev_msg(mut self, s: impl Into<String>) -> Self { self.message_dev = Some(s.into()); self }
    pub fn meta_kv(mut self, k: impl Into<String>, v: Value) -> Self { self.meta.insert(k.into(), v); self }
    pub fn cause(mut self, c: CauseEntry) -> Self { self.cause_chain.push(c); self }
    pub fn correlation(mut self, id: impl Into<String>) -> Self { self.correlation_id = Some(id.into()); self }
    pub fn trace(mut self, t: TraceContext) -> Self { self.trace = Some(t); self }

    pub fn build(self) -> ErrorObj {
        let spec = spec_of(self.code);
        ErrorObj {
            code: self.code,
            kind: spec.kind,
            message_user: self.message_user.unwrap_or_else(|| spec.default_user_msg.to_string()),
            message_dev: self.message_dev,
            http_status: spec.http_status,
            grpc_status: spec.grpc_status,
            retryable: spec.retryable,
            severity: spec.severity,
            meta: self.meta,
            cause_chain: if self.cause_chain.is_empty(){None}else{Some(self.cause_chain)},
            correlation_id: self.correlation_id,
            trace: self.trace,
        }
    }
}
```

------

## `src/render.rs`

```rust
use serde::{Serialize, Deserialize};
use crate::{model::ErrorObj, kind::ErrorKind, retry::RetryClass, severity::Severity};

#[derive(Serialize, Deserialize, Debug)]
pub struct PublicErrorView {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AuditErrorView {
    pub code: &'static str,
    pub kind: &'static str,
    pub http_status: u16,
    pub retryable: &'static str,
    pub severity: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_dev: Option<String>,
    pub meta: serde_json::Map<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause_chain: Option<Vec<crate::model::CauseEntry>>,
}

impl ErrorObj {
    pub fn to_public(&self) -> PublicErrorView {
        PublicErrorView {
            code: self.code.0,
            message: self.message_user.clone(),
            correlation_id: self.correlation_id.clone(),
        }
    }
    pub fn to_audit(&self) -> AuditErrorView {
        AuditErrorView {
            code: self.code.0,
            kind: ErrorKind::as_str(self.kind),
            http_status: self.http_status,
            retryable: RetryClass::as_str(self.retryable),
            severity: Severity::as_str(self.severity),
            message_dev: self.message_dev.clone(),
            meta: self.meta.clone(),
            cause_chain: self.cause_chain.clone(),
        }
    }
}
```

------

## `src/mapping_http.rs`（feature = `http`）

```rust
pub fn to_http_status(err: &crate::model::ErrorObj) -> http::StatusCode {
    http::StatusCode::from_u16(err.http_status).unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR)
}
```

------

## `src/mapping_grpc.rs`（feature = `grpc`）

```rust
pub fn to_grpc_status(err: &crate::model::ErrorObj) -> tonic::Status {
    use tonic::Code;
    let code = match err.grpc_status {
        Some(v) => Code::from_i32(v),
        None => Code::Unknown,
    };
    tonic::Status::new(code, err.message_user.clone())
}
```

------

## `src/wrap.rs`

```rust
use crate::{model::{ErrorObj, ErrorBuilder}, code::codes, code::ErrorCode};

#[cfg(feature = "wrap-reqwest")]
impl From<reqwest::Error> for ErrorObj {
    fn from(e: reqwest::Error) -> Self {
        let code: ErrorCode = if e.is_timeout() { codes::LLM_TIMEOUT } else { codes::PROVIDER_UNAVAILABLE };
        ErrorBuilder::new(code)
            .user_msg("Upstream provider is unavailable. Please retry later.")
            .dev_msg(format!("reqwest: {e}"))
            .meta_kv("provider", serde_json::json!("http"))
            .build()
    }
}

#[cfg(feature = "wrap-sqlx")]
impl From<sqlx::Error> for ErrorObj {
    fn from(e: sqlx::Error) -> Self {
        use sqlx::Error::*;
        let (code, user) = match e {
            RowNotFound => (codes::STORAGE_NOT_FOUND, "Resource not found."),
            _ => (codes::PROVIDER_UNAVAILABLE, "Database is unavailable. Please retry later."),
        };
        ErrorBuilder::new(code)
            .user_msg(user)
            .dev_msg(format!("sqlx: {e}"))
            .meta_kv("provider", serde_json::json!("db"))
            .build()
    }
}
```

------

## `src/labels.rs`

```rust
use std::collections::BTreeMap;
use crate::{model::ErrorObj, retry::RetryClass, severity::Severity};

pub fn labels(err: &ErrorObj) -> BTreeMap<&'static str, String> {
    let mut m = BTreeMap::new();
    m.insert("code", err.code.0.to_string());
    m.insert("kind", err.kind.as_str().to_string());
    m.insert("retryable", err.retryable.as_str().to_string());
    m.insert("severity", err.severity.as_str().to_string());
    if let Some(v) = err.meta.get("provider") { m.insert("provider", v.to_string()); }
    if let Some(v) = err.meta.get("tool") { m.insert("tool", v.to_string()); }
    if let Some(v) = err.meta.get("tenant") { m.insert("tenant", v.to_string()); }
    m
}
```

------

## `src/prelude.rs`

```rust
pub use crate::{
    kind::ErrorKind,
    retry::RetryClass,
    severity::Severity,
    code::{ErrorCode, CodeSpec, REGISTRY, spec_of, codes},
    model::{ErrorObj, ErrorBuilder, CauseEntry},
    render::{PublicErrorView, AuditErrorView},
    labels::labels,
};
```

------

## `tests/basic.rs`

```rust
use soulbase_errors::prelude::*;
use serde_json::json;

#[test]
fn build_and_render_public() {
    let err = ErrorBuilder::new(codes::AUTH_UNAUTHENTICATED)
        .user_msg("Please sign in.")
        .dev_msg("missing bearer token")
        .meta_kv("tenant", json!("tenantA"))
        .correlation("req-123")
        .build();

    let pv = err.to_public();
    assert_eq!(pv.code, "AUTH.UNAUTHENTICATED");
    assert_eq!(pv.message, "Please sign in.");
    assert_eq!(pv.correlation_id.as_deref(), Some("req-123"));

    let lbl = labels(&err);
    assert_eq!(lbl.get("code").unwrap(), "AUTH.UNAUTHENTICATED");
}

#[cfg(feature = "http")]
#[test]
fn http_status_mapping() {
    let err = ErrorBuilder::new(codes::QUOTA_RATELIMIT).build();
    let s = soulbase_errors::mapping_http::to_http_status(&err);
    assert_eq!(s.as_u16(), 429);
}
```

------

## `README.md`（简版）

~~~markdown
# soulbase-errors

Unified error domain & stable error codes.

## Build & Test
```bash
cargo check
cargo test
~~~

## Optional features

- `http`: map to HTTP status codes
- `grpc`: map to gRPC status
- `wrap-reqwest` / `wrap-sqlx`: wrap external errors

```
---

## （可选）workspace 根 `Cargo.toml` 增补

```toml
[workspace]
members = ["crates/sb-types", "crates/soulbase-errors"]
resolver = "2"
```

> 至此，`soulbase-errors` 的三件套已完成第 3 部分骨架。下一模块你想继续哪个（`soulbase-config` / `soulbase-auth` / `soulbase-interceptors`）？
