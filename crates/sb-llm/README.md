# sb-llm

Provider-agnostic LLM SPI offering chat (sync + stream), embeddings, and rerank traits with a local in-memory provider implementation.

- Unified message model with tool-call proposals and structured-output guard rails
- Basic cost/usage accounting helpers and stable error mapping via sb-errors
- Provider registry/trait system ready for feature-gated adapters (OpenAI/Claude/etc.)

## Development

    cargo check -p sb-llm
    cargo test -p sb-llm

## Next Steps

- Wire real provider adapters with HTTP clients and authentication
- Integrate sb-tools ToolSpec, QoS budgeting, and observability metrics
- Expand structured-output repair strategies and schema validation coverage
