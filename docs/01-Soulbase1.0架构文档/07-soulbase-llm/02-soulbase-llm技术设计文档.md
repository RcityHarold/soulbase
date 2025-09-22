# 文档 SB-07-TD：`soulbase-llm` 技术设计（Technical Design）

> 对应功能规约：SB-07（LLM SPI + Provider 插件 / Chat · Tools · Embedding · Rerank）
>  目标：给出 **crate 结构、核心数据模型、Traits/SPI、流式协议、结构化输出守护、用量与成本、错误映射、Provider 插件注册、观测与测试口径**，与 `soulbase-auth / -tools / -sandbox / -qos / -interceptors / -observe / -config / -errors / -types` 保持不变式与接口同频。

------

## 1. Crate 结构与 Feature 规划

```
soulbase-llm/
  src/
    lib.rs
    model.rs          # Role/Message/ContentSegment/ToolCallProposal/Usage/Cost/FinishReason
    chat.rs           # Chat SPI：ChatModel / ChatRequest / ChatResponse / ChatDelta（流式）
    embed.rs          # Embeddings SPI：EmbedModel / EmbedRequest / EmbedResponse
    rerank.rs         # Rerank SPI：RerankModel / RerankRequest / RerankResponse
    provider.rs       # ProviderFactory / Registry / ProviderCaps / ProviderCfg
    jsonsafe.rs       # 结构化输出（JSON/Schema）策略：Strict | Repair | Reject
    cost.rs           # 价目表/估算器（Pricing/Counters），与 QoS 对接
    errors.rs         # LLM 错误到稳定码映射（LLM.* / PROVIDER.* / SCHEMA.* / TIMEOUT）
    observe.rs        # 指标/Trace 标签导出（FTL、tokens/s、err_code…）
    prelude.rs
```

**Feature flags（建议）**

- `provider-openai` / `provider-claude` / `provider-gemini` / `provider-local`
- `schema_json`（依赖 `schemars` 用于 JSON Schema 校验）
- `http-client`（启用 `reqwest`/TLS 等）
- `stream-sse`（SSE 客户端，开放式流）
- `qos`（与 `soulbase-qos` 成本/预算对接）
- `observe`（与 `soulbase-observe` 指标上报）
- `tools-lite`（在 `soulbase-tools` 未就绪时用轻量 ToolSpec 占位，仅 TD 描述，RIS 时可临时启用）

------

## 2. 核心数据模型（`model.rs`）

### 2.1 消息与内容

```rust
pub enum Role { System, User, Assistant, Tool }

pub enum ContentSegment {
  Text { text: String },
  ImageRef { uri: String, mime: String, width: Option<u32>, height: Option<u32> },
  AudioRef { uri: String, seconds: f32, sample_rate: Option<u32>, mime: Option<String> },
  AttachmentRef { uri: String, bytes: Option<u64>, mime: Option<String> },  // 只作元数据引用
}

pub struct Message {
  pub role: Role,
  pub segments: Vec<ContentSegment>,
  pub tool_calls: Vec<ToolCallProposal>,     // Assistant 可能提案工具调用
}

pub struct ToolCallProposal {
  pub name: String,
  pub call_id: sb_types::Id,
  pub arguments: serde_json::Value,          // 按工具 JSON-Schema 校验（仅结构）
}
```

### 2.2 用量与成本

```rust
pub struct Usage {
  pub input_tokens: u32,
  pub output_tokens: u32,
  pub cached_tokens: Option<u32>,
  pub image_units: Option<u32>,
  pub audio_seconds: Option<f32>,
  pub requests: u32,
}

pub struct CostBreakdown { pub input: f32, pub output: f32, pub image: f32, pub audio: f32 }
pub struct Cost { pub usd: f32, pub currency: &'static str, pub breakdown: CostBreakdown }
```

### 2.3 终止与安全

```rust
pub enum FinishReason { Stop, Length, Tool, Safety, Other(String) }
```

------

## 3. Chat SPI（`chat.rs`）

### 3.1 请求/响应/流

