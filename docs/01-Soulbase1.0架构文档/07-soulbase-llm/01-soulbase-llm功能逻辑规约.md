### **文档 SB-07：soulbase-llm（LLM SPI + Provider 插件 / Chat · Tools · Embedding · Rerank）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：为 Soul 生态提供**供应商无关（provider-agnostic）\**的 LLM 能力底座，统一\**对话（Chat/Stream）**、**工具提案（Tool-Calling Proposal）**、**结构化输出（JSON/Schema）**、**向量嵌入（Embeddings）**、**重排（Rerank）**、**多模输入**、**成本/用量计量**与**稳定错误语义**，与 `soulbase-auth / -tools / -sandbox / -qos / -interceptors / -observe` 严密协作。
- **范围**：
  - **接口抽象**：`ChatModel / EmbedModel / RerankModel`；
  - **消息/多模**：文本 +（可选）图像/音频引用的统一消息模型；
  - **工具提案**：基于 Tools Manifest 的**提案**（不执行）；
  - **流式输出**：增量文本/工具提案 delta；
  - **结构化输出**：JSON/Schema 校验、保底修复；
  - **用量/成本**：token 与其它单元（图像帧/音频秒）的归一计量；
  - **错误**：`LLM.TIMEOUT / LLM.CONTEXT_OVERFLOW / LLM.SAFETY_BLOCK / PROVIDER.UNAVAILABLE` 等稳定码；
  - **插件**：多 Provider 适配（OpenAI/Claude/Gemini/本地/自托管）。
- **非目标**：不在本模块内执行工具/外部行动（由 `soulbase-sandbox` 执行）；不做模型路由/预算策略（属内核策略域与 `soulbase-qos`）；不绑定任一厂商高级特性语义（以能力并集暴露）。

------

#### **1. 功能定位（Functional Positioning）**

- **统一标准面**：屏蔽 Provider 差异，提供**稳定消息模型**与**一致的流式协议**。
- **安全网**：对结构化输出做**Schema 校验与温和修复**；对 Provider 拒绝/敏感阻断做**稳定错误映射**与**最小披露**。
- **计量与治理**：作为**真实用量与成本**的权威入口，向 `soulbase-qos` 报账；向 `soulbase-observe` 上报延迟/吞吐/命中等指标。
- **与工具生态闭环**：仅产出**工具提案**（ToolCallProposal），由内核策略 + `soulbase-sandbox` 决策与执行。

------

#### **2. 系统角色与地位（System Role & Position）**

- 所处层级：**基石层 / Soul-Base**；被内核策略（模型路由/上下文治理/工具策略）、应用后端直接依赖。
- 关系：
  - `sb-types`：统一 `Envelope/Subject/Consent` 与消息/提案数据契约；
  - `soulbase-auth`：调用前身份/配额；敏感场景下附带 `Consent`；
  - `soulbase-tools`：消费 Tools Manifest，暴露给模型的**受限工具清单**用于“提案”；
  - `soulbase-sandbox`：**不**直接调用，只把提案回交策略层；
  - `soulbase-qos`：统计 token/秒等**预算单元**并扣账；
  - `soulbase-observe`：延迟/用量/错误码与模型命中指标；
  - `soulbase-interceptors`：承接标准头/追踪与错误公共视图；
  - `soulbase-config`：模型与价目表、结构化输出策略、超时/重试等配置。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

**3.1 消息模型（Messages）**

- **Role**：`System | User | Assistant | Tool`；
- **Content**：多段（segments）结构，支持：
  - `text`（UTF-8），
  - `image_ref`（沙箱/对象存储的引用 URI + mime/尺寸摘要），
  - `audio_ref`（可选，秒数与采样率摘要），
  - `attachment_ref`（沙箱制品引用；仅元数据参与推理）；
- **ToolCallProposal**：`{name, call_id, arguments(json)}`，由模型提出，**不执行**。

**3.2 Chat 请求（ChatRequest）**

- `model_id`（规范化：`provider:model`），`messages[]`，`tool_specs[]`（从 `soulbase-tools` 的 Manifest 下采样且**只读**）；
- 采样参数：`temperature/top_p/max_tokens/stop/seed/frequency_penalty/presence_penalty/logit_bias[]`；
- **结构化输出**：`response_format = text | json | schema{json_schema}`；
- **缓存/幂等**：`idempotency_key`、`cache_hint`（便于上层命中）；
- **安全旗标**：`allow_sensitive=false` 默认；开启需 `Consent` 与策略批准。

**3.3 流式增量（ChatDelta）**

- `text_delta`、`tool_call_delta`、`finish_reason`（stop/length/tool/safety/other）、`usage_partial`、`timestamps`（first_token_ms 等）。

**3.4 Embeddings / Rerank**

- `EmbedRequest{ model_id, items[], normalize, pooling }` → `vectors[], dim, dtype, usage`;
- `RerankRequest{ model_id, query, candidates[] }` → `scores[]/ordering/usage`（cross/bi 统一抽象）。

**3.5 用量与成本（Usage & Cost）**

- `Usage{ input_tokens, output_tokens, cached_tokens?, image_units?, audio_seconds?, requests }`；
- `Cost{ usd, currency="USD", breakdown{input,output,image,audio}}`（价目由配置/Provider 表驱动）；
- 每次调用均产出**可聚合计量**。

------

#### **4. 不变式（Invariants）**

