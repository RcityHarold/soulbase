下面是 **SB-04-RIS：`soulbase-auth` 最小可运行骨架**。
 它与我们前两份文档（SB-04 功能规约 & 技术设计）**一一对齐**，并延续原始架构文档的精华口径（SSoT、deny-by-default、证据可审计、额度与授权一体化）。将以下内容放入 `soul-base/crates/soulbase-auth/` 后，可直接运行 `cargo check && cargo test`。

------

## 目录结构

```
soul-base/
└─ crates/
   └─ soulbase-auth/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ model.rs
      │  ├─ authn/
      │  │  ├─ mod.rs
      │  │  └─ oidc.rs
      │  ├─ attr/
      │  │  └─ mod.rs
      │  ├─ pdp/
      │  │  ├─ mod.rs
      │  │  └─ local.rs
      │  ├─ quota/
      │  │  ├─ mod.rs
      │  │  └─ memory.rs
      │  ├─ consent/
      │  │  └─ mod.rs
      │  ├─ cache/
      │  │  ├─ mod.rs
      │  │  └─ memory.rs
      │  ├─ intercept.rs
      │  ├─ errors.rs
      │  ├─ observe.rs
      │  ├─ events.rs
      │  └─ prelude.rs
      └─ tests/
         └─ basic.rs
```

------

## `Cargo.toml`

```toml
[package]
name = "soulbase-auth"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "AuthN · AuthZ · Quota SPI for the Soul platform"
repository = "https://example.com/soul-base"

[features]
default = ["authn-oidc", "pdp-local", "quota-memory", "cache-memory"]
authn-oidc = []
pdp-local = []
quota-memory = []
cache-memory = []

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
async-trait = "0.1"
thiserror = "1"
parking_lot = "0.12"
ahash = "0.8"            # 轻量哈希
base64 = "0.22"

# 平台内
sb-types = { path = "../sb-types", version = "1.0.0" }
soulbase-errors = { path = "../soulbase-errors", version = "1.0.0" }

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
serde_json = "1"
```

------

## `src/lib.rs`

```rust
pub mod model;
pub mod authn;
pub mod attr;
pub mod pdp;
pub mod quota;
pub mod consent;
pub mod cache;
pub mod intercept;
pub mod errors;
pub mod observe;
pub mod events;
pub mod prelude;
```

------

## `src/model.rs`

```rust
use serde::{Serialize, Deserialize};
use sb_types::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct ResourceUrn(pub String); // e.g. "soul:tool:browser"

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum Action { Read, Write, Invoke, List, Admin, Configure }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthzRequest {
  pub subject: Subject,
  pub resource: ResourceUrn,
  pub action: Action,
  #[serde(default)]
  pub attrs: serde_json::Value,
  #[serde(default)]
  pub consent: Option<Consent>,
  #[serde(default)]
  pub correlation_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Obligation {
  pub kind: String,                 // "mask" | "redact" | "watermark" | ...
  #[serde(default)]
  pub params: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Decision {
  pub allow: bool,
  #[serde(default)]
  pub reason: Option<String>,
  #[serde(default)]
  pub obligations: Vec<Obligation>,
  #[serde(default)]
  pub evidence: serde_json::Value,
  #[serde(default)]
  pub cache_ttl_ms: u32,
}

#[derive(Clone, Debug)]
pub enum AuthnInput {
  BearerJwt(String),
  ApiKey(String),
  MTls { peer_dn: String, san: Vec<String> },
  ServiceToken(String),
}

// ---- 配额相关 ----
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct QuotaKey {
  pub tenant: TenantId,
  pub subject_id: Id,
  pub resource: ResourceUrn,
  pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuotaOutcome { Allowed, RateLimited, BudgetExceeded }

// ---- 决策缓存 Key ----
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DecisionKey {
  pub tenant: TenantId,
  pub subject_id: Id,
  pub resource: ResourceUrn,
  pub action: Action,
  pub attrs_fingerprint: u64,
}

pub fn decision_key(req: &AuthzRequest, merged_attrs: &serde_json::Value) -> DecisionKey {
  use ahash::AHasher;
  use std::hash::{Hasher, Hash};
  let mut hasher = AHasher::default();
  hasher.write(serde_json::to_string(merged_attrs).unwrap_or_default().as_bytes());
  DecisionKey {
    tenant: req.subject.tenant.clone(),
    subject_id: req.subject.subject_id.clone(),
    resource: req.resource.clone(),
    action: req.action.clone(),
    attrs_fingerprint: hasher.finish(),
  }
}

/// 简化版：从 attrs 中读取 cost，默认 1
pub fn cost_from_attrs(attrs: &serde_json::Value) -> u64 {
  attrs.get("cost").and_then(|v| v.as_u64()).unwrap_or(1)
}
```

