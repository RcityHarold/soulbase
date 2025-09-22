# 文档 SB-01-TD：`sb-types` 技术设计（Technical Design）

> 版本：v1.0（对应功能规约 SB-01）
>  目标：给出 **crate 结构、核心类型/Traits、校验与 Schema 生成、版本治理、测试与验收** 的可落地实现方案。
>  语言/生态：Rust（`serde`/`schemars`）。
>  **注意**：本 crate 仅承载**数据契约**与**结构性校验**；鉴权、错误域、策略/状态机均不在本 crate 实现。

------

## 1. 设计目标与约束（Goals & Constraints）

- **单一职责**：跨仓统一的“数据契约底座”。
- **零策略**：不包含授权、路由、存储实现；仅定义**类型与不变式**校验。
- **跨语言友好**：内置 JSON Schema 生成；为 TS/Go/Java 绑定留扩展点。
- **不可变/可回放**：Envelope 追加式表达；显式因果/关联链路。
- **可版本化**：SemVer 驱动的 Schema 进化；契约测试覆盖兼容矩阵。
- **轻依赖**：仅 `serde`、`serde_json`、`schemars`（可选 feature）。

------

## 2. Crate 结构与模块划分

```
sb-types/
├─ Cargo.toml
└─ src/
   ├─ lib.rs
   ├─ id.rs              # Id, CausationId, CorrelationId
   ├─ time.rs            # Timestamp
   ├─ tenant.rs          # TenantId
   ├─ subject.rs         # Subject, SubjectKind, Claims
   ├─ scope.rs           # Scope, Consent
   ├─ envelope.rs        # Envelope<T> + 不变式校验
   ├─ trace.rs           # TraceContext 占位（TraceId/SpanId）
   ├─ traits.rs          # Partitioned, Versioned, Auditable, Causal
   ├─ validate.rs        # 结构性校验器（本地错误类型）
   └─ prelude.rs         # 常用导出
```

**Feature Flags**

- `schema`：启用 `schemars` 导出 JSON Schema。
- `serde_borrow`：允许 `Cow<'a, str>` 等零拷贝路径（后续可选）。
- `uuid`：可选将 `Id` 用 `uuid::Uuid` 适配（默认 `String` 包装）。

**外部依赖（最小集）**

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
schemars = { version = "0.8", optional = true, features = ["serde_json"] }
thiserror = "1"                # 仅用于本地校验错误
```

> 与 `soulbase-errors` 的集成：在后续版本中由 `validate` 的本地错误并入统一错误域（避免环依赖，当前先用本地错误）。

------

## 3. 核心数据结构（Rust）

### 3.1 标识与时间

```rust
// id.rs
use serde::{Deserialize, Serialize};
#[cfg(feature = "schema")] use schemars::JsonSchema;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Id(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CausationId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CorrelationId(pub String);

// time.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Timestamp(pub i64); // ms since epoch, UTC
```

### 3.2 租户与主体

```rust
// tenant.rs
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TenantId(pub String);

// subject.rs
use crate::{Id, TenantId};
use serde::{Deserialize, Serialize};
#[cfg(feature = "schema")] use schemars::JsonSchema;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum SubjectKind { User, Service, Agent }

