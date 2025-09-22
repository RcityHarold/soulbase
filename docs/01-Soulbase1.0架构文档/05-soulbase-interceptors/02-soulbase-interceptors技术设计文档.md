# 文档 SB-05-TD：`soulbase-interceptors` 技术设计（Technical Design）

> 对应功能规约：SB-05（统一拦截器链 / 中间件）
>  目标：给出 **crate 结构、阶段模型（Stages）、协议适配（HTTP/gRPC/MQ）、路由策略到资源/动作映射、Envelope 绑定、Schema 校验、AuthN/AuthZ/Quota 协同、Resilience（超时/熔断/重试）、Idempotency、Obligations 执行、错误规范化、观测与指标** 的可落地设计。
>  语言：Rust；不绑定具体框架，提供 **Trait + 适配器**；Tower/Axum/Tonic 等通过 feature 可选启用。
>  与原文精华同频：**SSoT（Envelope）/ 双层 PEP / 最小披露 / Deny-by-Default / 可回放审计**。

------

## 1. Crate 结构与模块

```
soulbase-interceptors/
  src/
    lib.rs
    context.rs          # Request/Response Context（协议无关，上下文总线）
    stages/             # 阶段接口 & 默认实现
      mod.rs
      context_init.rs   # X-Request-Id / Trace / Envelope seed / Config snapshot 元信息
      route_policy.rs   # 路由→ResourceUrn/Action/Attrs 模板解析
      authn_map.rs      # 认证映射（Subject），租户一致性校验
      authz_quota.rs    # 细粒度授权 + 配额一次性决策
      schema_guard.rs   # 请求/响应 Schema 校验
      resilience.rs     # 超时 / 熔断 / 重试 / 限并发（策略载体）
      obligations.rs    # 义务执行（mask/redact/watermark）
      response_stamp.rs # 标准响应头（X-Config-* / X-Trace-Id / X-Obligations）
      error_norm.rs     # 错误规范化（soulbase-errors）
    adapters/           # 协议适配层
      http.rs           # HTTP（Axum/Tower 可选）
      grpc.rs           # gRPC（Tonic 可选）
      mq.rs             # MQ/事件（Kafka/NATS/AMQP 可选）
    policy/             # 路由策略模型 & 解析器
      model.rs
      dsl.rs            # YAML/JSON 策略 DSL（可来自 soulbase-config）
    schema/             # Schema 注册与校验（JSON Schema / Protobuf 描述）
      json.rs
      pb.rs
    idempotency/        # 幂等存储接口（内存/Redis）
      mod.rs
      memory.rs
    observe.rs          # 指标/日志/Trace 标签导出
    errors.rs           # 本模块错误到 soulbase-errors 映射
    prelude.rs
```

**Features（建议）**

- `with-axum`, `with-tower`, `with-tonic`：协议栈适配
- `schema-json`, `schema-pb`：请求/响应 Schema 校验后端
- `store-redis`：幂等结果存储（可选）
- `resilience-tower`：以 Tower Layer 落实超时/熔断/重试
- `route-dsl`：启用 YAML/JSON 路由策略 DSL 装载（结合 `soulbase-config`）

**上游依赖**：`sb-types`、`soulbase-config`、`soulbase-auth`、`soulbase-errors`、（可选）`soulbase-observe`

------

## 2. 核心抽象与上下文模型

### 2.1 Request/Response Context（协议无关）

```rust
/// 协议无关的上下文快照（流经各 Stage）
/// - 不持有大对象引用；业务体（body）由适配器以 trait 提供读/写
pub struct InterceptContext {
  pub request_id: String,
  pub trace: sb_types::TraceContext,
  pub route: Option<RouteBinding>,            // 由 route_policy 决定
  pub subject: Option<sb_types::Subject>,
  pub tenant_header: Option<String>,          // X-Soul-Tenant
  pub consent_token: Option<String>,          // X-Consent-Token（上层解析后置入）
  pub config_version: Option<String>,         // X-Config-Version
  pub config_checksum: Option<String>,        // X-Config-Checksum
  pub obligations: Vec<Obligation>,          // 决策返回的义务
  pub envelope_seed: EnvelopeSeed,            // Envelope 初始元信息
  pub extensions: http::Extensions,           // 自定义扩展（小心敏感数据）
}

pub struct EnvelopeSeed {
  pub correlation_id: Option<String>,
  pub causation_id: Option<String>,
  pub partition_key: String,  // 建议："{tenant}:{resource_key}"
  pub produced_at_ms: i64,
}
```

