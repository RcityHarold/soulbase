# 文档 SB-08-TD：`soulbase-tools` 技术设计（Technical Design）

> 对应功能规约：SB-08（Tool SDK / Manifest / Registry / Invocation Protocol）
>  目标：给出 **Tool Manifest / Registry / Invoker** 的数据与接口、**状态机**、**预检与执行编排**，以及与 **Auth / Sandbox / Observe / Errors / Config / QoS / Interceptors / LLM** 的耦合点。
>  语言：Rust（接口草案；不含 RIS 代码骨架）。与既有模块保持四大不变式：**Schema-first、最小权限、默认拒绝、证据闭环**。

------

## 1. Crate 结构与 Feature 规划

```
soulbase-tools/
  src/
    lib.rs
    manifest.rs       # ToolManifest/ConsentPolicy/Limits/Capabilities/Compatibility
    registry.rs       # 注册/启停/发现/可用视图(AvailableSpec)
    preflight.rs      # 预检：Schema校验 + AuthZ/Quota + Profile合成 + Guard校验
    invoker.rs        # 调用编排：Idempotency/Obligations/执行/结果校验/证据闭环
    mapping.rs        # Manifest → Sandbox ExecOp 映射（规范化参数→能力域操作）
    dsl.rs            # （可选）清单/参数 DSL 扩展 & 代码生成钩子
    errors.rs         # 与 soulbase-errors 的稳定映射（TOOL.* / POLICY.* / SANDBOX.*）
    events.rs         # Envelope<ToolRegistered/Updated/InvokedBegin/InvokedEnd/...>
    observe.rs        # 指标标签/计时器/默认采样点
    prelude.rs
```

**Features**

- `schema_json`（默认）：基于 `schemars` 的 JSON-Schema 校验
- `tenant_scoped_registry`：租户级实例化与差异化策略
- `idempotency_store_redis`：幂等结果存储（可选）
- `struct_out_repair`：输出轻修复策略（默认开启，遵循策略）

------

## 2. 数据模型（`manifest.rs`）

### 2.1 基本类型

```rust
pub struct ToolId(pub String);                 // "<group>.<pkg>.<name>"
pub enum SafetyClass { Low, Medium, High }
pub enum SideEffect { None, Read, Write, Network, Filesystem, Browser, Process }

pub struct ConsentPolicy {
  pub required: bool,                          // Safety=High 或 SideEffect∈{Write,Process} → true
  pub max_ttl_ms: Option<u64>,
  pub scope_hint: Vec<sb_types::Scope>,  // 期望同意范围（结构提示，非授权）
}

pub struct Limits {
  pub timeout_ms: u64,
  pub max_bytes_in: u64,
  pub max_bytes_out: u64,
  pub max_files: u64,
  pub max_depth: u32,
  pub max_concurrency: u32,
}

pub struct CapabilityDecl {                    // 与 sandbox::Capability 对齐
  pub domain: String,                          // "fs" | "net.http" | "browser" | "proc" | "tmp"
  pub action: String,                          // "read"|"write"|"get"|"post"|...
  pub resource: String,                        // 路径/域名/工具名 模板或前缀
  pub attrs: serde_json::Value,                // 细粒度参数（阈值/方法）
}

pub struct CompatMatrix {
  pub llm_models_allow: Vec<String>,           // 可见/推荐模型清单（用于 LLM 提案）
  pub platform_min: String,                    // 平台最小版本
  pub notes: Option<String>,
}
```

### 2.2 Tool Manifest（Schema-first）

```rust
pub struct ToolManifest {
  pub id: ToolId,
  pub version: semver::Version,                // SemVer
  pub display_name: String,
  pub description: String,
  pub tags: Vec<String>,

  pub input_schema: schemars::schema::RootSchema,
  pub output_schema: schemars::schema::RootSchema,

  pub scopes: Vec<sb_types::Scope>,      // 需要的 AuthZ 范围（最小权限）
  pub capabilities: Vec<CapabilityDecl>,       // Sandbox 能力映射（最小集）
  pub side_effect: SideEffect,
  pub safety_class: SafetyClass,
  pub consent: ConsentPolicy,

  pub limits: Limits,
  pub idempotency: IdempoKind,                 // "Keyed" | "None"
  pub concurrency: ConcurrencyKind,            // "Serial" | "Parallel"

  pub compat: CompatMatrix,
  pub deprecated: bool,
}

pub enum IdempoKind { Keyed, None }
pub enum ConcurrencyKind { Serial, Parallel }
```

**约束**