------

## `src/authn/mod.rs`

```rust
use async_trait::async_trait;
use crate::errors::AuthError;
use crate::model::AuthnInput;
use sb_types::prelude::*;

pub mod oidc;

#[async_trait]
pub trait Authenticator: Send + Sync {
  async fn authenticate(&self, input: AuthnInput) -> Result<Subject, AuthError>;
}
```

### `src/authn/oidc.rs`（最小 OIDC Stub，对接 Soul-Auth 的占位）

```rust
use super::*;
use sb_types::prelude::*;

/// 最小实现：演示把 Bearer 解析为 "sub@tenant" 的格式；生产中应调用 Discovery/JWKS 验签
pub struct OidcAuthenticatorStub;

#[async_trait::async_trait]
impl super::Authenticator for OidcAuthenticatorStub {
  async fn authenticate(&self, input: AuthnInput) -> Result<Subject, crate::errors::AuthError> {
    match input {
      AuthnInput::BearerJwt(tok) => {
        // 仅演示：期望 "sub@tenant"；否则视为未认证
        let (sub, tenant) = tok.split_once('@')
          .ok_or_else(|| crate::errors::unauthenticated("Invalid bearer format"))?;
        Ok(Subject {
          kind: sb_types::prelude::SubjectKind::User,
          subject_id: Id(sub.to_string()),
          tenant: TenantId(tenant.to_string()),
          claims: Default::default(),
        })
      }
      _ => Err(crate::errors::unauthenticated("Unsupported authn input")),
    }
  }
}
```

------

## `src/attr/mod.rs`

```rust
use async_trait::async_trait;
use crate::errors::AuthError;
use crate::model::AuthzRequest;

#[async_trait]
pub trait AttributeProvider: Send + Sync {
  async fn augment(&self, _req: &AuthzRequest) -> Result<serde_json::Value, AuthError>;
}

/// 默认空属性提供者
pub struct DefaultAttributeProvider;
#[async_trait]
impl AttributeProvider for DefaultAttributeProvider {
  async fn augment(&self, _req: &AuthzRequest) -> Result<serde_json::Value, AuthError> {
    Ok(serde_json::json!({}))
  }
}
```

------

## `src/pdp/mod.rs`

```rust
use async_trait::async_trait;
use crate::errors::AuthError;
use crate::model::{AuthzRequest, Decision};

pub mod local;

#[async_trait]
pub trait Authorizer: Send + Sync {
  async fn decide(&self, req: &AuthzRequest, merged_attrs: &serde_json::Value) -> Result<Decision, AuthError>;
}
```

### `src/pdp/local.rs`（本地策略：Deny-by-default，允许 attrs.allow=true）

```rust
use super::*;
use serde_json::Value;

/// 最小可用：deny-by-default；当 merged_attrs["allow"] == true 时放行
pub struct LocalAuthorizer;

#[async_trait::async_trait]
impl super::Authorizer for LocalAuthorizer {
  async fn decide(&self, _req: &AuthzRequest, merged_attrs: &Value) -> Result<Decision, crate::errors::AuthError> {
    let allow = merged_attrs.get("allow").and_then(|v| v.as_bool()).unwrap_or(false);
    Ok(Decision {
      allow,
      reason: if allow { None } else { Some("deny-by-default".into()) },
      obligations: vec![],
      evidence: serde_json::json!({"policy":"local","rule": if allow {"allow"} else {"deny"}}),
      cache_ttl_ms: if allow { 1000 } else { 0 },
    })
  }
}
```

------

## `src/quota/mod.rs`

