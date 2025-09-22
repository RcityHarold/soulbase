# 文档 SB-06-TD：`soulbase-sandbox` 技术设计（Technical Design）

> 对应功能规约：SB-06（受控执行 / Controlled Execution & Evidence）
>  目标：给出 **crate 结构、核心模型、Traits/SPI、Profile 合成流程、各执行器与 Guards、证据结构、与 Auth/Tools/Observe 的接口**，使“**能力声明 → 授权核验 → 受控执行 → 证据闭环**”端到端可落地。
>  语言：Rust（以 `serde`/`async-trait` 为基础）；本 TD 不包含 RIS 代码骨架。

------

## 1. Crate 结构与 Feature 规划

```
soulbase-sandbox/
  src/
    lib.rs
    model.rs            # Capability/SafetyClass/SideEffect/Grant/Budget/Profile/Exec APIs
    profile.rs          # ProfileBuilder（Grant ∩ Manifest ∩ PolicyConfig 合成）
    guard.rs            # PolicyGuard（Pre-Exec 校验：路径/URL/命令模板/大小/白名单）
    exec/               # 执行器 SPI 与实现
      mod.rs            # Executor, ExecCtx, ExecOp, ExecResult
      fs.rs             # FsExecutor（本地受限文件操作）
      net.rs            # NetExecutor（出站 HTTP）
      browser.rs        # BrowserExecutor（无头浏览器只读能力）
      proc.rs           # ProcExecutor（白名单子进程）
      tmp.rs            # TmpExecutor（隔离临时空间）
    budget.rs           # BudgetMeter（与 soulbase-qos 对接）
    evidence.rs         # Evidence 结构、哈希/摘要、Envelope 事件构造器
    revoke.rs           # Grant 撤销/过期监听（可选）
    config.rs           # PolicyConfig 载入口径（对白名单/限额/映射做 Schema 限定）
    errors.rs           # 与 soulbase-errors 的稳定映射（SANDBOX/POLICY/PROVIDER/NETWORK/…）
    observe.rs          # 指标标签导出
    prelude.rs
```

**Feature flags（建议）**

- `exec-fs`, `exec-net`, `exec-browser`, `exec-proc`, `exec-tmp`（各执行器开关）
- `browser-chromium`（Headless Chromium 守护/Playwright 适配）
- `net-reqwest`（基于 reqwest 的 HTTP 客户端）
- `wasi`（未来接入 WASI/wasmtime 作为统一隔离载体）
- `qos`（启用与 `soulbase-qos` 的预算对接）
- `observe`（证据与指标上报通道）

------

## 2. 核心模型（`model.rs`）