- `scopes` 与 `capabilities` 必须一致同向（写→必须含写能力与写 scope）；
- `safety_class` 与 `side_effect` 一致（有 Write/Process → ≥ Medium/High）；
- `input/output_schema` 必须可序列化且有 `$schema/$id`（CI 中强制）。

------

## 3. 注册与发现（`registry.rs`）

### 3.1 状态 & 可用视图

```rust
pub enum ToolState { Registered, Enabled, Paused, Deprecated }

pub struct RegistryRecord {
  pub manifest: ToolManifest,
  pub state: ToolState,
  pub created_at: i64,                         // ms epoch
  pub updated_at: i64,
}

pub struct AvailableSpec {                      // 租户/策略/预算 合成视图
  pub manifest: ToolManifest,
  pub policy_hash: String,                     // 配置/白名单/映射版本
  pub enabled: bool,
  pub visible_to_llm: bool,                    // LLM 可见（仅 name + 输入摘要）
  pub safety_class: SafetyClass,               // 与策略合并后的上限
  pub side_effect: SideEffect,
}
```

### 3.2 接口（简化）

```rust
#[async_trait::async_trait]
pub trait ToolRegistry {
  async fn register(&self, manifest: ToolManifest) -> Result<(), ToolError>;
  async fn update(&self, manifest: ToolManifest) -> Result<(), ToolError>;
  async fn set_state(&self, id: &ToolId, state: ToolState) -> Result<(), ToolError>;

  async fn get(&self, id: &ToolId, tenant: &sb_types::TenantId) -> Option<AvailableSpec>;
  async fn list(&self, tenant: &sb_types::TenantId, filter: ListFilter) -> Vec<AvailableSpec>;
}

pub struct ListFilter {
  pub tags: Vec<String>,
  pub safety_le: Option<SafetyClass>,
  pub side_effect_in: Vec<SideEffect>,
  pub text: Option<String>,
}
```

**注册校验**

- Manifest 的 JSON-Schema 合法；
- `scopes` ↔ `capabilities` 语义一致；
- `safety` 与 `side_effect` 合规；
- 版本 SemVer 流转合法（`MAJOR` 兼容性变更需通过 ADR）。

------

## 4. 预检（`preflight.rs`）

### 4.1 输入与结果

```rust
pub struct ToolCall {
  pub tool_id: ToolId,
  pub call_id: sb_types::Id,             // 对齐 LLM 提案/外部生成
  pub actor: sb_types::Subject,
  pub tenant: sb_types::TenantId,
  pub origin: ToolOrigin,                      // Llm|Api|System
  pub args: serde_json::Value,                 // 按 input_schema 校验
  pub consent: Option<sb_types::Consent>,
  pub idempotency_key: Option<String>,
}

pub enum ToolOrigin { Llm, Api, System }

pub struct PreflightOutput {
  pub allow: bool,
  pub reason: Option<String>,
  pub profile_hash: Option<String>,            // Sandbox Profile 合成结果标识
  pub obligations: Vec<soulbase_auth::prelude::Obligation>,
  pub budget_snapshot: serde_json::Value,      // 预算快照（可选）
}
```

### 4.2 流程（合成顺序）

1. **发现**：查 `AvailableSpec`（租户/策略合成视图必须 `enabled`）。
2. **输入校验**：用 `manifest.input_schema` 校验 `args`；失败 → `SCHEMA.VALIDATION_FAILED`。
3. **授权/配额（Auth/Z + Quota）**：
   - 用 `soulbase-auth::AuthFacade.authorize(...)` 对 `resource="soul:tool:{id}" action="invoke"` 做决策；
   - 高风险路径（Safety=High or SideEffect=Write/Process）要求 `Consent`；
   - 失败 → 返回拒绝（`AUTH.FORBIDDEN`/`POLICY.DENY_TOOL`/`QUOTA.*`）。
4. **Profile 合成**：调用 `soulbase-sandbox::ProfileBuilder`：`Grant ∩ Manifest ∩ PolicyConfig`→`Profile`；
5. **Guard 校验**：`soulbase-sandbox::PolicyGuard.validate(profile, planned_ops)`（如 URL/路径/方法）；
6. **输出**：`allow=true` 时返回 `profile_hash/obligations/budget_snapshot`；否则返回拒绝原因。

**说明**：第 5 步需要**规划执行计划**（planned_ops），见 §5.2「Manifest → ExecOp 映射」。

------

## 5. 调用编排（`invoker.rs`）

### 5.1 状态机

```
Idle
 └─(preflight)─> Ready
Ready
 ├─(invoke)────────────> Running
 │                       ├─(ok)──────> Completed
 │                       ├─(deny)────> Denied
 │                       └─(error)───> Failed
 └─(cancel)────────────> Canceled
```

