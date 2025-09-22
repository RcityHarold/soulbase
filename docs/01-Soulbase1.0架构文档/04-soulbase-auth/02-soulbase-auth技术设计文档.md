# 文档 SB-04-TD：`soulbase-auth` 技术设计（Technical Design）

> 对应功能规约：SB-04（AuthN · AuthZ · Quota）
>  目标：给出 **crate 结构、核心类型、SPI/Traits、适配器扩展点、决策与证据模型、缓存与撤销、与 Soul-Auth / Soul-Hub / soulbase-qos / soulbase-interceptors 的协同口径**。
>  语言：Rust（`serde` 基础）。本 TD 只给接口与行为约束，不包含 RIS 代码骨架。

------

## 1. Crate 结构与模块

```
soulbase-auth/
  src/
    lib.rs
    model.rs           # Subject映射补充、Resource/Action/Scope/Consent别名、Decision/Evidence
    authn/             # 认证：OIDC/JWT、API Key、mTLS、服务间Token
      mod.rs
      oidc.rs          # 对接 Soul-Auth（Discovery/JWKS）
      apikey.rs
      mtls.rs
      service_token.rs
    attr/              # AttributeProvider：环境/资源属性装载
      mod.rs
    pdp/               # Authorizer（PDP适配）：本地/OPA/Cedar/自研
      mod.rs
      local.rs
      opa.rs
      cedar.rs
    quota/             # QuotaStore：配额/预算检查
      mod.rs
      memory.rs
      redis.rs         # 可选
    consent/           # ConsentVerifier：验证同意凭据与范围
      mod.rs
    cache/             # DecisionCache：TTL与撤销（revocation）
      mod.rs
      memory.rs
      redis.rs         # 可选
    intercept.rs       # 与 soulbase-interceptors 的对接约定（上下文键、Envelope注入位）
    errors.rs          # 与 soulbase-errors 的稳定映射
    observe.rs         # 指标标签导出、计时助手
    events.rs          # Envelope<AuthDecisionEvent/QuotaEvent/...>
    prelude.rs
```

**Feature Flags（建议）**

- `authn-oidc`（默认启用）：对接 **Soul-Auth**（Discovery/JWKS 缓存/轮换）。
- `pdp-opa` / `pdp-cedar` / `pdp-local`（默认 `local`）。
- `quota-redis`、`cache-redis`（分布式配额与撤销）。
- `mtls`、`apikey`、`service-token`（按需）。

**外部依赖（最小集合，TD 层列举，不强制具体版本）**

- `serde` / `serde_json` / `thiserror`
- （可选）`jsonwebtoken` 或 `josekit`（OIDC/JWT）
- （可选）`reqwest`（OPA/Cedar 远程）
- `sb-types`（Subject/Scope/Consent/Envelope）
- `soulbase-errors`（稳定错误域）
- `soulbase-qos`（预算度量接口，占位）

------

## 2. 核心类型（`model.rs`）

### 2.1 资源与动作命名

```rust
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ResourceUrn(pub String); // "soul:tool:browser", "soul:model:gpt-4o"

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Action { Read, Write, Invoke, List, Admin, Configure }
```

> 约束：资源 URN 与动作集合是**协议契约**；各业务域可扩展但不得改变语义。

### 2.2 授权请求与决策

```rust
use sb_types::{prelude::*, Envelope};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AuthzRequest {
  pub subject: Subject,                  // 由 AuthN 映射
  pub resource: ResourceUrn,
  pub action: Action,
  pub attrs: serde_json::Value,          // 环境/资源属性（ABAC）
  pub consent: Option<Consent>,          // 高风险操作的明示同意
  pub correlation_id: Option<String>,    // 贯通链路
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Obligation {
  pub kind: String,                      // "mask"|"redact"|"watermark"|...
  pub params: serde_json::Value,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Decision {
  pub allow: bool,
  pub reason: Option<String>,
  pub obligations: Vec<Obligation>,
  pub evidence: serde_json::Value,       // 命中规则、策略版本哈希、输入摘要
  pub cache_ttl_ms: u32,                 // 本地缓存建议
}
```

------

## 3. SPI 抽象（核心 Traits）

### 3.1 认证（AuthN）

```rust
#[async_trait::async_trait]
pub trait Authenticator: Send + Sync {
  /// 输入：来之 Soul-Hub 透传的 bearer / mTLS / API Key / STS
  /// 输出：统一 Subject（tenant/claims 已映射）
  async fn authenticate(&self, token_or_ctx: AuthnInput) -> Result<Subject, AuthError>;
}

pub enum AuthnInput {
  BearerJwt(String),
  ApiKey(String),
  MTls { peer_dn: String, san: Vec<String> },
  ServiceToken(String),
}
```

**OIDC 映射（对接 Soul-Auth）**

