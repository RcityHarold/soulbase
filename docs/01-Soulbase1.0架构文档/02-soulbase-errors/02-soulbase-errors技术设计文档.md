# 文档 SB-02-TD：`soulbase-errors` 技术设计（Technical Design）

> 对应功能规约：SB-02
>  目标：给出**crate 设计、核心类型、构造/映射/渲染/封装外部错误**等可落地接口规范。
>  语言：Rust（`serde` 为基础，`tonic`/`http` 相关映射走可选 feature）。

------

## 1. 设计目标与约束（摘录）

- **稳定可机读**：错误域、稳定错误码（Stable Code）与标签可被程序消费与聚合。
- **跨协议**：提供到 HTTP/gRPC 的**标准映射口径**（建议映射，可被上层策略“合法地”降级/升级）。
- **最小披露**：对外仅暴露 `code + message_user (+ correlation_id)`；诊断细节只进审计/日志。
- **零策略**：不做“如何重试/是否熔断”的决策，只给出 `retryable`/`severity` 等建议标签。
- **轻依赖**：核心仅依赖 `serde`，其他映射（HTTP/gRPC/reqwest/sqlx）走 feature。

------

## 2. Crate 结构与模块

```
soulbase-errors/
  src/
    lib.rs
    kind.rs          # ErrorKind 枚举
    code.rs          # ErrorCode 类型、码表与注册/校验（静态）
    model.rs         # ErrorObj 主体结构 + 视图（Public/Audit）
    retry.rs         # Retryable/BackoffHint
    severity.rs      # Severity
    mapping_http.rs  # HTTP 映射（feature = "http"])
    mapping_grpc.rs  # gRPC 映射（feature = "grpc"])
    render.rs        # to_public()/to_audit() 渲染
    wrap.rs          # 外部错误封装（按 feature 拓展）
    labels.rs        # 观测标签导出
    prelude.rs
```

**features（建议）**

- `http`：提供 `to_http_status()` 与便捷响应体构造（不绑定具体 web 框架）。
- `grpc`：提供 `to_grpc_status()`（`tonic::Status`）。
- `wrap-reqwest` / `wrap-sqlx` / `wrap-llm`：外部错误分类封装。

**基础依赖**

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
once_cell = "1"
thiserror = "1"
```

------

## 3. 核心类型与码表

### 3.1 ErrorKind（稳定分类）

```rust
// kind.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ErrorKind {
  Auth, Quota, Schema, PolicyDeny, Sandbox,
  Provider, Storage, Timeout, Conflict, NotFound, Precondition,
  Serialization, Network, RateLimit, QosBudgetExceeded,
  ToolError, LlmError, A2AError, Unknown,
}
```

### 3.2 ErrorCode（稳定错误码）

```rust
// code.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ErrorCode(pub &'static str); // e.g. "AUTH.UNAUTHENTICATED"

// 每个 code 的注册信息（建议映射与标签）
#[derive(Clone, Debug)]
pub struct CodeSpec {
  pub code: ErrorCode,
  pub kind: crate::kind::ErrorKind,
  pub http_status: u16,           // 建议 HTTP 状态
  pub grpc_status: Option<i32>,   // 建议 gRPC code（tonic::Code as i32）
  pub retryable: crate::retry::RetryClass,
  pub severity: crate::severity::Severity,
  pub default_user_msg: &'static str,
}

// 静态码表（示例，完整清单在 RIS 中实现）
pub mod codes {
  use super::*;
  pub const AUTH_UNAUTHENTICATED: ErrorCode = ErrorCode("AUTH.UNAUTHENTICATED");
  pub const AUTH_FORBIDDEN:      ErrorCode = ErrorCode("AUTH.FORBIDDEN");
  pub const SCHEMA_VALIDATION:   ErrorCode = ErrorCode("SCHEMA.VALIDATION_FAILED");
  pub const QUOTA_RATELIMIT:     ErrorCode = ErrorCode("QUOTA.RATE_LIMITED");
  pub const LLM_TIMEOUT:         ErrorCode = ErrorCode("LLM.TIMEOUT");
  pub const PROVIDER_UNAVAILABLE:ErrorCode = ErrorCode("PROVIDER.UNAVAILABLE");
  pub const POLICY_DENY_TOOL:    ErrorCode = ErrorCode("POLICY.DENY_TOOL");
  // ...
}
```

> 码表**集中注册**于 `registry()`（一次性构建、去重校验），CI 会对**缺失/重复/漂移**做契约检查。

### 3.3 Retry 与 Severity

```rust
// retry.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RetryClass { None, Transient, Permanent }

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BackoffHint {
  pub initial_ms: u64,   // 建议初始退避
  pub max_ms: u64,       // 上限
}