### 5.2 Manifest → Sandbox ExecOp 映射（`mapping.rs`）

**映射表（示例）**

| Manifest.capability（domain/action） | 参数来源（args 内字段）              | 生成的 `ExecOp`                                            |
| ------------------------------------ | ------------------------------------ | ---------------------------------------------------------- |
| `net.http:get` host/path             | `args.url / args.headers`            | `ExecOp::NetHttp{ method:"GET", url, headers, body:None }` |
| `fs.read` path                       | `args.path, args.offset?, args.len?` | `ExecOp::FsRead{ path, offset, len }`                      |
| `fs.write` path                      | `args.path, args.content_b64`        | `ExecOp::FsWrite{ path, bytes_b64, overwrite:false }`      |
| `tmp.use`                            | `args.size_bytes?`                   | `ExecOp::TmpAlloc{ size_bytes }`                           |

> **要求**：映射逻辑**只做规范化与参数防注入**（如路径归一化、URL 标准化），**不**放宽 Manifest 限制。

### 5.3 Invoker 接口

```rust
pub struct InvokeRequest {
  pub spec: AvailableSpec,
  pub call: ToolCall,
  pub profile_hash: String,
  pub obligations: Vec<soulbase_auth::prelude::Obligation>,
}

pub struct InvokeResult {
  pub status: InvokeStatus,                   // Ok | Denied | Error
  pub error_code: Option<&'static str>,
  pub output: Option<serde_json::Value>,      // 通过 output_schema 校验后的结构
  pub evidence_ref: Option<sb_types::Id>,
}

pub enum InvokeStatus { Ok, Denied, Error }

#[async_trait::async_trait]
pub trait Invoker {
  async fn preflight(&self, call: &ToolCall) -> Result<PreflightOutput, ToolError>;
  async fn invoke(&self, req: InvokeRequest) -> Result<InvokeResult, ToolError>;
}
```

### 5.4 调用步骤（细节）

- **A. 幂等与并发**
  - `Idempotency-Key` 存在时：命中幂等存储 → 直接返回缓存；
  - `concurrency=Serial`：对同一 `tool_id + tenant` 建互斥锁；
- **B. Begin 证据**
  - 构造 `Envelope<ToolInvokeBegin>`（含 `call_id/tenant/subject/tool_id/profile_hash/args_digest`），经 Observe 管道写出；
- **C. 执行**
  1. 依据映射生成 `ExecOp[*]`；
  2. 对每个 op 调用 `sandbox.run(profile, envelope_id, evidence_sink, budget_meter, op)`；
  3. 若任一 op 返回错误 → 终止，进入错误路径；
- **D. Output 处理**
  - 聚合结果为一个结构化 `output`；运行 `output_schema` 校验；
  - 若 `struct_out_repair` 开启：先轻修复后校验；仍失败 → `SCHEMA.VALIDATION_FAILED`；
  - 应用 `obligations`（mask/redact/watermark）；
- **E. End 证据**
  - 生成 `Envelope<ToolInvokeEnd>`（`status/error_code/budget_used/side_effects_digest`）；
- **F. 幂等写入**
  - `Idempotency-Key` 存在时写入结果缓存（TTL & 大小上限）；
- **G. 返回结果**

------

## 6. 与外部模块耦合点

- **Auth**（soulbase-auth）：
  - 在 **Preflight** 统一调用 `AuthFacade.authorize(.. resource="soul:tool:{id}" action="invoke" ..)`；
  - 高风险路径强制 `Consent`；配额扣减与决策缓存遵从 Auth 的返回；
- **Sandbox**（soulbase-sandbox）：
  - ProfileBuilder / PolicyGuard / Executors / EvidenceSink 的使用遵循 SB-06；
  - Manifest → CapabilityDecl → ExecOp 的映射是**唯一**外部行动通道；
- **Interceptors**（soulbase-interceptors）：
  - 入站：Envelope/Trace/Tenant 注入；出站：公共错误视图与标准响应头；
- **Observe**（soulbase-observe）：
  - 指标：`tool_invocations_total`/`tool_denied_total{code}`/`tool_latency_ms`/`tool_budget_bytes` 等；
  - 事件：`ToolRegistered/Updated/InvokedBegin/InvokedEnd`；
- **QoS**（soulbase-qos）：
  - 预算单位（calls/bytes/cpu_ms 等）与 Sandbox/LLM 一致；
- **Config**（soulbase-config）：
  - 策略/白名单/映射与热更；更新后 `AvailableSpec` 即刻刷新；
