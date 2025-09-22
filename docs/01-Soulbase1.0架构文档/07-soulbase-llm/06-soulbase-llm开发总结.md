
# 07-soulbase-llm · 开发总结

## 1. 当前实现概述
- 已完成 `sb-llm` crate，提供统一的 LLM SPI：Chat（同步/流式）、Embeddings、Rerank。
- 模块默认集成本地 `local` Provider，覆盖对话、流式一致性、多段输出、结构化输出守护、Embedding、Rerank 全链路。
- 通过 `sb-errors` 补齐 `LLM.CONTEXT_OVERFLOW`、`LLM.SAFETY_BLOCK` 稳定错误码，保证与全局错误域一致。

## 2. 核心能力清单
- **消息模型**：Role/ContentSegment/ToolCallProposal，支持文本、多模引用与工具提案。
- **Chat SPI**：统一请求参数、流式 `ChatDelta`、结构化输出策略（Off/StrictReject/StrictRepair），并返回 Usage/Cost。
- **Embeddings/Rerank SPI**：向量维度、分数、用量、成本输出规范统一。
- **Provider 注册**：`Registry + ProviderFactory` 支持按 `provider:model` 动态装配；本地 Provider 示例可直接通过 `LocalProviderFactory::install` 使用。
- **结构化输出守护**：`StructOutPolicy` 提供 JSON 校验与轻修复；若启用 schema-json 特性，可结合 JSON Schema 校验。
- **成本/用量计量**：内置估算器及 `zero_cost` 占位，后续可接 `sb-qos` 真实账单。
- **稳定错误**：通过 `LlmError` 统一构造 `PROVIDER.UNAVAILABLE / LLM.TIMEOUT / LLM.CONTEXT_OVERFLOW / LLM.SAFETY_BLOCK / SCHEMA.VALIDATION_FAILED / UNKNOWN.INTERNAL` 等错误码。

## 3. 测试与验证
- `cargo test -p sb-llm` 全量通过，覆盖：
  - 同步与流式输出文本一致性；
  - JSON 严格模式校验；
  - Embeddings 与 Rerank 流程。

## 4. 与周边模块协同
- **sb-errors**：新增 LLM 相关稳定码，对齐公共视图。
- **sb-types**：复用 `Id` 等基础类型。
- 预留与 **sb-tools**、**sb-qos**、**sb-observe** 的接口点，后续接入真实 ToolSpec/预算/指标时无需变更 SPI。

## 5. 后续增强建议
1. **真实 Provider 适配**：通过 feature flag 接入 OpenAI/Claude/Gemini 等 Provider，处理鉴权、SSE、重试、熔断等实际运行逻辑。
2. **QoS 与成本对账**：联动 `sb-qos`，将 Usage/Cost 落入统一账本与预算扣减逻辑。
3. **观测与指标**：扩展 `observe.rs`，输出 `llm_requests_total`、`llm_latency_ms`、`llm_errors_total` 等指标并携带稳定标签。
4. **更强结构化守护**：与 JSON Schema 深度结合，支持字段级纠错、枚举修复、提示上下文注入等策略。
5. **工具生态集成**：在 `sb-tools` 成熟后，将占位 `ToolSpec` 切换为正式类型，并扩展工具提案的 schema 校验、权限提示。