- Discovery：`/.well-known/openid-configuration` + `jwks_uri`，缓存 JWK，**自动轮换**（新失败→回退旧钥）。
- 声明映射：`sub→subject_id`, `tenant→TenantId`, `roles/permissions→claims`，可携带 `consents[]` 摘要。
- 错误：`AUTH.UNAUTHENTICATED`、`AUTH.CLAIM_INVALID`、`AUTH.TOKEN_EXPIRED`。

### 3.2 属性提供（AttributeProvider）

```rust
#[async_trait::async_trait]
pub trait AttributeProvider: Send + Sync {
  async fn augment(&self, req: &AuthzRequest) -> Result<serde_json::Value, AuthError>;
}
// 典型：地理/设备/风险评分、资源所有者、密级标签
```

### 3.3 授权（PDP/Authorizer）

```rust
#[async_trait::async_trait]
pub trait Authorizer: Send + Sync {
  async fn decide(&self, req: &AuthzRequest, attrs: &serde_json::Value) -> Result<Decision, AuthError>;
}
```

**适配器**

- `LocalAuthorizer`：本地规则（RBAC/表驱动）。
- `OpaAuthorizer`：HTTP/rego 调用：`input = {subject, resource, action, attrs, consent}` → `allow/obligations/evidence/ttl`。
- `CedarAuthorizer`：嵌入或远程执行 Cedar policy，返回同样结构。

### 3.4 配额/预算（QuotaStore）

```rust
#[async_trait::async_trait]
pub trait QuotaStore: Send + Sync {
  /// 原子检查并扣减
  async fn check_and_consume(&self, key: &QuotaKey, cost: u64) -> Result<QuotaOutcome, AuthError>;
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QuotaKey { pub tenant: TenantId, pub subject_id: Id, pub resource: ResourceUrn, pub action: Action }

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum QuotaOutcome { Allowed, RateLimited, BudgetExceeded }
```

> 典型用法：LLM tokens / 工具调用次数 / 带宽字节数。
>  错误映射：`QUOTA.RATE_LIMITED`、`QUOTA.BUDGET_EXCEEDED`。

### 3.5 同意验证（ConsentVerifier）

```rust
#[async_trait::async_trait]
pub trait ConsentVerifier: Send + Sync {
  async fn verify(&self, consent: &Consent, req: &AuthzRequest) -> Result<bool, AuthError>;
}
// 验证expires_at、Scope包含关系、来源与签名（必要时回 Soul-Auth 二次确认）
```

### 3.6 决策缓存与撤销（DecisionCache）

```rust
#[async_trait::async_trait]
pub trait DecisionCache: Send + Sync {
  async fn get(&self, key: &DecisionKey) -> Option<Decision>;
  async fn put(&self, key: DecisionKey, d: &Decision);
  async fn revoke(&self, subject: &Subject); // 撤销（角色/权限变更）
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DecisionKey {
  pub tenant: TenantId,
  pub subject_id: Id,
  pub resource: ResourceUrn,
  pub action: Action,
  pub attrs_fingerprint: u64, // 输入摘要（稳定哈希）
}
```

------

## 4. 标准决策流程（合成接口）

> 提供一个**门面**（Facade）把 AuthN → Attr → PDP → Consent → Quota → 缓存/审计 串起来，供 `soulbase-interceptors` 与内核调用。

```rust
pub struct AuthFacade {
  pub authenticator: Box<dyn Authenticator>,
  pub attr_provider: Box<dyn AttributeProvider>,
  pub authorizer: Box<dyn Authorizer>,
  pub consent: Box<dyn ConsentVerifier>,
  pub quota: Box<dyn QuotaStore>,
  pub cache: Box<dyn DecisionCache>,
}

impl AuthFacade {
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
      let mut req = AuthzRequest { subject: subject.clone(), resource, action, attrs, consent, correlation_id };
      let merged_attrs = self.attr_provider.augment(&req).await.unwrap_or(serde_json::json!({}));

      // 3) Cache
      let dkey = decision_key(&req, &merged_attrs);
      if let Some(hit) = self.cache.get(&dkey).await { return Ok(hit); }

      // 4) PDP 决策
      let mut decision = self.authorizer.decide(&req, &merged_attrs).await?;

      // 5) 高风险同意校验（allow前置）
      if decision.allow {
          if let Some(c) = &req.consent {
              if !self.consent.verify(c, &req).await? { 
                  return Err(AuthError::policy_deny("consent invalid")); 
              }
          }
      }

      // 6) 配额检查（仅 allow 路径）
      if decision.allow {
         match self.quota.check_and_consume(&QuotaKey {
            tenant: req.subject.tenant.clone(),
            subject_id: req.subject.subject_id.clone(),
            resource: req.resource.clone(),
            action: req.action.clone(),
         }, cost_from_attrs(&merged_attrs)).await? {
            QuotaOutcome::Allowed => { /* pass */ }
            QuotaOutcome::RateLimited => return Err(AuthError::rate_limited()),
            QuotaOutcome::BudgetExceeded => return Err(AuthError::budget_exceeded()),
         }
      }

      // 7) 缓存与返回
      if decision.cache_ttl_ms > 0 { self.cache.put(dkey, &decision).await; }
      Ok(decision)
  }
}
```