// severity.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Severity { Info, Warn, Error, Critical }
```

### 3.4 错误对象（主结构）与视图

```rust
// model.rs
use serde::{Serialize, Deserialize};
use serde_json::{Map, Value};
use crate::{kind::ErrorKind, code::ErrorCode, retry::RetryClass, severity::Severity};
use sb_types::trace::TraceContext;

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CauseEntry {
  pub code: String,                    // 上游错误标识（脱敏）
  pub summary: String,                 // 简要说明
  #[serde(skip_serializing_if = "Option::is_none")]
  pub meta: Option<Map<String, Value>>,
}
```

------

## 4. 构造器与注册（Builder & Registry）

### 4.1 码表注册（集中定义 + 去重校验）

```rust
// code.rs（节选）
use once_cell::sync::Lazy;
use std::collections::HashMap;

pub static REGISTRY: Lazy<HashMap<&'static str, CodeSpec>> = Lazy::new(|| {
  let mut m = HashMap::new();

  let add = |m: &mut HashMap<&'static str, CodeSpec>, s: CodeSpec| {
    if m.insert(s.code.0, s).is_some() {
      panic!("duplicate error code: {}", s.code.0);
    }
  };

  add(&mut m, CodeSpec {
    code: codes::AUTH_UNAUTHENTICATED,
    kind: crate::kind::ErrorKind::Auth,
    http_status: 401,
    grpc_status: Some(16), // UNAUTHENTICATED
    retryable: crate::retry::RetryClass::Permanent,
    severity: crate::severity::Severity::Warn,
    default_user_msg: "Please sign in.",
  });

  // ...更多 add(...)
  m
});
```

### 4.2 构造器（Builder）

```rust
// model.rs（节选）
pub struct ErrorBuilder {
  code: ErrorCode,
  base: &'static crate::code::CodeSpec,
  message_user: Option<String>,
  message_dev: Option<String>,
  meta: Map<String, Value>,
  cause_chain: Vec<CauseEntry>,
  correlation_id: Option<String>,
  trace: Option<TraceContext>,
}

impl ErrorBuilder {
  pub fn new(code: ErrorCode) -> Self {
    let base = crate::code::REGISTRY
      .get(code.0)
      .expect("unregistered ErrorCode");
    Self {
      code, base, message_user: None, message_dev: None,
      meta: Map::new(), cause_chain: vec![], correlation_id: None, trace: None
    }
  }
  pub fn user_msg(mut self, s: impl Into<String>) -> Self { self.message_user = Some(s.into()); self }
  pub fn dev_msg(mut self, s: impl Into<String>) -> Self { self.message_dev = Some(s.into()); self }
  pub fn meta_kv(mut self, k: impl Into<String>, v: serde_json::Value) -> Self {
    self.meta.insert(k.into(), v); self
  }
  pub fn cause(mut self, c: CauseEntry) -> Self { self.cause_chain.push(c); self }
  pub fn correlation(mut self, id: impl Into<String>) -> Self { self.correlation_id = Some(id.into()); self }
  pub fn trace(mut self, t: TraceContext) -> Self { self.trace = Some(t); self }

  pub fn build(self) -> ErrorObj {
    ErrorObj {
      code: self.code,
      kind: self.base.kind,
      message_user: self.message_user.unwrap_or_else(|| self.base.default_user_msg.to_string()),
      message_dev: self.message_dev,
      http_status: self.base.http_status,
      grpc_status: self.base.grpc_status,
      retryable: self.base.retryable,
      severity: self.base.severity,
      meta: self.meta,
      cause_chain: if self.cause_chain.is_empty() { None } else { Some(self.cause_chain) },
      correlation_id: self.correlation_id,
      trace: self.trace,
    }
  }
}
```

------

## 5. 协议映射（HTTP / gRPC）

> 规则：**码表先于协议**；映射为建议值，上层可“合法”调整但不可改变语义。

```rust
// mapping_http.rs（feature = "http"）
pub fn to_http_status(err: &crate::model::ErrorObj) -> http::StatusCode {
  http::StatusCode::from_u16(err.http_status).unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR)
}

// mapping_grpc.rs（feature = "grpc"）
pub fn to_grpc_status(err: &crate::model::ErrorObj) -> tonic::Status {
  use tonic::Code;
  let code = match err.grpc_status {
    Some(16) => Code::Unauthenticated,
    Some(7)  => Code::PermissionDenied,
    Some(8)  => Code::ResourceExhausted,
    Some(14) => Code::Unavailable,
    _ => Code::Unknown,
  };
  tonic::Status::new(code, err.message_user.clone())
}
```

> 可选提供**便捷响应构造**：`to_http_response_json(err)`（返回 `(StatusCode, serde_json::Value)`），由各 Web 框架自行包装。

------

## 6. 渲染视图（最小披露原则）

```rust
// render.rs
use serde::{Serialize, Deserialize};
use crate::model::ErrorObj;

#[derive(Serialize, Deserialize)]
pub struct PublicErrorView {
  pub code: &'static str,
  pub message: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub correlation_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct AuditErrorView<'a> {
  pub code: &'static str,
  pub kind: &'static str,
  pub http_status: u16,
  pub retryable: &'static str,
  pub severity: &'static str,
  pub message_dev: Option<&'a str>,
  pub meta: &'a serde_json::Map<String, serde_json::Value>,
  pub cause_chain: &'a Option<Vec<crate::model::CauseEntry>>,
}