### 2.2 协议适配面（抽象）

```rust
#[async_trait::async_trait]
pub trait ProtoRequest {
  fn method(&self) -> &str;            // HTTP: GET/POST；gRPC/MQ 可映射 service/method/topic
  fn path(&self) -> &str;              // HTTP path；gRPC 映射成 "/pkg.svc/Method"
  fn headers(&self) -> &HeadersView;   // 统一读接口
  async fn read_json(&mut self) -> Result<serde_json::Value, Error>;  // 可选
}

#[async_trait::async_trait]
pub trait ProtoResponse {
  fn headers_mut(&mut self) -> &mut HeadersMut;
  async fn write_json(&mut self, body: &serde_json::Value) -> Result<(), Error>;
  fn set_status(&mut self, code: u16);
}
```

> 适配器（Axum/Tonic/MQ）负责把框架请求体映射为 `ProtoRequest/ProtoResponse`。

------

## 3. 阶段模型（Stages）与编排

每个 Stage 实现统一接口；拦截器链通过**顺序编排**与**错误短路**形成处理流程。

```rust
#[async_trait::async_trait]
pub trait Stage: Send + Sync {
  async fn handle(
    &self,
    cx: &mut InterceptContext,
    req: &mut dyn ProtoRequest,
    rsp: &mut dyn ProtoResponse
  ) -> Result<StageOutcome, Error>;
}

pub enum StageOutcome { Continue, ShortCircuit }   // 短路：阶段已回写响应（如拒绝/错误）
```

**默认编排顺序（可配置）**
 `ContextInit → RoutePolicy → AuthNMap → TenantGuard → AuthZQuota → SchemaGuard(req) → Resilience(around handler) → SchemaGuard(resp) → Obligations → ResponseStamp → ErrorNorm`

> `Resilience` 以“环”（around）包裹业务处理函数，前后可结合度量计时。

------

## 4. 路由策略（Route → Resource/Action/Attrs）

### 4.1 模型

```rust
pub struct RoutePolicySpec {
  pub when: MatchCond,              // path/method/topic 模式
  pub bind: RouteBindingSpec,       // ResourceUrn/Action/Attrs 模板
}

pub enum MatchCond {
  Http { method: String, path_glob: String },     // e.g. GET, /v1/memory/items/*
  Grpc { service: String, method: String },       // pkg.svc, Method
  Mq   { topic_glob: String },
}

pub struct RouteBindingSpec {
  pub resource: String,             // "soul:tool:browser"
  pub action: String,               // "Read"|"Write"|"Invoke"|...
  pub attrs_template: serde_json::Value, // 可插值：path/query/header/body 字段
}
```

### 4.2 解析与绑定

- 通过 **模板引擎**（轻量 JSON-Path/JQ 子集）从请求中提取变量，渲染 `attrs_template`。
- 首命中原则；未命中 → **Deny**（稳定码 `POLICY.DENY_TOOL` 或等效）。

------

## 5. Envelope 绑定与标准头

- `ContextInit` 生成/采纳 `X-Request-Id`、`TraceContext`，构造 `EnvelopeSeed`：
  - `partition_key`：默认 `"{tenant}:{first-path-seg or logical-key}"`（可在策略中覆盖）。
  - `produced_at_ms`：当前 UTC 毫秒。
- `ResponseStamp` 写出：`X-Request-Id`，`X-Trace-Id`（或 `traceparent`），`X-Config-Version/Checksum`，`X-Obligations`（仅种类列表）。

------

## 6. AuthN/AuthZ/Quota 协同