- **LLM**（soulbase-llm）：
  - 仅消费受限 `ToolSpec`（**name + input_schema 摘要**）；决策与执行在 tools/sandbox/auth 侧完成。

------

## 7. 错误映射（`errors.rs`）

| 阶段      | 触发                   | 稳定错误码                                     |
| --------- | ---------------------- | ---------------------------------------------- |
| 注册/更新 | Manifest 校验失败      | `SCHEMA.VALIDATION_FAILED`                     |
| 预检      | 未注册/禁用/租户不可用 | `POLICY.DENY_TOOL`                             |
| 预检      | 输入不匹配 Schema      | `SCHEMA.VALIDATION_FAILED`                     |
| 预检      | 未授权/无同意/配额不足 | `AUTH.FORBIDDEN` / `QUOTA.*`                   |
| Guard     | 白名单/路径/方法拦截   | `SANDBOX.CAPABILITY_BLOCKED`                   |
| 执行      | 上游不可用/超时        | `PROVIDER.UNAVAILABLE` / `LLM.TIMEOUT`（依域） |
| 输出      | 输出 Schema 不合规     | `SCHEMA.VALIDATION_FAILED`                     |
| 通用      | 未分类                 | `UNKNOWN.INTERNAL`                             |

------

## 8. 事件（`events.rs`）

- `ToolRegistered/Updated/StateChanged`：`id/version/tenant/policy_hash/state`
- `ToolInvokeBegin`：`envelope_id/tenant/subject/tool_id/call_id/profile_hash/args_digest`
- `ToolInvokeEnd`：`envelope_id/status/error_code?/budget_used/side_effects_digest/output_digest`

> **最小披露**：仅记录摘要（hash/size），不落原始 `args/output` 内容。

------

## 9. 指标（`observe.rs`）

- `tool_invocations_total{tool_id,tenant,origin}`
- `tool_denied_total{tool_id,code}`
- `tool_latency_ms{tool_id}`（端到端 & 各阶段：preflight/exec/validate）
- `tool_budget_bytes{dir=in|out}`/`tool_budget_calls`
- `tool_errors_total{code}`

**标签最小集**：`tenant`, `tool_id`, `code`, `origin`, `safety`, `side_effect`.

------

## 10. 安全与治理

- **三方交集**：执行能力 = `Grant ∩ Manifest ∩ PolicyConfig`；
- **高风险同意**：`Safety=High` 或 `SideEffect∈{Write,Process}` 时 `Consent.required=true`；
- **幂等**：幂等工具必须实现 `Idempotency-Key` 路径与结果缓存；
- **脱敏**：日志/事件只存摘要；对 `output` 执行 `obligations`；
- **租户隔离**：`tenant` 贯穿查找、授权、Profile 合成、证据与指标标签；
- **版本治理**：SemVer；`MAJOR` 变更需兼容层或迁移指南；`MINOR/PATCH` 不破坏已有调用者。

------

## 11. 测试与验收（契约/黑盒）

- **契约测试**（soulbase-contract-testkit）
  - Manifest 校验矩阵（Schema/Scopes/Capabilities/Consent/SideEffect/Safety）；
  - 预检→执行→证据链完整性；
  - 错误码与公共视图一致性；
- **黑盒**
  - 未授权/无同意/白名单拦截/预算超限/输出结构失败；
  - 幂等命中/并发串行化；
- **回放**
  - 基于 Evidence 恢复调用摘要并核对指标；

------

## 12. 版本化与迁移

- `ToolManifest` 新增字段仅以可选形式出现；
- `Capabilities/SideEffect/Safety` 枚举新增分支**不**破坏旧消费者；
- `AvailableSpec` 结构新增仅向后兼容；
- 重大变更发布 ADR，并提供自动化**清单迁移器**（从旧结构生成新结构与兼容 shim）。

------

## 13. 开放问题

- **异步/长任务**：标准化 `start → poll/status → complete` 协议与 Evidence 分段；
- **组合工具**：Tool-of-Tools 的编排与 Schema 自动对接（输出→下一个输入）；
- **多模工具与隐私**：图像/音频处理的最小摘要与安全红线；
- **跨域（A2A）工具**：凭证与证据链的签名与最小披露策略（对接 `soulbase-a2a`）；
- **工具市场/分发**：签名清单（caplet）与来源信任链，是否纳入本模块或独立项目。

------

> 若上述 TD 确认无误，我将按“三件套”继续输出 **SB-08-RIS（最小可运行骨架）**：包含 `ToolManifest/Registry/Invoker` 的最小实现、Preflight/Invoke 编排骨架、与 `soulbase-auth/sandbox/observe/errors` 的直连接口与单测（注册→预检→执行→证据）。