/// 附带最小必要声明，避免敏感信息泛滥
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Subject {
    pub kind: SubjectKind,
    pub subject_id: Id,
    pub tenant: TenantId,
    /// 附加声明（如 roles, permissions 摘要、client_id 等）
    #[serde(default)]
    pub claims: serde_json::Map<String, serde_json::Value>,
}
```

### 3.3 权限范围与同意

```rust
// scope.rs
use serde::{Deserialize, Serialize};
#[cfg(feature = "schema")] use schemars::JsonSchema;
use crate::Timestamp;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Scope {
    pub resource: String,       // e.g. "tool:browser" / "storage:kv"
    pub action: String,         // "invoke" | "read" | "write" | "list" | ...
    #[serde(default)]
    pub attrs: serde_json::Map<String, serde_json::Value>, // 可选细粒度属性
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Consent {
    #[serde(default)]
    pub scopes: Vec<Scope>,
    pub expires_at: Option<Timestamp>,
    #[serde(default)]
    pub purpose: Option<String>,       // 可选：同意目的说明
}
```

### 3.4 Trace 上下文（占位）

```rust
// trace.rs
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TraceContext {
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    #[serde(default)]
    pub baggage: serde_json::Map<String, serde_json::Value>,
}
```

### 3.5 Envelope（统一传输壳）

```rust
// envelope.rs
use crate::*;
use serde::{Deserialize, Serialize};
#[cfg(feature = "schema")] use schemars::JsonSchema;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Envelope<T> {
    pub envelope_id: Id,
    pub produced_at: Timestamp,
    pub partition_key: String,

    #[serde(default)]
    pub causation_id: Option<CausationId>,
    #[serde(default)]
    pub correlation_id: Option<CorrelationId>,

    pub actor: Subject,
    #[serde(default)]
    pub consent: Option<Consent>,

    /// 负载 Schema 的 SemVer 版本（"1.2.0"）
    pub schema_ver: String,
    /// 可选追踪上下文（日志/追踪体系读取）
    #[serde(default)]
    pub trace: Option<TraceContext>,

    pub payload: T,
}
```

------

## 4. 契约 Traits 与不变式校验

```rust
// traits.rs
pub trait Versioned {
    fn schema_version(&self) -> &str;          // "MAJOR.MINOR.PATCH"
}

pub trait Partitioned {
    fn partition_key(&self) -> &str;           // e.g. "tenant:conv_123"
}

pub trait Auditable {
    fn actor(&self) -> &crate::subject::Subject;
    fn produced_at(&self) -> crate::Timestamp;
}

pub trait Causal {
    fn causation_id(&self) -> Option<&crate::id::CausationId>;
    fn correlation_id(&self) -> Option<&crate::id::CorrelationId>;
}