```rust
pub struct ResponseFormat {
  pub kind: ResponseKind,                           // Text | Json | JsonSchema
  pub json_schema: Option<schemars::schema::RootSchema>,
  pub strict: bool,                                 // 严格模式（true）触发校验/修复策略
}

pub enum ResponseKind { Text, Json, JsonSchema }

pub struct ChatRequest {
  pub model_id: String,                             // 规范化: "provider:model"
  pub messages: Vec<Message>,
  pub tool_specs: Vec<ToolSpec>,                    // 来自 soulbase-tools 的只读清单（仅名称/参数结构）
  pub temperature: Option<f32>,
  pub top_p: Option<f32>,
  pub max_tokens: Option<u32>,
  pub stop: Vec<String>,
  pub seed: Option<u64>,
  pub frequency_penalty: Option<f32>,
  pub presence_penalty: Option<f32>,
  pub logit_bias: serde_json::Map<String, serde_json::Value>,
  pub response_format: Option<ResponseFormat>,
  pub idempotency_key: Option<String>,
  pub allow_sensitive: bool,                        // 默认 false，需要策略与 Consent
  pub metadata: serde_json::Value,                  // 追踪/路由/策略辅助
}

pub struct ChatResponse {
  pub model_id: String,
  pub message: Message,                             // 汇总后的 Assistant 消息（合并所有增量）
  pub usage: Usage,
  pub cost: Option<Cost>,
  pub finish: FinishReason,
  pub provider_meta: serde_json::Value,             // 不稳定信息：原始对齐
}

pub struct ChatDelta {
  pub text_delta: Option<String>,
  pub tool_call_delta: Option<ToolCallProposal>,    // 增量提案（新增/补全参数）
  pub usage_partial: Option<Usage>,                 // 流式用量累积
  pub finish: Option<FinishReason>,
  pub first_token_ms: Option<u32>,                  // 首 token 时间
}
```

> `ToolSpec`：来自 `soulbase-tools`（名称 + JSON-Schema），本模块只把它**只读披露**给模型，不执行。

### 3.2 Trait

```rust
#[async_trait::async_trait]
pub trait ChatModel: Send + Sync {
  type Stream: futures_core::Stream<Item = Result<ChatDelta, LlmError>> + Unpin + Send + 'static;

  async fn chat(&self, req: ChatRequest, enforce: &StructOutPolicy) -> Result<ChatResponse, LlmError>;

  async fn chat_stream(&self, req: ChatRequest, enforce: &StructOutPolicy)
      -> Result<Self::Stream, LlmError>;
}
```

------

## 4. Embeddings / Rerank SPI（`embed.rs` / `rerank.rs`）

```rust
pub struct EmbedItem { pub id: String, pub text: String }
pub struct EmbedRequest { pub model_id: String, pub items: Vec<EmbedItem>, pub normalize: bool, pub pooling: Option<String> }
pub struct EmbedResponse { pub dim: u32, pub vectors: Vec<Vec<f32>>, pub usage: Usage, pub cost: Option<Cost>, pub provider_meta: serde_json::Value }

#[async_trait::async_trait]
pub trait EmbedModel: Send + Sync {
  async fn embed(&self, req: EmbedRequest) -> Result<EmbedResponse, LlmError>;
}

pub struct RerankRequest { pub model_id: String, pub query: String, pub candidates: Vec<String> }
pub struct RerankResponse { pub scores: Vec<f32>, pub ordering: Vec<usize>, pub usage: Usage, pub cost: Option<Cost>, pub provider_meta: serde_json::Value }

#[async_trait::async_trait]
pub trait RerankModel: Send + Sync {
  async fn rerank(&self, req: RerankRequest) -> Result<RerankResponse, LlmError>;
}
```

------

## 5. 结构化输出守护（`jsonsafe.rs`）

### 5.1 策略

```rust
pub enum StructOutPolicy {
  Off,                                // 不校验
  StrictReject,                       // 校验失败即报错（SCHEMA.VALIDATION_FAILED）
  StrictRepair { max_attempts: u8 },  // 尝试“温和修复”若仍失败则报错
}
```

### 5.2 行为

