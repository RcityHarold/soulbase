**每个模块=三份产物** → ①功能逻辑规约 ②技术设计 ③最小可运行骨架（代码）。

下面给出本模块 `sb-types` 的**RIS 代码骨架**（可直接放到 `soul-base/crates/sb-types/`），开箱即可 `cargo check/test`。

------

## 目录结构（建议）

```
soul-base/
├─ Cargo.toml                  # workspace 根（示例在文末）
└─ crates/
   └─ sb-types/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ id.rs
      │  ├─ time.rs
      │  ├─ tenant.rs
      │  ├─ subject.rs
      │  ├─ scope.rs
      │  ├─ trace.rs
      │  ├─ envelope.rs
      │  ├─ traits.rs
      │  ├─ validate.rs
      │  └─ prelude.rs
      └─ tests/
         └─ basic.rs
```

------

## `crates/sb-types/Cargo.toml`

```toml
[package]
name = "sb-types"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "Data contract primitives for the Soul platform (IDs, Subject, Scope, Consent, Envelope...)."
repository = "https://example.com/soul-base"

[features]
schema = ["schemars"]
serde_borrow = []
uuid = ["dep:uuid"]

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
semver = "1"
schemars = { version = "0.8", optional = true, features = ["serde_json"] }
uuid = { version = "1", optional = true, features = ["v4"] }

[dev-dependencies]
serde_json = "1"
```

------

## `src/lib.rs`

```rust
pub mod id;
pub mod time;
pub mod tenant;
pub mod subject;
pub mod scope;
pub mod trace;
pub mod envelope;
pub mod traits;
pub mod validate;
pub mod prelude;

#[cfg(feature = "schema")]
pub mod schema_gen {
    use super::*;
    use schemars::schema::RootSchema;
    use schemars::schema_for;

    pub fn envelope_schema<T>() -> RootSchema
    where
        T: schemars::JsonSchema,
    {
        schema_for!(envelope::Envelope<T>)
    }
}
```

------

## `src/id.rs`

```rust
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

impl Id {
    pub fn new_random() -> Self {
        #[cfg(feature = "uuid")] {
            return Self(uuid::Uuid::new_v4().to_string());
        }
        // 非 uuid 特性时，仅用于演示；生产建议统一 Id 生成服务
        Self(format!("id_{}", nanoid::nanoid!()))
    }
}
```

> 如不想引入 `nanoid`，可去掉 `new_random`，由上层生成 Id。

------

## `src/time.rs`

```rust
use serde::{Deserialize, Serialize};
#[cfg(feature = "schema")] use schemars::JsonSchema;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Timestamp(pub i64); // ms since epoch, UTC
```

------

## `src/tenant.rs`

```rust
use serde::{Deserialize, Serialize};
#[cfg(feature = "schema")] use schemars::JsonSchema;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TenantId(pub String);
```

------

## `src/subject.rs`

```rust
use crate::tenant::TenantId;
use crate::id::Id;
use serde::{Deserialize, Serialize};
#[cfg(feature = "schema")] use schemars::JsonSchema;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum SubjectKind { User, Service, Agent }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Subject {
    pub kind: SubjectKind,
    pub subject_id: Id,
    pub tenant: TenantId,
    #[serde(default)]
    pub claims: serde_json::Map<String, serde_json::Value>,
}
```

------

## `src/scope.rs`

```rust
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
#[cfg(feature = "schema")] use schemars::JsonSchema;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Scope {
    pub resource: String,
    pub action: String,
    #[serde(default)]
    pub attrs: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Consent {
    #[serde(default)]
    pub scopes: Vec<Scope>,
    pub expires_at: Option<Timestamp>,
    #[serde(default)]
    pub purpose: Option<String>,
}
```

------

## `src/trace.rs`

```rust
use serde::{Deserialize, Serialize};
#[cfg(feature = "schema")] use schemars::JsonSchema;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TraceContext {
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    #[serde(default)]
    pub baggage: serde_json::Map<String, serde_json::Value>,
}
```

------

## `src/envelope.rs`