// 对 Envelope 的默认实现
impl<T> Versioned for Envelope<T> {
    #[inline] fn schema_version(&self) -> &str { &self.schema_ver }
}
impl<T> Partitioned for Envelope<T> {
    #[inline] fn partition_key(&self) -> &str { &self.partition_key }
}
impl<T> Auditable for Envelope<T> {
    #[inline] fn actor(&self) -> &Subject { &self.actor }
    #[inline] fn produced_at(&self) -> Timestamp { self.produced_at }
}
impl<T> Causal for Envelope<T> {
    #[inline] fn causation_id(&self) -> Option<&CausationId> { self.causation_id.as_ref() }
    #[inline] fn correlation_id(&self) -> Option<&CorrelationId> { self.correlation_id.as_ref() }
}
```

### 4.1 结构性校验器（仅本地错误）

```rust
// validate.rs
use crate::*;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValidateError {
    #[error("empty_field:{0}")]
    EmptyField(&'static str),
    #[error("invalid_semver:{0}")]
    InvalidSemVer(String),
    #[error("tenant_mismatch")]
    TenantMismatch,
}

pub trait Validate {
    fn validate(&self) -> Result<(), ValidateError>;
}

impl Validate for Subject {
    fn validate(&self) -> Result<(), ValidateError> {
        if self.subject_id.0.is_empty() { return Err(ValidateError::EmptyField("subject_id")); }
        if self.tenant.0.is_empty() { return Err(ValidateError::EmptyField("tenant")); }
        Ok(())
    }
}

impl<T> Validate for Envelope<T> {
    fn validate(&self) -> Result<(), ValidateError> {
        if self.envelope_id.0.is_empty() { return Err(ValidateError::EmptyField("envelope_id")); }
        if self.partition_key.is_empty() { return Err(ValidateError::EmptyField("partition_key")); }
        if semver::Version::parse(&self.schema_ver).is_err() {
            return Err(ValidateError::InvalidSemVer(self.schema_ver.clone()));
        }
        self.actor.validate()?;
        // 这里不强制 trace/consent 存在
        Ok(())
    }
}
```

> 说明：`semver` 为轻依赖，可加入 Cargo（或在上层 crate 校验）。
>  将来当 `soulbase-errors` 就绪时，`ValidateError` 会迁移/映射到统一错误域。

------

## 5. Schema 生成与跨语言绑定

- 开启 `schema` feature 时，为所有导出类型自动派生 `schemars::JsonSchema`。
- 在 `lib.rs` 导出一个**集中生成器**，用于 CI 产出 JSON Schema 工件：

```rust
// lib.rs（节选）
#[cfg(feature = "schema")]
pub mod schema_gen {
    use super::*;
    use schemars::{schema_for, gen::SchemaSettings};

    pub fn schema_for_envelope<T>() -> schemars::schema::RootSchema
    where T: schemars::JsonSchema {
        schema_for!(Envelope<T>)
    }
}
```

- **工件产出规范**：
  - 文件组织：`schema/<crate-version>/Envelope.json`, `schema/<crate-version>/Subject.json`, …
  - 发布流程：CI 在 tag 时产出并上传到制品库（供 TS/Go/Java 代码生成器消费）。

------

## 6. 与 Soul-Auth / Soul-Hub 的映射口径（类型层）

> **仅定义类型，不在此 crate 写解析逻辑**。映射函数由 `soulbase-auth`/`soulbase-interceptors` 实现。

- **Token → Subject**：
  - `sub`→`Subject.subject_id`、`tenant`→`TenantId`、`roles/permissions`→存入 `claims`；
  - 可选 `consents`（数组）→由上层转为 `Consent` 注入 `Envelope`。
- **Headers → Envelope meta**：
  - `X-Request-Id`→`envelope_id`（或派生新 Id 并记录到 claims）；
  - `X-Trace-Id/X-Span-Id`→`TraceContext`；
  - `X-Soul-Tenant`→`TenantId`；
  - `X-Consent-Token`→上层解析后填入 `consent`。

------

## 7. 存储模型与索引（建议口径）

尽管本 crate **不**实现存储，但为下游统一口径提供建议字段组合：

- **分区键（PartitionKey）**：`"{tenant}:{conversation_id}"`、`"{tenant}:{session_id}"` 或 `"{tenant}:{aggregate_id}"`。
- **索引建议**：
  - PK(`partition_key`), SK(`produced_at` DESC) —— 回放/时序；
  - 二级索引：`actor.subject_id`、`correlation_id`。
- **归档策略字段**：可在 Envelope 扩展域中附 `ttl` 或 `retention_class`（建议由上层 QoS 模块维护）。

------

## 8. 并发与性能（容量估算）

- Envelope 典型大小：**0.5–2 KB**（不含大型 `payload`）。
- 序列化：`serde_json` p50 < 100µs（中等 payload）；
- 建议为高频路径引入 `serde_borrow` feature，在必要处使用 `Cow<'a, str>` 降拷贝。
- 生成 Id/时间戳：不在本 crate 内实现，交由上层统一生成，避免系统时钟/随机源耦合。

------

## 9. 可观测性（Observability）

- **追踪**：`TraceContext` 作为可选扩展；上层拦截器负责注入。
- **审计字段**：`actor/consent/causation_id/correlation_id/partition_key/produced_at/schema_ver` 视为**最小审计集合**。
- **证据**：具体执行证据（如工具执行日志）由上层定义为 `payload` 内部结构或外部证据链引用（本 crate 不介入）。

------

## 10. 测试与验收（Testing & Acceptance）

### 10.1 单元测试（本 crate）

- **结构校验**：空字段/非法 SemVer/空分区键返回 `ValidateError`。
- **序列化一致性**：常见类型序列化/反序列化应等价。
- **Schema 生成**（feature=`schema`）：能生成非空 JSON Schema，含必填字段。

### 10.2 合同/兼容测试（在 `soulbase-contract-testkit`）

- **兼容矩阵**：`MAJOR` 变更触发失败用例；`MINOR/PATCH` 在旧消费者模式下通过。
- **跨语言反序列化**：TS/Go/Java 对 `Envelope<ExamplePayload>` 的反序列化成功率 100%。

------

## 11. 版本化与迁移（Versioning & Migration）

- **SemVer 严格执行**：
  - `PATCH`：文档/注释/非破坏性 bugfix；
  - `MINOR`：只可**新增可选字段**或新增枚举**向后兼容**分支；
  - `MAJOR`：删除/重命名/改变语义，必须配迁移指南 + 兼容垫片（建议由上层做映射）。
- **Schema 锚点**：在 `Envelope<T>` 持有 `schema_ver`（字符串），由**上层负载的 Schema 版本**写入；本 crate 不维护负载版本生命周期。
- **弃用流程**：字段标注 `#[deprecated(note = "...", since = "x.y.z")]`，至少跨 **两个 MINOR** 周期再移除。

------

## 12. 参考实现片段与示例

### 12.1 Prelude 与便捷构造

```rust
// prelude.rs
pub use crate::{
  Id, CausationId, CorrelationId, Timestamp, TenantId,
  Subject, SubjectKind, Scope, Consent, TraceContext,
  Envelope,
  traits::{Versioned, Partitioned, Auditable, Causal},
  validate::{Validate, ValidateError},
};

// envelope.rs（便捷构造器，不生成 id/时间戳，仅组装）
impl<T> Envelope<T> {
    pub fn new(
        envelope_id: Id,
        produced_at: Timestamp,
        partition_key: String,
        actor: Subject,
        schema_ver: impl Into<String>,
        payload: T
    ) -> Self {
        Self {
            envelope_id, produced_at, partition_key,
            causation_id: None, correlation_id: None,
            actor, consent: None,
            schema_ver: schema_ver.into(),
            trace: None,
            payload,
        }
    }

    pub fn with_correlation(mut self, corr: CorrelationId) -> Self { self.correlation_id = Some(corr); self }
    pub fn with_causation(mut self, caus: CausationId) -> Self { self.causation_id = Some(caus); self }
    pub fn with_consent(mut self, c: Consent) -> Self { self.consent = Some(c); self }
    pub fn with_trace(mut self, t: TraceContext) -> Self { self.trace = Some(t); self }
}
```

### 12.2 示例 JSON（示意）

```json
{
  "envelope_id": "env_01Hxx...",
  "produced_at": 1726123456789,
  "partition_key": "tenantA:conv_9f2c",
  "causation_id": "cause_abc",
  "correlation_id": "corr_root",
  "actor": {
    "kind": "User",
    "subject_id": "user_123",
    "tenant": "tenantA",
    "claims": { "roles": ["member"], "client_id": "soulbrowser" }
  },
  "consent": {
    "scopes": [{ "resource": "tool:browser", "action": "invoke" }],
    "expires_at": 1726127056789,
    "purpose": "web research"
  },
  "schema_ver": "1.0.0",
  "trace": { "trace_id": "t-1", "span_id": "s-1", "baggage": {} },
  "payload": { "type": "IncomingCommand", "args": { "q": "hello" } }
}
```

------

## 13. 开放事项（Open Issues）

- 是否引入统一 `NonEmptyString` 新类型约束常用字符串字段（可减少运行时校验）。
- `serde_borrow` 的零拷贝路由在多线程/异步下的收益评估。
- TraceContext 是否内置兼容 W3C TraceContext（`traceparent`/`tracestate`）的直通字段。
- 与 `soulbase-errors` 的错误映射整合时机与策略。

------

## 14. 里程碑与交付物

- **M1**：完成上述 crate 目录与类型；校验器；基本单测；`schema` feature 输出 JSON Schema。
- **M2**：接入 `soulbase-contract-testkit` 的兼容测试；在 `soulbase-interceptors` 中落入站校验。
- **M3**：发布 v1.0.0；生成 TS/Go 绑定工件并用于 1–2 个下游服务集成验收。

------

> 本技术设计与《功能逻辑规约》一一对应。若你确认没有遗漏，我可以继续提交 **最小可运行骨架（crate 初始化代码）** 或转入下一个模块的规约文档输出。