- 当 `response_format = Json|JsonSchema` 且 `strict=true`：
  1. 校验 JSON 格式 →
  2. 若有 Schema：用 `schemars` 校验结构/必填/枚举 →
  3. 若失败且策略为 `StrictRepair`：在**不改变语义**前提下修正（去除多余字段、null→缺省、类型窄化），仍失败→返回 `SCHEMA.VALIDATION_FAILED`。
- 修复日志只入**审计**，不对外返回原始未修复内容。

------

## 6. Provider 插件系统（`provider.rs`）

```rust
pub struct ProviderCaps {
  pub chat: bool, pub stream: bool, pub tools: bool,
  pub embeddings: bool, pub rerank: bool,
  pub multimodal: bool, pub json_schema: bool,
}

pub struct ProviderCfg { pub name: String, pub http: serde_json::Value, pub models: Vec<String> }

pub trait ProviderFactory: Send + Sync {
  fn name(&self) -> &'static str;
  fn create_chat(&self, model: &str, cfg: &ProviderCfg) -> Option<Box<dyn ChatModel>>;
  fn create_embed(&self, model: &str, cfg: &ProviderCfg) -> Option<Box<dyn EmbedModel>>;
  fn create_rerank(&self, model: &str, cfg: &ProviderCfg) -> Option<Box<dyn RerankModel>>;
  fn caps(&self) -> ProviderCaps;
}

pub struct Registry { /* 内含 HashMap<(provider, kind) -> factory> */ }

impl Registry {
  pub fn register(&mut self, factory: Box<dyn ProviderFactory>) { /* ... */ }
  pub fn chat(&self, model_id: &str) -> Option<Box<dyn ChatModel>> { /* 解析 "provider:model" → factory */ }
  pub fn embed(&self, model_id: &str) -> Option<Box<dyn EmbedModel>> { /* ... */ }
  pub fn rerank(&self, model_id: &str) -> Option<Box<dyn RerankModel>> { /* ... */ }
}
```

> **配置装配**：`soulbase-config` 提供 `model_aliases`, `pricing`, `timeouts`, `retries`，注册时注入。

------

## 7. 成本与用量（`cost.rs`）

### 7.1 价目表

- `PricingTable`: `{ "provider": { "model": { "input_per_1k": $, "output_per_1k": $, "image_unit": $, "audio_sec": $ } } }`
- 从 `soulbase-config` 热更新并带版本号；`soulbase-llm` 仅计算**建议成本**，最终对账由 `soulbase-qos` 聚合。

### 7.2 计量口径

- Provider 报告优先；若无，则使用**本地估算器**（参考分词器与经验值）。
- **流式**：每个 delta 可提供 `usage_partial`，终态做一次**对齐校正**。

------

## 8. 错误映射（`errors.rs`）

| 场景                                | 稳定码                                    | 对外 HTTP/gRPC 建议 |
| ----------------------------------- | ----------------------------------------- | ------------------- |
| 认证/鉴权失败（API Key/OIDC）       | `AUTH.UNAUTHENTICATED` / `AUTH.FORBIDDEN` | 401/403             |
| 上下文超限（tokens/context/window） | `LLM.CONTEXT_OVERFLOW`                    | 400                 |
| 超时（连接/首包/整体）              | `LLM.TIMEOUT`                             | 503/504             |
| 供应商拒绝/安全阻断                 | `LLM.SAFETY_BLOCK`                        | 400/403             |
| 供应商不可用/网络错误               | `PROVIDER.UNAVAILABLE`                    | 503                 |
| 结构化输出校验失败                  | `SCHEMA.VALIDATION_FAILED`                | 422                 |
| 预算/配额不足                       | `QUOTA.BUDGET_EXCEEDED`                   | 429                 |
| 未分类                              | `UNKNOWN.INTERNAL`                        | 500                 |

> 所有错误统一产出 `ErrorObj`（公共/审计视图分离），并在指标打 `code/kind/retryable/severity` 标签。

------

## 9. 观测与指标（`observe.rs`）

**关键指标**

- `llm_requests_total{provider,model,mode=chat|stream|embed|rerank}`
- `llm_latency_ms{phase=total|first_token}`
- `llm_tokens{type=input|output|cached}`
- `llm_cost_usd_total{provider,model}`
- `llm_errors_total{code}`
- `llm_stream_throughput_tokens_per_s`