1. **供应商无关**：SPI 输出与错误语义不随 Provider 变化；
2. **只提案不执行**：工具“提案”不可在本模块内落地；
3. **结构化输出可验证**：`response_format=json/schema` 必经 Schema 校验；失败将按策略选择**保底修复或报错**；
4. **最小披露**：日志/证据不存原文，只记**摘要与统计**；
5. **稳定错误**：Provider/网络/安全/上下文等错误**统一映射**到 `soulbase-errors`；
6. **可重现**：流式序列与终态结果**一致**；seed 在可支持情况下提供**幂等近似**；
7. **预算前置**：调用前检查配额；用量回写 `soulbase-qos`；
8. **安全默认关闭**：`allow_sensitive=false`；高风险输出需策略 + Consent。

------

#### **5. 能力与抽象接口（Abilities & Abstract Interfaces）**

> 仅定义**能力与行为**；具体 Traits/插件在 TD/RIS 落地。

- **Chat（同步/流式）**
  - 输入：`ChatRequest`；输出：`ChatResponse` 或流 `ChatDelta`；
  - 支持工具提案/结构化输出；
  - 用量/成本回填；延迟标注。
- **Embeddings**
  - 文本/多模嵌入；可选 `normalize/pooling`；批量/并行；
  - 输出维度/向量 dtype/usage/cost。
- **Rerank**
  - 输入查询 + 候选列表；输出带分数的排序与 usage/cost。
- **插件注册**
  - `ProviderFactory.register(name, cfg)` → `ChatModel/EmbedModel/RerankModel`；
  - 热更新：模型别名/价目表/超时/重试策略可由 `soulbase-config` 更新。
- **结构化输出守护**
  - `json_strict=true` 时启用严格 JSON；若 Provider 输出破损：
    - **优先校验** → **温和修复（可选）** → **返回校验错误**（`SCHEMA.VALIDATION_FAILED`）。
- **提示工程保护**
  - 内置**系统前缀（不可见）**注入供应商提示的防注入/越权提醒；
  - 对工具清单/安全政策做**只读**披露，禁止模型“自我授权”。

------

#### **6. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **性能目标**（单模型，无网络拥塞）：
  - 首 token 延迟（FTL）p95 **≤ 1.5× Provider 基线**；
  - 流式稳定吞吐（tokens/s）与 Provider 误差 **≤ 5%**；
- **一致性**：流/非流**语义一致**（终态文本一致，工具提案序列等价）；
- **结构化正确率**：`json/schema` 模式**有效率 ≥ 99.5%**（其余按策略修复或报错）；
- **错误映射**：所有失败**100%** 有稳定错误码；`UNKNOWN.*` **≤ 0.1%**；
- **计量准确性**：与 Provider 账单抽样核对误差 **≤ 0.5%**；
- **验收**：契约测试覆盖**多 Provider**、流式一致性、结构化输出、工具提案、错误映射与计量。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：`soulbase-config`（模型映射/价目/超时/重试）、`soulbase-auth`（令牌/租户/配额）、`soulbase-tools`（工具清单）
- **下游**：内核策略（模型路由/上下文治理/工具策略）、`soulbase-qos`（预算）、`soulbase-observe`（指标）
- **边界**：不做工具执行与外网行动（交 `soulbase-sandbox`）；不做复杂上下文裁剪（交内核 `context-governor`）；不落持久化。

------

#### **8. 风险与控制（Risks & Controls）**

- **提示注入/越权**：系统提示**强前缀**；工具清单仅披露只读元数据；提案需二次授权。
- **结构化失败**：启用“严格模式 + 轻修复 + 报错”三段式；提供失败计数与回退策略。
- **账单偏差**：双计量（Provider 原始 + 本地估算）并对账；异常自动告警。
- **隐私泄露**：日志与 Evidence 仅存**摘要**；可配置安全红线（敏感字段屏蔽）。
- **供应商波动**：超时/熔断/重试策略与模型多活；必要时自动降级到**较小模型**或**只读回答**。
- **上下文溢出**：基于 `context-governor` 的裁剪/压缩信号；溢出直接 `LLM.CONTEXT_OVERFLOW`。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 对话（流式 + 工具提案）**

1. 内核组装 `ChatRequest`（包含工具清单）→ `soulbase-llm`；
2. Provider 流式输出：`text_delta` 与 `tool_call_delta`；
3. `soulbase-llm` 规范化 delta、回填部分用量与时间戳；
4. 终态汇总：文本/提案列表/usage/cost/finish_reason；
5. 内核策略读取“提案”→ 交给 `soulbase-sandbox` 执行或拒绝。

**9.2 结构化输出（JSON/Schema）**

1. `response_format=schema{json_schema}`；
2. 生成内容先校验：通过→返回；失败→轻修复（可选）→仍失败→`SCHEMA.VALIDATION_FAILED`。

**9.3 Embeddings**

1. 批量输入→并行批处理→返回 `vectors[]/dim/usage/cost`；
2. 计量与价目表由配置驱动；错误映射 Provider 细节为稳定码。

**9.4 Rerank**

1. 输入 query 与 candidates；
2. 返回每个候选的分数与排序/usage/cost；
3. 错误与计量路径等同。

------

#### **10. 开放问题（Open Issues / TODO）**

- **多模音频/视频**的通用抽象与单位计量（秒/帧）与价目对齐；
- **函数级别对齐**（工具提案参数的 JSON-Schema 自动纠错/强约束）；
- **可插拔缓存**（prompt 语义哈希、向量近邻缓存）与一致性策略；
- **不同 Provider 安全分级**与**策略降级矩阵**（例如对高风险场景禁用工具提案）；
- **批量/并发队列**的通用调度策略是否内置或交由 `soulbase-qos` 统一治理。

------

> 本规约与既有文档**同频共振**：强调**SSoT 契约**、**稳定错误语义**、**只提案不执行**、**预算/观测闭环**与**结构化输出可验证**。若你确认无误，我将按“三件套”继续输出 **SB-07-TD（技术设计）**。