```rust
use async_trait::async_trait;
use crate::errors::AuthError;
use crate::model::{QuotaKey, QuotaOutcome};

pub mod memory;

#[async_trait]
pub trait QuotaStore: Send + Sync {
  async fn check_and_consume(&self, key: &QuotaKey, cost: u64) -> Result<QuotaOutcome, AuthError>;
}
```

### `src/quota/memory.rs`（内存配额：示例总是允许）

```rust
use super::*;
pub struct MemoryQuota;
#[async_trait::async_trait]
impl super::QuotaStore for MemoryQuota {
  async fn check_and_consume(&self, _key: &crate::model::QuotaKey, _cost: u64) -> Result<crate::model::QuotaOutcome, crate::errors::AuthError> {
    Ok(crate::model::QuotaOutcome::Allowed)
  }
}
```

------

## `src/consent/mod.rs`

```rust
use async_trait::async_trait;
use sb_types::prelude::Consent;
use crate::model::AuthzRequest;
use crate::errors::AuthError;

#[async_trait]
pub trait ConsentVerifier: Send + Sync {
  async fn verify(&self, consent: &Consent, _req: &AuthzRequest) -> Result<bool, AuthError>;
}

/// 最小实现：一律视为有效（演示用；生产需校验范围/有效期/签名）
pub struct AlwaysOkConsent;
#[async_trait::async_trait]
impl ConsentVerifier for AlwaysOkConsent {
  async fn verify(&self, _consent: &Consent, _req: &AuthzRequest) -> Result<bool, AuthError> {
    Ok(true)
  }
}
```

------

## `src/cache/mod.rs`

```rust
use async_trait::async_trait;
use crate::model::{DecisionKey, Decision};

pub mod memory;

#[async_trait]
pub trait DecisionCache: Send + Sync {
  async fn get(&self, key: &DecisionKey) -> Option<Decision>;
  async fn put(&self, key: DecisionKey, d: &Decision);
  async fn revoke(&self, _subject_id: &sb_types::prelude::Id) { /* optional */ }
}
```

### `src/cache/memory.rs`

```rust
use super::*;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::time::{Duration, Instant};

struct Entry { d: Decision, exp: Instant }

pub struct MemoryDecisionCache {
  ttl_default: Duration,
  map: RwLock<HashMap<DecisionKey, Entry>>,
}

impl MemoryDecisionCache {
  pub fn new(ttl_ms: u64) -> Self {
    Self { ttl_default: Duration::from_millis(ttl_ms), map: RwLock::new(HashMap::new()) }
  }
}

#[async_trait::async_trait]
impl super::DecisionCache for MemoryDecisionCache {
  async fn get(&self, key: &DecisionKey) -> Option<Decision> {
    let now = Instant::now();
    let map = self.map.read();
    map.get(key).and_then(|e| if e.exp > now { Some(e.d.clone()) } else { None })
  }
  async fn put(&self, key: DecisionKey, d: &Decision) {
    let mut map = self.map.write();
    let ttl = if d.cache_ttl_ms == 0 { self.ttl_default } else { Duration::from_millis(d.cache_ttl_ms as u64) };
    map.insert(key, Entry { d: d.clone(), exp: Instant::now() + ttl });
  }
  async fn revoke(&self, _subject_id: &sb_types::prelude::Id) {
    // 简化：留作扩展
  }
}
```

------

## `src/intercept.rs`

```rust
/// 与 soulbase-interceptors 的对接约定占位：
/// - 从请求上下文提取 bearer / tenant / trace / correlation_id；
/// - 调用 AuthFacade::authorize(...)；
/// - 把决策结果与证据摘要挂入 Envelope 审计（由上层实现）。
```

------

## `src/errors.rs`