### 6.1 AuthNMap（认证映射）

- 从 `Authorization` 或网关透传头取得凭据，调用 `soulbase-auth::Authenticator` → `Subject`。
- 校验 `X-Soul-Tenant` 与 `Subject.tenant` 一致；不一致 → `AUTH.FORBIDDEN`。
- 将 `Subject` 注入 `InterceptContext.subject`。

### 6.2 AuthZQuota（一次性决策）

- 根据 `RouteBinding` 构造 `AuthzRequest`（含 `attrs` 渲染结果与 `consent`）并调用 `soulbase-auth` 门面：
  - PDP 决策（Allow/Deny + obligations + evidence + cache_ttl）
  - 通过时**原子扣额**（QuotaStore）
- 返回 `Decision`：
  - `allow=false` → 规范化错误并**短路**
  - `allow=true` → 将 `obligations` 注入上下文

------

## 7. SchemaGuard（请求/响应 Schema）

- **请求体**：
  - JSON：`schema-json` 开启时按 **JSON Schema** 校验。
  - gRPC：`schema-pb` 开启时读取 Protobuf 描述（`prost-reflect` 等），做字段/枚举范围校验。
- **响应体**（可选）：对出站 JSON/Protobuf 进行模式校验（建议在测试与拨测中启用）。
- 失败映射：`SCHEMA.VALIDATION_FAILED`（HTTP 422 / gRPC INVALID_ARGUMENT）。

------

## 8. Resilience（超时/熔断/重试/限并发）

- 不与网关前置限流冲突；此处关注**服务内**调用与 Handler 保护：
  - **超时**：对 Handler 设置最大处理时间；超时 → `LLM.TIMEOUT` 或 `PROVIDER.UNAVAILABLE`（按资源域映射）。
  - **熔断**：对连续失败/高错误率的路由进入开路状态（返回 `PROVIDER.UNAVAILABLE`，`retryable=Transient`）。
  - **重试**：仅对 **幂等/只读路由** 进行指数退避重试（次数/间隔可配置）。
  - **限并发**：避免击穿下游；过载 → `QUOTA.RATE_LIMITED`。

> 实现建议：提供抽象策略接口；启用 `resilience-tower` 时以 Tower Layer 落地。

------

## 9. Idempotency（可选）

- 读取 `Idempotency-Key`；仅对 **写操作** 生效。
- 接口：`IdemStore::get/put`，键为 `(tenant, subject, resource, action, key)`；值为**公共响应视图**（脱敏）。
- 命中 → 直接返回缓存结果，并写 `X-Idempotent-Replay: true`；
- 新写入 → 放行业务后缓存（TTL & 大小上限）；
- 冲突或键非法 → `SCHEMA.VALIDATION_FAILED` / `POLICY.DENY_*`。

------

## 10. Obligations 执行

- 义务模型复用 `soulbase-auth::Obligation { kind, params }`：
  - `mask`：对响应 JSON 指定路径字段做掩码（`****`）；
  - `redact`：删除指定路径字段；
  - `watermark`：在响应元数据或文本附加水印/来源标记；
- 无法执行（路径不存在/类型不符）：
  - 若 `Obligation-Strict=true` → 拒绝并记录 `POLICY.DENY_*`；
  - 否则记录警告并跳过该义务。
- 执行顺序：`redact` → `mask` → `watermark`。

------

## 11. 错误规范化（Error Normalizer）

- 将任意异常统一映射为 `soulbase-errors::ErrorObj`：
  - 认证失败 → `AUTH.UNAUTHENTICATED/..`
  - 授权拒绝 → `AUTH.FORBIDDEN` or `POLICY.DENY_*`
  - 预算/速率 → `QUOTA.BUDGET_EXCEEDED` / `QUOTA.RATE_LIMITED`
  - Schema → `SCHEMA.VALIDATION_FAILED`
  - 下游不可用/超时 → `PROVIDER.UNAVAILABLE` / `LLM.TIMEOUT`
  - 未分类 → `UNKNOWN.INTERNAL`（目标 ≤ 0.1%）