### 2.1 能力域与风险

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Capability {
  FsRead { path: String },
  FsWrite { path: String },
  FsList { path: String },
  NetHttp { host: String, port: Option<u16>, scheme: Option<String>, methods: Vec<String> },
  BrowserUse { scope: String },                 // "open|screenshot|extract"
  ProcExec { tool: String },                    // 白名单可执行名（无 shell）
  TmpUse,
  SysGpu { class: String },                     // 可选
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum SafetyClass { Low, Medium, High }

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum SideEffect { None, Read, Write, Network, Filesystem, Browser, Process }
```

### 2.2 授权票据与预算

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Grant {
  pub tenant: sb_types::TenantId,
  pub subject_id: sb_types::Id,
  pub tool_name: String,
  pub call_id: sb_types::Id,            // 对齐 ToolCallProposal.call_id
  pub capabilities: Vec<Capability>,          // 授权集合
  pub expires_at: i64,                        // ms since epoch
  pub budget: Budget,
  pub decision_key_fingerprint: String,       // 防复用
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Budget {
  pub calls: u64,
  pub bytes_out: u64,
  pub bytes_in: u64,
  pub cpu_ms: u64,
  pub gpu_ms: u64,
  pub file_count: u64,
}
```

### 2.3 执行配置 Profile（合成视图）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Profile {
  pub tenant: sb_types::TenantId,
  pub subject_id: sb_types::Id,
  pub tool_name: String,
  pub call_id: sb_types::Id,
  pub capabilities: Vec<Capability>,          // Grant ∩ Manifest ∩ PolicyConfig 的交集
  pub safety: SafetyClass,                    // 取三者中最高等级
  pub side_effect: SideEffect,
  pub limits: Limits,                         // 大小/数量/时间等限额
  pub whitelists: Whitelists,                 // 域名/路径/工具名白名单
  pub mappings: Mappings,                     // 路径映射/临时目录根
  pub timeout_ms: u64,
  pub profile_hash: String,                   // 参与 Evidence 与回放
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Limits {
  pub max_bytes_in: u64,                      // 单次输入/下载上限
  pub max_bytes_out: u64,                     // 单次输出/上传上限
  pub max_files: u64,
  pub max_depth: u32,                         // 浏览器导航深度/目录深度
  pub max_concurrency: u32,                   // 并发执行器数
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Whitelists {
  pub domains: Vec<String>,
  pub paths: Vec<String>,
  pub tools: Vec<String>,
  pub mime_allow: Vec<String>,
  pub methods: Vec<String>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Mappings {
  pub root_fs: String,                        // 根目录映射（chroot-like）
  pub tmp_dir: String,                        // 隔离 tmp
}
```

### 2.4 执行请求与结果（抽象）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ExecOp {
  FsRead { path: String, offset: Option<u64>, len: Option<u64> },
  FsWrite { path: String, bytes_b64: String, overwrite: bool },
  NetHttp { method: String, url: String, headers: serde_json::Map<String, serde_json::Value>, body_b64: Option<String> },
  BrowserNav { url: String },                 // 只读能力
  BrowserScreenshot { selector: Option<String>, full_page: bool },
  ProcExec { tool: String, args: Vec<String>, timeout_ms: Option<u64> },
  TmpAlloc { size_bytes: u64 },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExecResult {
  pub ok: bool,
  pub code: Option<&'static str>,             // 稳定错误码（soulbase-errors）
  pub message: Option<String>,                // 用户可见消息（脱敏）
  pub out: serde_json::Value,                 // 受控输出（如 bytes 摘要或句柄）
  pub used: Budget,                           // 本次消耗
}
```

------

## 3. Traits / SPI

### 3.1 ProfileBuilder（Grant ∩ Manifest ∩ PolicyConfig）

```rust
#[async_trait::async_trait]
pub trait ProfileBuilder: Send + Sync {
  async fn build(
    &self,
    grant: &Grant,
    manifest: &soulbase_tools::Manifest,      // 工具声明（权限/安全等级/副作用/Schema）
    policy: &PolicyConfig,                    // 来自 soulbase-config 的沙箱策略
  ) -> Result<Profile, SandboxError>;
}
```

> **合成规则**：
>
> - 能力：取三者交集（并对资源/动作/属性取最窄值）；
> - 风险：取三者最高 `SafetyClass`；
> - 副作用：取三者并集，但执行时仍受能力交集约束；
> - 限额/白名单/映射：Grant/Manifest/PolicyConfig 三方有冲突时取**最严格**；
> - `profile_hash` = Hash(上述关键字段 + 策略版本 + Manifest 版本)。

### 3.2 PolicyGuard（Pre-Exec 审核）

```rust
#[async_trait::async_trait]
pub trait PolicyGuard: Send + Sync {
  async fn validate(&self, profile: &Profile, op: &ExecOp) -> Result<(), SandboxError>;
}
```

> **校验项**：路径归一化与前缀绑定、域名与端口白名单、方法/MIME 白名单、重定向链校验、命令与参数模板、大小与并发限制、Consent 必要性。

### 3.3 Executor（各域执行器）

```rust
pub struct ExecCtx<'a> {
  pub profile: &'a Profile,
  pub envelope_seed: &'a sb_types::Envelope<serde_json::Value>, // 只用于追踪种子（或更轻量引用）
  pub budget: &'a dyn BudgetMeter,
  pub evidence: &'a dyn EvidenceSink,
  pub cancel: &'a dyn CancelToken,
}

#[async_trait::async_trait]
pub trait Executor: Send + Sync {
  fn capability_domain(&self) -> &'static str; // "fs" | "net.http" | "browser" | "proc" | "tmp"
  async fn execute(&self, ctx: &ExecCtx<'_>, op: ExecOp) -> Result<ExecResult, SandboxError>;
}
```

**内置执行器（最小实现）**

- `FsExecutor`：受限根目录，拒绝符号链接/路径穿越；写入仅限 `mappings.tmp_dir` 或 `root_fs` 子树；
- `NetExecutor`：解析 URL，校验域名与方法；下载/上传大小门限，响应体默认返回**摘要**（hash + 前 N bytes）；
- `BrowserExecutor`：单页只读会话（open/extract/screenshot），禁脚本注入与下载；限制导航深度与分辨率；
- `ProcExecutor`：可执行名与参数白名单，禁继承环境；CPU/内存/时长限额，stdout/stderr 截断成**摘要**；
- `TmpExecutor`：分配/释放隔离 tmp 空间。

### 3.4 BudgetMeter（与 QoS 协同）

```rust
#[async_trait::async_trait]
pub trait BudgetMeter: Send + Sync {
  async fn check_and_consume(&self, kind: &str, amount: u64) -> Result<(), SandboxError>; // "bytes_out"/"bytes_in"/"calls"/"cpu_ms"/"gpu_ms"/"file_count"
}
```

> **建议**：启用 `qos` feature 时用 `soulbase-qos` 的计量器；RIS 阶段提供内存占位实现。

### 3.5 EvidenceSink（证据事件）

```rust
#[async_trait::async_trait]
pub trait EvidenceSink: Send + Sync {
  async fn begin(&self, ev: &EvidenceBegin);
  async fn end(&self, ev: &EvidenceEnd);
}
```

> 默认实现：转为 `Envelope<EvidenceEvent>` 投递至 `soulbase-observe`/审计通道；失败不阻塞主执行。

### 3.6 RevocationWatcher（可选）

```rust
#[async_trait::async_trait]
pub trait RevocationWatcher: Send + Sync {
  async fn is_revoked(&self, grant: &Grant) -> bool;
}
```

------

## 4. Profile 合成流程（Sequence）

1. **输入**：`Grant`（来自 `soulbase-auth`）、`Manifest`（`soulbase-tools`）、`PolicyConfig`（`soulbase-config`）。
2. **交集计算**：能力集合求交，并将资源/动作/属性取最窄。
3. **风险合并**：选最大 `SafetyClass`；副作用取并集（执行仍受能力交集约束）。
4. **限额与白名单**：对齐最严格限额、路径与域名白名单；映射与 tmp 根确定。
5. **预算视图**：创建预算视图（剩余额度 = Grant.budget ⊓ QoS 全局/租户预算）。
6. **输出**：`Profile{…, profile_hash}`；同时写入 `policy_version_hash` 以便 Evidence 追溯。

------

## 5. 执行主流程（Orchestrator）

**伪时序：**

```
Orchestrator::run(profile, op, envelope_seed):
  1) revoke.check(grant) → 若撤销 → SANDBOX.PERMISSION_DENY
  2) guard.validate(profile, op) → 若失败 → POLICY.DENY_TOOL / SANDBOX.CAPABILITY_BLOCKED
  3) evidence.begin(...)
  4) budget.check_and_consume("calls", 1) → 超限 → QUOTA.BUDGET_EXCEEDED
  5) exec = pick_executor(op)
  6) result = exec.execute(ctx, op)  // 内部对 bytes_in/out/cpu_ms 实时计量 & 扣减
  7) evidence.end(..., result.used, status)
  8) 返回 result（公共视图：摘要/句柄；不直接外泄敏感内容）
```

**取消/超时**：

- `CancelToken` 由上游/内核传入；执行器需周期检测 token；超时触发 `Timeout` → `LLM.TIMEOUT` 或 `PROVIDER.UNAVAILABLE`（按域映射）。

------

## 6. Guard 规则（关键约束）

- **FS**：绝对路径归一化 → 必须以 `root_fs` 或 `tmp_dir` 为前缀；拒绝 `..`/符号链接；写入大小 ≤ `limits.max_bytes_out`；文件数 ≤ `limits.max_files`。
- **NET**：
  - URL 解析 → host 必在 `whitelists.domains`；方法在 `whitelists.methods`；
  - **重定向链**每步都校验白名单；响应 `Content-Type` ∈ `whitelists.mime_allow`；体积 ≤ `limits.max_bytes_in`。
- **BROWSER**：
  - 禁止注入脚本与下载；导航深度 ≤ `limits.max_depth`；截图分辨率与频率限制；指纹/隐私默认关闭。
- **PROC**：
  - `tool` ∈ `whitelists.tools`；参数模板校验（正则/枚举）；禁环境继承；资源限额（CPU/内存/时长）；输出截断为**摘要**。
- **Consent**：`SafetyClass=High` 或 `side_effect=Write/Process` → 需要有效 `Consent`（由 `soulbase-auth` 在 Grant 侧校验；Sandbox 再次检查存在性与时效）。

------

## 7. Evidence 结构（`evidence.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EvidenceBegin {
  pub envelope_id: sb_types::Id,
  pub tenant: sb_types::TenantId,
  pub subject_id: sb_types::Id,
  pub tool_name: String,
  pub call_id: sb_types::Id,
  pub profile_hash: String,
  pub capability: String,                 // e.g. "net.http:GET:https://example.com"
  pub inputs_digest: Digest,              // sha256/base64 + size
  pub produced_at_ms: i64,
  pub policy_version_hash: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EvidenceEnd {
  pub envelope_id: sb_types::Id,
  pub status: String,                     // "ok" | "denied" | "error"
  pub error_code: Option<&'static str>,   // 稳定错误码
  pub outputs_digest: Digest,
  pub side_effects: Vec<SideEffectRec>,   // 文件/网络/进程摘要
  pub budget_used: Budget,
  pub finished_at_ms: i64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Digest { pub algo: &'static str, pub b64: String, pub size: u64 }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SideEffectRec {
  pub kind: SideEffect,
  pub meta: serde_json::Value,            // 如 { "path": "...", "bytes": 123 } or { "url": "...", "bytes_in": ... }
}
```

> **最小披露**：`inputs/outputs_digest` 与 `side_effects.meta` 仅记录摘要，不含原文内容。

------

## 8. 与外部模块的接口

- **soulbase-tools**：
  - 使用 `Manifest.permissions/side_effect/safety_class/input_schema/output_schema`；
  - Sandbox **拒绝** Manifest 与 Grant 不一致的操作。
- **soulbase-auth**：
  - ProfileBuilder 接收 `Grant`；执行时再做**撤销/过期**检查；预算通过 `BudgetMeter` 与 `Quota` 对齐；
- **soulbase-interceptors**：
  - 从拦截器注入的 `Envelope` 种子（RequestId/Trace/Tenant/Subject）用于 Evidence；
  - 错误通过 `soulbase-errors` 规范化，返回公共视图；必要时由拦截器写标准响应头。
- **soulbase-config**：
  - `PolicyConfig`（白名单/黑名单/映射/阈值）从配置快照读取；HotReload 时新请求立即生效。
- **soulbase-observe**：
  - `EvidenceBegin/End` 转为 `Envelope<EvidenceEvent>` 输出；
  - 指标标签（tenant/resource/tool/safety/side_effect/code/retryable）。

------

## 9. 错误映射（`errors.rs`）

| 场景                                  | 稳定错误码（建议）                     | 说明                         |
| ------------------------------------- | -------------------------------------- | ---------------------------- |
| 未授权能力/Grant 缺失/过期/撤销       | `SANDBOX.PERMISSION_DENY`              | 401/403 语义（对外公共视图） |
| Manifest 与操作不一致                 | `POLICY.DENY_TOOL`                     | 403                          |
| 触发沙箱策略（黑名单/大小/路径/方法） | `SANDBOX.CAPABILITY_BLOCKED`           | 403                          |
| 出站网络不可用/超时                   | `PROVIDER.UNAVAILABLE` / `LLM.TIMEOUT` | 503/504                      |
| 路径穿越/符号链接/越界写入            | `SANDBOX.CAPABILITY_BLOCKED`           | 403                          |
| 命令模板/参数非法                     | `POLICY.DENY_TOOL`                     | 403                          |
| 预算/配额超限                         | `QUOTA.BUDGET_EXCEEDED`                | 429                          |
| JSON/输入结构不合法                   | `SCHEMA.VALIDATION_FAILED`             | 422                          |
| 执行器内部异常                        | `TOOL.EXECUTION_FAILED`                | 500                          |
| 未分类                                | `UNKNOWN.INTERNAL`                     | 500（目标 ≤ 0.1%）           |

------

## 10. 观测与指标（`observe.rs`）

**核心指标**

- `sandbox_exec_total{domain,tool,safety,side_effect,tenant}`
- `sandbox_denied_total{code}`、`sandbox_errors_total{code}`
- `sandbox_budget_used_bytes{in|out}`、`sandbox_cpu_ms`、`sandbox_gpu_ms`
- `sandbox_latency_ms{domain}`、`sandbox_profile_build_ms`、`sandbox_guard_validate_ms`

**最小标签**：`tenant`, `tool`, `domain(fs|net|browser|proc|tmp)`, `safety`, `side_effect`, `code`, `retryable`

------

## 11. 安全要点（实现强约束）

- 统一**路径归一化**、**根目录绑定**与**禁止符号链接**；
- 网络**重定向链校验**、DNS 直连白名单、禁私网地址（可选）；
- 浏览器**无状态/洁净会话**、禁扩展/下载、限制指纹；
- 进程**禁 shell**、禁继承环境、资源 cgroup 限制（未来在 `wasi` or 本地 runner 里落地）；
- 证据**脱敏与摘要化**，避免敏感内容入库；
- **Begin/End 双证据**保证“失败必有记”。

------

## 12. 测试与验收

- **契约测试（soulbase-contract-testkit）**：
  - 能力交集/风险合并/白名单命中/大小阈值；
  - 错误码与公共/审计视图一致性；
  - Evidence 字段完整性与可回放性。
- **安全回归**：路径穿越/符号链接/重定向越界/命令注入/大包炸弹/长尾执行。
- **性能基线**：Profile 合成与 Guard 校验的 p95；执行器自开销曲线。
- **拨测**：预算消耗、撤销实时性、热更策略生效时延。

------

## 13. 版本化与迁移

- `Capability/SafetyClass/SideEffect` 枚举新增仅增不破；移除/语义变更需 **MAJOR**。
- Evidence 字段采用**前向兼容**：新增字段为可选，旧消费者无感。
- Profile 与 PolicyConfig Schema 遵循 **SemVer** 与差异报告。

------

## 14. 开放问题

- **统一隔离载体**：WASI/wasmtime vs 轻量本地 runner 的抽象边界与切换策略。
- **GPU 资源**：跨作业的 GPU 秒/显存 MB*s 计量与 QoS 口径统一。
- **浏览器队列**：限流/并发/池化策略的通用实现是否放入本模块。
- **离线重放**：基于 Evidence 的“模拟执行器”标准接口。

------

### 下一步

若本 TD 符合你的预期，我将按照“三件套”流程输出 **SB-06-RIS（最小可运行骨架）**：

- `ProfileBuilder` / `PolicyGuard` 接口与最小实现；
- `FsExecutor`（只读）与 `NetExecutor`（白名单 GET/HEAD）占位实现；
- `EvidenceSink` 与 `BudgetMeter` 内存版；
- 单测样例：允许/拒绝/预算超限/证据生成。