```rust
use thiserror::Error;
use soulbase_errors::prelude::*;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct AuthError(pub ErrorObj);

impl AuthError {
  pub fn into_inner(self) -> ErrorObj { self.0 }
}

pub fn unauthenticated(msg: &str) -> AuthError {
  AuthError(ErrorBuilder::new(codes::AUTH_UNAUTHENTICATED).user_msg("Please sign in.").dev_msg(msg).build())
}
pub fn forbidden(msg: &str) -> AuthError {
  AuthError(ErrorBuilder::new(codes::AUTH_FORBIDDEN).user_msg("Forbidden.").dev_msg(msg).build())
}
pub fn rate_limited() -> AuthError {
  AuthError(ErrorBuilder::new(codes::QUOTA_RATELIMIT).user_msg("Too many requests. Please retry later.").build())
}
pub fn budget_exceeded() -> AuthError {
  AuthError(ErrorBuilder::new(codes::QUOTA_BUDGET).user_msg("Budget exceeded.").build())
}
pub fn policy_deny(msg: &str) -> AuthError {
  AuthError(ErrorBuilder::new(codes::POLICY_DENY_TOOL).user_msg("Operation denied by policy.").dev_msg(msg).build())
}
pub fn provider_unavailable(msg: &str) -> AuthError {
  AuthError(ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE).user_msg("Upstream provider unavailable.").dev_msg(msg).build())
}
```

------

## `src/observe.rs`

```rust
use std::collections::BTreeMap;
use crate::model::{ResourceUrn, Action, Decision};

pub fn labels(tenant: &str, res: &ResourceUrn, act: &Action, allow: bool) -> BTreeMap<&'static str, String> {
  let mut m = BTreeMap::new();
  m.insert("tenant", tenant.to_string());
  m.insert("resource", res.0.clone());
  m.insert("action", format!("{:?}", act));
  m.insert("allow", allow.to_string());
  m
}
```

------

## `src/events.rs`

```rust
use serde::{Serialize, Deserialize};
use sb_types::prelude::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthDecisionEvent {
  pub subject_id: Id,
  pub tenant: TenantId,
  pub resource: String,
  pub action: String,
  pub allow: bool,
  pub policy_hash: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuotaEvent {
  pub tenant: TenantId,
  pub subject_id: Id,
  pub resource: String,
  pub action: String,
  pub cost: u64,
  pub outcome: String,
}
```

------

## `src/prelude.rs`

```rust
pub use crate::model::{
  ResourceUrn, Action, AuthzRequest, Obligation, Decision,
  AuthnInput, QuotaKey, QuotaOutcome, DecisionKey, decision_key, cost_from_attrs
};
pub use crate::authn::{Authenticator, oidc::OidcAuthenticatorStub};
pub use crate::attr::{AttributeProvider, DefaultAttributeProvider};
pub use crate::pdp::{Authorizer, local::LocalAuthorizer};
pub use crate::quota::{QuotaStore, memory::MemoryQuota};
pub use crate::consent::{ConsentVerifier, AlwaysOkConsent};
pub use crate::cache::{DecisionCache, memory::MemoryDecisionCache};
pub use crate::errors::{AuthError};
```

------

## 高层门面（在 TD 中定义，这里给可运行实现）

在 RIS 中提供一个最小门面，便于测试与集成演示：

```rust
// 在 src/lib.rs 之后追加
use crate::prelude::*;
use sb_types::prelude::*;

pub struct AuthFacade {
  pub authenticator: Box<dyn Authenticator>,
  pub attr_provider: Box<dyn AttributeProvider>,
  pub authorizer: Box<dyn Authorizer>,
  pub consent: Box<dyn ConsentVerifier>,
  pub quota: Box<dyn QuotaStore>,
  pub cache: Box<dyn DecisionCache>,
}

impl AuthFacade {
  pub fn minimal() -> Self {
    AuthFacade {
      authenticator: Box::new(OidcAuthenticatorStub),
      attr_provider: Box::new(DefaultAttributeProvider),
      authorizer: Box::new(LocalAuthorizer),
      consent: Box::new(AlwaysOkConsent),
      quota: Box::new(MemoryQuota),
      cache: Box::new(MemoryDecisionCache::new(1000)),
    }
  }

  pub async fn authorize(
    &self,
    token_or_ctx: AuthnInput,
    resource: ResourceUrn,
    action: Action,
    attrs: serde_json::Value,
    consent: Option<Consent>,
    correlation_id: Option<String>,
  ) -> Result<Decision, AuthError> {
    // 1) AuthN
    let subject = self.authenticator.authenticate(token_or_ctx).await?;

    // 2) 构造请求 + 补充属性
    let req = AuthzRequest { subject: subject.clone(), resource, action, attrs, consent, correlation_id };
    let merged_attrs = self.attr_provider.augment(&req).await.unwrap_or(serde_json::json!({}));

    // 3) 缓存
    let dkey = decision_key(&req, &merged_attrs);
    if let Some(hit) = self.cache.get(&dkey).await { return Ok(hit); }

    // 4) PDP
    let mut decision = self.authorizer.decide(&req, &merged_attrs).await?;

    // 5) 同意（最小实现：AlwaysOkConsent 总为真）
    if decision.allow {
      // 6) 配额
      let cost = cost_from_attrs(&merged_attrs);
      let outcome = self.quota.check_and_consume(&QuotaKey {
        tenant: req.subject.tenant.clone(),
        subject_id: req.subject.subject_id.clone(),
        resource: req.resource.clone(),
        action: req.action.clone(),
      }, cost).await?;
      use crate::model::QuotaOutcome::*;
      match outcome {
        Allowed => { /* pass */ }
        RateLimited => return Err(crate::errors::rate_limited()),
        BudgetExceeded => return Err(crate::errors::budget_exceeded()),
      }
    }

    if decision.cache_ttl_ms > 0 { self.cache.put(dkey, &decision).await; }
    Ok(decision)
  }
}
```