**行为不变式**

- `deny-by-default`：Authorizer 默认拒绝，需显式放行；
- 配额检查与授权合并在**一次调用**中完成；
- 缓存遵循 `cache_ttl_ms`，撤销时**主动失效**。

------

## 5. 与 Soul-Auth / Soul-Hub 协同

- **Soul-Auth**：
  - OIDC Issuer（Discovery/JWKS）；Subject/Consent 的权威来源；
  - 建议暴露 **/introspect** 或 **userinfo** 以支持边缘节点短证书刷新。
- **Soul-Hub**（入口 PEP）：
  - 在网关做 **Bearer 验签/限流**，并透传 `X-Request-Id / X-Soul-Tenant / TraceId`；
  - 服务内使用 `AuthFacade.authorize` 做细粒度决策与配额扣减；
  - 错误向外映射使用 `soulbase-errors`，公共视图最小披露。

------

## 6. 错误映射（`errors.rs`）

- 认证失败 → `AUTH.UNAUTHENTICATED` / `AUTH.TOKEN_EXPIRED` / `AUTH.CLAIM_INVALID`
- 禁止访问 → `AUTH.FORBIDDEN`
- 策略拒绝 → `POLICY.DENY_TOOL` / `POLICY.DENY_MODEL`（或 `POLICY.DENY_*`）
- 配额限制 → `QUOTA.RATE_LIMITED` / `QUOTA.BUDGET_EXCEEDED`
- PDP 不可用 → `PROVIDER.UNAVAILABLE`（`retryable=Transient`）
- 网络/序列化 → `NETWORK` / `Serialization`
- 其余兜底 → `UNKNOWN.INTERNAL`

> 对外仅 `code + message_user (+ correlation_id)`；审计视图包含 evidence/cause。

------

## 7. 观测与事件（`observe.rs` / `events.rs`）

**主要指标**

- `authn_latency_ms`、`pdp_latency_ms`、`quota_latency_ms`、`decision_cache_hit_ratio`
- `allow_rate` / `deny_rate` / `rate_limited` / `budget_exceeded`
- 标签：`tenant`, `resource`, `action`, `pdp`, `authn`, `retryable`

**事件（Envelope）**

- `AuthDecisionEvent{ subject_id, tenant, resource, action, allow, policy_hash, obligations[] }`
- `AuthDenyEvent{ code, reason, policy_hash }`
- `QuotaEvent{ key, cost, outcome }`

------

## 8. 安全与缓存撤销

- **JWK 缓存**：成功获取新钥失败时回退旧钥；定时刷新（默认 5m）。
- **决策缓存**：短 TTL（常规 1–60s），撤销通道（消息广播/轮询）；对**高风险资源**禁止缓存或 TTL=0。
- **Consent**：仅在 `allow` 时验证且记录证据（签名指纹/来源）；高风险必须具备 `consent`。
- **多租户隔离**：`DecisionKey/QuotaKey` 必含 `tenant`；跨租户访问需显式策略与审计。

------

## 9. 性能与容量（目标）

- **AuthN OIDC 本地验签**：p95 ≤ 2ms；
- **PDP OPA/Cedar**：p95 ≤ 25ms；冷启动时并发保护（连接池/熔断/退避）；
- **DecisionCache 命中**≥ 85%；
- **QuotaStore** 原子更新（Redis `INCRBY`/Lua 或数据库事务）。

------

## 10. 测试与验收

- **单测**：映射正确性、缓存与撤销、Consent 范围匹配、Quota 原子性。
- **契约测试（contract-testkit）**：稳定错误码与公共/审计视图、OPA/Cedar 输入/输出契约、跨版本兼容。
- **集成**：与 `soulbase-interceptors` 跑通“入口→Facade→事件/指标”闭环。
- **拨测**：模拟策略变更、撤销、PDP 故障、配额耗尽，确保降级与回退策略生效。

------

## 11. 版本化与迁移

- SPI 采用 **SemVer**；新增字段只增不破；变更策略需 ADR 与 **MAJOR**。
- 配额度量单位新增通过**枚举扩展**与**meta 标签**保持兼容。
- 决策 `evidence` 采用**松散 JSON**，允许 PDP 注入额外证据字段。

------

## 12. 开放问题

- **统一属性字典**：是否在平台层收敛常用 `attrs` 键（如 `risk.level`, `resource.owner`）。
- **人审义务（obligation）落点**：是否提供标准“二次确认”回调协议。
- **跨边缘一致性**：撤销/配额的多 Region 同步策略。
- **细粒度预算**：与 `soulbase-qos` 的度量口径（GPU秒/IO带宽）标准化。

------

> 若本 TD 符合预期，我将按“三件套”流程，下一步输出 **SB-04-RIS（最小可运行骨架）**，把上述 SPI/类型与最小适配器（`OidcAuthenticator` stub、`LocalAuthorizer`、`MemoryQuota`、`MemoryDecisionCache`）落为可编译代码骨架。