```rust
use serde::{Deserialize, Serialize};
#[cfg(feature = "schema")] use schemars::JsonSchema;

use crate::{
    id::{Id, CausationId, CorrelationId},
    time::Timestamp,
    subject::Subject,
    scope::Consent,
    trace::TraceContext,
};

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

    pub schema_ver: String,

    #[serde(default)]
    pub trace: Option<TraceContext>,

    pub payload: T,
}

impl<T> Envelope<T> {
    pub fn new(
        envelope_id: Id,
        produced_at: Timestamp,
        partition_key: String,
        actor: Subject,
        schema_ver: impl Into<String>,
        payload: T,
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
    pub fn with_correlation(mut self, c: CorrelationId) -> Self { self.correlation_id = Some(c); self }
    pub fn with_causation(mut self, c: CausationId) -> Self { self.causation_id = Some(c); self }
}
```

------

## `src/traits.rs`

```rust
use crate::{subject::Subject, time::Timestamp, envelope::Envelope, id::{CausationId, CorrelationId}};

pub trait Versioned { fn schema_version(&self) -> &str; }
pub trait Partitioned { fn partition_key(&self) -> &str; }
pub trait Auditable { fn actor(&self) -> &Subject; fn produced_at(&self) -> Timestamp; }
pub trait Causal {
    fn causation_id(&self) -> Option<&CausationId>;
    fn correlation_id(&self) -> Option<&CorrelationId>;
}

impl<T> Versioned for Envelope<T> { fn schema_version(&self) -> &str { &self.schema_ver } }
impl<T> Partitioned for Envelope<T> { fn partition_key(&self) -> &str { &self.partition_key } }
impl<T> Auditable for Envelope<T> {
    fn actor(&self) -> &Subject { &self.actor }
    fn produced_at(&self) -> Timestamp { self.produced_at }
}
impl<T> Causal for Envelope<T> {
    fn causation_id(&self) -> Option<&CausationId> { self.causation_id.as_ref() }
    fn correlation_id(&self) -> Option<&CorrelationId> { self.correlation_id.as_ref() }
}
```

------

## `src/validate.rs`

```rust
use thiserror::Error;
use crate::{subject::Subject, envelope::Envelope};
use semver::Version;

#[derive(Debug, Error)]
pub enum ValidateError {
    #[error("empty_field:{0}")]
    EmptyField(&'static str),
    #[error("invalid_semver:{0}")]
    InvalidSemVer(String),
}

pub trait Validate { fn validate(&self) -> Result<(), ValidateError>; }

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
        if Version::parse(&self.schema_ver).is_err() {
            return Err(ValidateError::InvalidSemVer(self.schema_ver.clone()));
        }
        self.actor.validate()?;
        Ok(())
    }
}
```

------

## `src/prelude.rs`

```rust
pub use crate::{
  id::{Id, CausationId, CorrelationId},
  time::Timestamp,
  tenant::TenantId,
  subject::{Subject, SubjectKind},
  scope::{Scope, Consent},
  trace::TraceContext,
  envelope::Envelope,
  traits::{Versioned, Partitioned, Auditable, Causal},
  validate::{Validate, ValidateError},
};
```

------

## `tests/basic.rs`

```rust
use sb_types::prelude::*;

#[test]
fn envelope_validates() {
    let actor = Subject {
        kind: SubjectKind::User,
        subject_id: Id("user_1".into()),
        tenant: TenantId("tenantA".into()),
        claims: Default::default(),
    };
    let env = Envelope::new(
        Id("env_1".into()),
        Timestamp(1_726_000_000_000),
        "tenantA:conv_1".into(),
        actor,
        "1.0.0",
        serde_json::json!({"hello":"world"})
    );
    assert!(env.validate().is_ok());
}
```

------

## `README.md`（简版）

~~~markdown
# sb-types

Data contract primitives for the Soul platform:
IDs, Subject/Tenant, Scope/Consent, TraceContext, and Envelope<T>.

## Build
```bash
cargo check
cargo test
~~~

## JSON Schema (optional)

~~~bash
cargo test --features schema
---

## （可选）workspace 根 `Cargo.toml`（示例）

```toml
[workspace]
members = ["crates/sb-types"]
resolver = "2"

[workspace.package]
edition = "2021"
license = "Apache-2.0"
~~~

------

### 这样做的收益

- 把“规范→设计→代码”打通成**同构三件套**，每个模块启动成本可控，避免口径漂移。
- 所有仓都用相同骨架：统一 feature、lint、测试与 Schema 产物路径，**极大加速集成**。