------

## `tests/basic.rs`

```rust
use soulbase_auth::prelude::*;
use soulbase_auth::AuthFacade;

#[tokio::test]
async fn authorize_allow_path() {
    let facade = AuthFacade::minimal();
    // Bearer 形如 "sub@tenant"
    let token = AuthnInput::BearerJwt("user_1@tenantA".into());
    let res = ResourceUrn("soul:tool:browser".into());

    // 本地 PDP 规则：merged_attrs["allow"] == true 则放行
    let decision = facade.authorize(
        token, res.clone(), Action::Invoke,
        serde_json::json!({"allow": true, "cost": 2}),
        None, Some("corr-1".into())
    ).await.expect("authorized");

    assert!(decision.allow);

    // 第二次命中缓存
    let decision2 = facade.authorize(
        AuthnInput::BearerJwt("user_1@tenantA".into()),
        res, Action::Invoke,
        serde_json::json!({"allow": true, "cost": 2}),
        None, Some("corr-1".into())
    ).await.expect("authorized");
    assert!(decision2.allow);
}

#[tokio::test]
async fn authorize_deny_by_default() {
    let facade = AuthFacade::minimal();
    let token = AuthnInput::BearerJwt("user_2@tenantB".into());
    let res = ResourceUrn("soul:model:gpt-4o".into());

    let decision = facade.authorize(
        token, res, Action::Invoke,
        serde_json::json!({}), // 没有 allow=true
        None, None
    ).await.expect("decision ok");

    assert_eq!(decision.allow, false);
    assert_eq!(decision.reason.as_deref(), Some("deny-by-default"));
}
```

------

## `README.md`（简版）

~~~markdown
# soulbase-auth (RIS)

Minimal runnable skeleton for AuthN · AuthZ · Quota SPI.

- AuthN: OIDC stub (`BearerJwt "sub@tenant" -> Subject`)
- PDP: Local (deny-by-default; allow when `attrs.allow=true`)
- Quota: In-memory (always Allowed)
- Cache: In-memory TTL

## Run
```bash
cargo check
cargo test
~~~

## Next

- Replace OIDC stub with real Discovery/JWKS validator (Soul-Auth).
- Add OPA/Cedar PDP adapters.
- Add Redis-backed Quota & DecisionCache.

```
---

### 对齐说明

- **同频**：严格保留“Subject/Scope/Consent/Decision/Evidence、deny-by-default、一次性授权+扣额、证据与缓存撤销”的不变式口径。  
- **最小即用**：可直接被 `soulbase-interceptors` 和内核调用进行端到端集成验证；后续仅需替换/扩展适配器即可上生产路径。  
- **契约化**：错误统一走 `soulbase-errors` 稳定码；所有类型 `serde` 可序列化，便于事件审计与跨语言集成。

如果你认可这份骨架，我们就继续沿既定顺序推进下一个模块（`soulbase-interceptors` 功能规约）。
::contentReference[oaicite:0]{index=0}
```