impl ErrorObj {
  pub fn to_public(&self) -> PublicErrorView {
    PublicErrorView {
      code: self.code.0,
      message: self.message_user.clone(),
      correlation_id: self.correlation_id.clone(),
    }
  }
  pub fn to_audit(&self) -> AuditErrorView<'_> {
    AuditErrorView {
      code: self.code.0,
      kind: format!("{:?}", self.kind).as_str(), // 也可固定映射
      http_status: self.http_status,
      retryable: match self.retryable { crate::retry::RetryClass::None => "None", crate::retry::RetryClass::Transient => "Transient", crate::retry::RetryClass::Permanent => "Permanent" },
      severity: match self.severity { crate::severity::Severity::Info=>"Info", Severity::Warn=>"Warn", Severity::Error=>"Error", Severity::Critical=>"Critical" },
      message_dev: self.message_dev.as_deref(),
      meta: &self.meta,
      cause_chain: &self.cause_chain,
    }
  }
}
```

> **保证**：`to_public()` 不包含 `message_dev/meta/cause_chain` 等敏感内容。

------

## 7. 外部错误封装（Wrapping）

> 通过 feature 启用特定生态封装，把第三方错误**分类→稳定码**，同时保留**脱敏**的 `cause_chain`。

```rust
// wrap.rs（示意）
#[cfg(feature = "wrap-reqwest")]
impl From<reqwest::Error> for crate::model::ErrorObj {
  fn from(e: reqwest::Error) -> Self {
    let code = if e.is_timeout() {
      crate::code::codes::PROVIDER_UNAVAILABLE
    } else {
      crate::code::codes::PROVIDER_UNAVAILABLE
    };
    crate::model::ErrorBuilder::new(code)
      .user_msg("Upstream provider is unavailable. Please retry later.")
      .dev_msg(format!("reqwest: {}", e))
      .meta_kv("provider", serde_json::json!("http"))
      .build()
  }
}
```

> 其他：`wrap-sqlx`（区分 `RowNotFound`→`NotFound`、`Database`→`Storage`）、`wrap-llm`（`context_length`→`LLM.CONTEXT_OVERFLOW`、`safety_block`→`LLM.SAFETY_BLOCK`）等。

------

## 8. 观测标签导出（Metrics Labels）

```rust
// labels.rs
use std::collections::BTreeMap;
use crate::model::ErrorObj;

pub fn labels(err: &ErrorObj) -> BTreeMap<&'static str, String> {
  let mut m = BTreeMap::new();
  m.insert("code", err.code.0.to_string());
  m.insert("kind", format!("{:?}", err.kind));
  m.insert("retryable", match err.retryable { crate::retry::RetryClass::None=>"none", crate::retry::RetryClass::Transient=>"transient", crate::retry::RetryClass::Permanent=>"permanent" }.to_string());
  m.insert("severity", match err.severity { crate::severity::Severity::Info=>"info", Severity::Warn=>"warn", Severity::Error=>"error", Severity::Critical=>"critical" }.to_string());
  if let Some(p) = err.meta.get("provider") { m.insert("provider", p.to_string()); }
  if let Some(t) = err.meta.get("tool") { m.insert("tool", t.to_string()); }
  if let Some(tenant) = err.meta.get("tenant") { m.insert("tenant", tenant.to_string()); }
  m
}
```

------

## 9. 测试与验收（要点）

- **码表完整性**：`REGISTRY` 去重；为每个 code 校验 `http/grpc/retryable/severity/default_msg` 均已定义。
- **最小披露**：`to_public()` 不含 dev/meta/cause；`to_audit()` 包含完整上下文。
- **映射一致性**：HTTP/gRPC 映射与功能规约的建议表一致。
- **包装一致性**：外部错误（reqwest/sqlx/llm）能被**确定性**分类到指定 `code`。
- **兼容测试**（在 `soulbase-contract-testkit`）：对旧版本 code 的公共视图序列化保持兼容。

------

## 10. 版本与迁移

- **新增 code**：必须在 `REGISTRY` 注册 + 文档化 + 契约测试覆盖。
- **修改语义**：需要 ADR + **MAJOR** 升级；旧 code **不得复用**为新语义。
- **废弃流程**：标注 deprecated，并在两个 MINOR 周期后移除；期间需要映射兼容层。

------

## 11. 开放事项

- `Severity` 与 HealthGuardian 的联动阈值（是否在 TD 中给出建议表）。
- i18n：`message_user` 的本地化字典加载口径（建议在上层实现，通过 `meta.lang` 或请求上下文注入）。
- A2A 错误的证据最小字段集合（签名算法、对账指纹），与 `soulbase-a2a` 协同细化。

------

**备注**：以上为**技术设计**，尚未落地为完整代码骨架（RIS）。若你确认无误，我将保持“三件套”节奏，**下一步输出 SB-02-RIS**（最小可运行骨架），并随后进入你指定的下一个模块。