**Trace 标签最小集**：`tenant`, `model_id`, `provider`, `tool_proposal=true|false`, `code`

------

## 10. 安全与策略协同

- **工具清单只读披露**：仅名称+参数 JSON-Schema；禁止提示中披露**授权/密钥**信息。
- **敏感默认关闭**：`allow_sensitive=false`；需要策略与 `Consent`（由 `soulbase-auth` 验证）。
- **提示注入防护**：SPI 层提供**系统前缀**（不可覆写）注入安全规则与边界提醒。
- **数据最小披露**：日志/指标/Evidence 不落原文，只存**hash/长度**等摘要。

------

## 11. 与其他模块接口

- **soulbase-auth**：在上层拦截器完成 AuthN/AuthZ/Quota；本模块读取 `Subject/Consent` 只作**记录和安全提示**（不二次判权）。
- **soulbase-tools**：`ToolSpec` 由 Tools 层生成；本模块不执行，仅产生提案。
- **soulbase-sandbox**：**不直接调用**；内核依据提案选择是否进入 Sandbox 执行。
- **soulbase-qos**：将 `Usage/Cost` 汇报；核对预算/成本与租户视图。
- **soulbase-config**：提供 `model_aliases/pricing/timeouts/retries/struct_out_policy`；支持热更。
- **soulbase-interceptors/observe**：承接追踪与错误公共视图、暴露指标。

------

## 12. 弹性与可靠性（可选实现建议）

- **超时**：分解为 `connect`, `ttfb`（首 token）, `complete`；
- **重试**：仅对 `PROVIDER.UNAVAILABLE`/部分幂等错误，指数退避；禁止对已产生工具提案的流进行自动重试（由内核决策）。
- **熔断**：按 provider/model 维度；打开后快速失败并建议降级模型。

------

## 13. 流式一致性保障

- **单调拼接**：`text_delta` 必须按顺序且不回退；
- **提案幂等**：`tool_call_delta` 的 `call_id` 持久，参数补齐不得**修改已确认字段**；
- **终态一致**：流式汇总结果与非流模式输出一致（允许空白字符差异≤1%）。

------

## 14. 测试与验收（契约/黑盒/压测）

- **契约测试**（`soulbase-contract-testkit`）：
  - 多 Provider 的 Chat/Stream/Embed/Rerank 输入输出契约；
  - 结构化输出（Strict/Repair/Reject）矩阵；
  - 错误映射表一致性；
  - 计量/成本与配置价表一致性。
- **黑盒**：
  - 工具提案仅在允许清单内；敏感开关严格生效；
  - 流式/非流一致；超时/熔断/重试策略起效。
- **压测**：
  - FTL/吞吐指标与 Provider 基线的偏差上限；
  - 并发下的流控与内存占用曲线。

------

## 15. 版本化与迁移

- SPI 与数据模型遵循 **SemVer**；新增字段只增不破；枚举新增分支保持向后兼容。
- `ToolSpec` 将在 `soulbase-tools` 稳定后切换为正式类型；在此之前可用 `tools-lite` 过渡。
- `ResponseFormat` 与修复策略新增选项时，默认行为保持“不更改现有严格/修复/拒绝的语义”。

------

## 16. 开放问题

- 统一的**多模输入批注**（时间戳/区域标注）抽象与各 Provider 对齐；
- **对抗样本/安全红线**：是否在 SPI 层直接引入“红线词典/内容盾”钩子；
- **Prompt 语义缓存**：是否在 SPI 层内置 N×M 近邻缓存接口（需与 QoS/Observe 协同）。

------

> 若以上 TD 符合你的预期，我将按“三件套”节奏输出 **SB-07-RIS（最小可运行骨架）**：包含 `ChatModel/EmbedModel/RerankModel` Traits、消息与流式类型、`ProviderRegistry`、`StructOutPolicy` 桩实现、`openai/local` 两个示范 Provider 的最小占位实现与单测（流式一致性/结构化校验/错误映射）。