- **公共视图**对外；**审计视图**写入观测面。
- HTTP/gRPC 映射遵循 `soulbase-errors` 建议状态码，必要时附 `Retry-After`。

------

## 12. 观测与指标（Observe）

- **请求维度**：`requests_total`、`latency_ms{stage=...}`、`active_requests`
- **授权**：`auth_allow_total`、`auth_deny_total{code}`、`decision_cache_hit_ratio`（来自 `soulbase-auth`）
- **配额**：`quota_consumed_total{resource,action}`、`rate_limited_total`
- **Schema**：`schema_failures_total{direction=req|resp}`
- **Resilience**：`cb_open_total{route}`、`timeout_total{route}`、`retries_total{route}`
- **义务**：`obligation_applied_total{kind}`、`obligation_failed_total{kind}`
- **标签最小集**：`tenant`, `resource`, `action`, `code`, `retryable`, `severity`, `route_id`

------

## 13. 与其他基石模块的接口契约

- **sb-types**：
  - 使用 `TraceContext`、`Subject`、`Envelope` 字段口径；
  - `partition_key` 策略与租户一致性强校验。
- **soulbase-config**：
  - 读取快照 `version/ checksum`；热更后新请求**即刻**带新戳记。
- **soulbase-auth**：
  - 通过 `Authenticator/Authorizer/QuotaStore` 门面一次性决策；`consent` 由上游解析注入。
- **soulbase-errors**：
  - 错误公共/审计视图转换；状态码/重试标签一致。
- **soulbase-observe**：
  - 指标/日志标签规范一致；`Envelope` 审计事件输出。

------

## 14. 安全与合规

- **租户强一致**：请求头与 Subject 不一致直接拒绝；
- **最小披露**：日志/事件默认不含敏感字段（可配置屏蔽表）；
- **Consent 传递**：高风险路径要求 `consent_token` 或上游 `Consent` 结构；
- **幂等缓存**：仅存公共视图；带租户/主体/资源维度键；可选加密存储。

------

## 15. 性能目标

- 拦截器链自开销 p95 ≤ 2ms（不含远程 PDP）；
- Schema 校验 p95 ≤ 1ms（中等 JSON）；
- 义务执行 p95 ≤ 1ms（中等 JSON，路径 ≤ 10）。

------

## 16. 测试与验收

- **契约测试**：
  - 路由匹配表 → Resource/Action/Attrs 绑定正确；
  - 错误映射一致性（HTTP/gRPC 状态码与稳定码对齐）。
- **黑盒**：
  - 未声明路由拒绝、租户不一致拒绝、无 consent 拒绝、预算不足拒绝、Schema 失败、义务失败场景。
- **回放**：
  - `Envelope` 序列重建一次请求的授权证据与义务执行顺序。
- **压测**：
  - 在目标 QPS 下满足自开销与整体延迟目标；Resilience 策略不引入明显尾延。

------

## 17. 版本化与迁移

- **Stage SPI** 与 `RoutePolicySpec` 遵循 SemVer；新增阶段以可选启用方式引入；
- **标准头** 不随意变更；新增头先进入“试验性”前缀（如 `X-Soul-Exp-*`）再稳定化；
- **DSL** 变更需提供向后兼容解析与迁移脚本。

------

## 18. 开放问题（Open Issues）

- JSON-Path/JQ 子集选择与性能权衡；
- Protobuf Schema 校验粒度（必填/oneof/范围）与跨语言一致性；
- MQ 入口的“至少一次”与幂等/去重策略的统一口径；
- Obligations 在多媒体/二进制响应中的最小实现（例如添加只读水印元数据）。

------

> 本 TD 给出 `soulbase-interceptors` 的**接口与编排蓝图**。若确认无误，下一步可输出 **SB-05-RIS（最小可运行骨架）**，包含：上下文与 Stage 接口、HTTP 适配器样例（Axum/Tower）、最小路由策略解析、与 `soulbase-auth` 门面的集成、错误规范化与观测标签占位。
