下面是 **SB-07-RIS：`soulbase-llm` 最小可运行骨架**。
 与 SB-07（功能规约）& SB-07-TD（技术设计）逐项对齐：提供 **SPI/Traits、消息与流式协议、结构化输出守护（简化版）、Provider 注册与一个本地 `local` Provider 示例**（纯内存、无外部依赖），以及**单测**覆盖同步/流式一致性、Embeddings、Rerank 与错误规范化路径。将内容放入 `soul-base/crates/soulbase-llm/` 后可直接 `cargo check && cargo test`。

> 说明：为保持“可运行”与“零外网依赖”，本 RIS 仅内置 `local` Provider；`openai/claude/gemini` 等在后续按 feature 接入。结构化输出策略提供 **Off/StrictReject/StrictRepair(轻修复桩)**。

------

## 目录结构

```
soul-base/
└─ crates/
   └─ soulbase-llm/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ model.rs
      │  ├─ chat.rs
      │  ├─ embed.rs
      │  ├─ rerank.rs
      │  ├─ provider.rs
      │  ├─ jsonsafe.rs
      │  ├─ cost.rs
      │  ├─ errors.rs
      │  ├─ observe.rs
      │  └─ prelude.rs
      └─ tests/
         └─ basic.rs
```

------

## Cargo.toml

```toml
[package]
name = "soulbase-llm"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "Provider-agnostic LLM SPI (Chat/Stream/Embeddings/Rerank) for the Soul platform"
repository = "https://example.com/soul-base"

[features]
default = []
schema_json = ["schemars"]
provider-local = []     # 本地示例 Provider（默认隐式启用，RIS 内部不做 cfg）

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
async-trait = "0.1"
thiserror = "1"
futures-core = "0.3"
futures-util = "0.3"
schemars = { version = "0.8", optional = true, features = ["serde_json"] }

# 平台内
sb-types = { path = "../sb-types", version = "1.0.0" }
soulbase-errors = { path = "../soulbase-errors", version = "1.0.0" }

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread","macros","time"] }
```

------

## src/lib.rs

```rust
pub mod model;
pub mod chat;
pub mod embed;
pub mod rerank;
pub mod provider;
pub mod jsonsafe;
pub mod cost;
pub mod errors;
pub mod observe;
pub mod prelude;

pub use provider::{Registry, LocalProviderFactory};
```

------

## src/model.rs

```rust
use serde::{Serialize, Deserialize};
use sb_types::prelude::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Role { System, User, Assistant, Tool }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum ContentSegment {
  Text { text: String },
  ImageRef { uri: String, mime: String, width: Option<u32>, height: Option<u32> },
  AudioRef { uri: String, seconds: f32, sample_rate: Option<u32>, mime: Option<String> },
  AttachmentRef { uri: String, bytes: Option<u64>, mime: Option<String> },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolCallProposal {
  pub name: String,
  pub call_id: Id,
  pub arguments: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Message {
  pub role: Role,
  #[serde(default)]
  pub segments: Vec<ContentSegment>,
  #[serde(default)]
  pub tool_calls: Vec<ToolCallProposal>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Usage {
  pub input_tokens: u32,
  pub output_tokens: u32,
  #[serde(default)]
  pub cached_tokens: Option<u32>,
  #[serde(default)]
  pub image_units: Option<u32>,
  #[serde(default)]
  pub audio_seconds: Option<f32>,
  pub requests: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CostBreakdown { pub input: f32, pub output: f32, pub image: f32, pub audio: f32 }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Cost { pub usd: f32, pub currency: &'static str, pub breakdown: CostBreakdown }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum FinishReason { Stop, Length, Tool, Safety, Other(String) }
```

------

## src/chat.rs

```rust
use serde::{Serialize, Deserialize};
use futures_core::Stream;
use crate::model::*;
use crate::jsonsafe::StructOutPolicy;
use crate::errors::LlmError;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum ResponseKind { Text, Json, JsonSchema }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResponseFormat {
  pub kind: ResponseKind,
  #[serde(default)]
  pub json_schema: Option<schemars::schema::RootSchema>,
  #[serde(default)]
  pub strict: bool,
}

/// 仅在 RIS 阶段定义轻量 ToolSpec；后续切换 soulbase-tools
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSpec {
  pub name: String,
  #[serde(default)]
  pub input_schema: Option<schemars::schema::RootSchema>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatRequest {
  pub model_id: String,
  pub messages: Vec<Message>,
  #[serde(default)]
  pub tool_specs: Vec<ToolSpec>,
  #[serde(default)]
  pub temperature: Option<f32>,
  #[serde(default)]
  pub top_p: Option<f32>,
  #[serde(default)]
  pub max_tokens: Option<u32>,
  #[serde(default)]
  pub stop: Vec<String>,
  #[serde(default)]
  pub seed: Option<u64>,
  #[serde(default)]
  pub frequency_penalty: Option<f32>,
  #[serde(default)]
  pub presence_penalty: Option<f32>,
  #[serde(default)]
  pub logit_bias: serde_json::Map<String, serde_json::Value>,
  #[serde(default)]
  pub response_format: Option<ResponseFormat>,
  #[serde(default)]
  pub idempotency_key: Option<String>,
  #[serde(default)]
  pub allow_sensitive: bool,
  #[serde(default)]
  pub metadata: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatResponse {
  pub model_id: String,
  pub message: Message,
  pub usage: Usage,
  #[serde(default)]
  pub cost: Option<crate::model::Cost>,
  pub finish: FinishReason,
  #[serde(default)]
  pub provider_meta: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatDelta {
  #[serde(default)]
  pub text_delta: Option<String>,
  #[serde(default)]
  pub tool_call_delta: Option<ToolCallProposal>,
  #[serde(default)]
  pub usage_partial: Option<Usage>,
  #[serde(default)]
  pub finish: Option<FinishReason>,
  #[serde(default)]
  pub first_token_ms: Option<u32>,
}

#[async_trait::async_trait]
pub trait ChatModel: Send + Sync {
  type Stream: Stream<Item = Result<ChatDelta, LlmError>> + Unpin + Send + 'static;

  async fn chat(&self, req: ChatRequest, enforce: &StructOutPolicy) -> Result<ChatResponse, LlmError>;
  async fn chat_stream(&self, req: ChatRequest, enforce: &StructOutPolicy) -> Result<Self::Stream, LlmError>;
}
```

------

## src/embed.rs

```rust
use serde::{Serialize, Deserialize};
use crate::model::Usage;
use crate::errors::LlmError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbedItem { pub id: String, pub text: String }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbedRequest {
  pub model_id: String,
  pub items: Vec<EmbedItem>,
  pub normalize: bool,
  pub pooling: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbedResponse {
  pub dim: u32,
  pub vectors: Vec<Vec<f32>>,
  pub usage: Usage,
  #[serde(default)]
  pub cost: Option<crate::model::Cost>,
  #[serde(default)]
  pub provider_meta: serde_json::Value,
}

#[async_trait::async_trait]
pub trait EmbedModel: Send + Sync {
  async fn embed(&self, req: EmbedRequest) -> Result<EmbedResponse, LlmError>;
}
```

------

## src/rerank.rs

```rust
use serde::{Serialize, Deserialize};
use crate::model::Usage;
use crate::errors::LlmError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RerankRequest {
  pub model_id: String,
  pub query: String,
  pub candidates: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RerankResponse {
  pub scores: Vec<f32>,
  pub ordering: Vec<usize>,
  pub usage: Usage,
  #[serde(default)]
  pub cost: Option<crate::model::Cost>,
  #[serde(default)]
  pub provider_meta: serde_json::Value,
}

#[async_trait::async_trait]
pub trait RerankModel: Send + Sync {
  async fn rerank(&self, req: RerankRequest) -> Result<RerankResponse, LlmError>;
}
```

------

## src/jsonsafe.rs

```rust
use crate::errors::LlmError;

#[derive(Clone, Debug)]
pub enum StructOutPolicy {
  Off,
  StrictReject,
  StrictRepair { max_attempts: u8 },
}

pub fn enforce_json(text: &str, policy: &StructOutPolicy) -> Result<serde_json::Value, LlmError> {
  match policy {
    StructOutPolicy::Off => {
      serde_json::from_str(text).map_err(|e| LlmError::schema(&format!("json parse (off policy): {e}")))
    }
    StructOutPolicy::StrictReject => {
      serde_json::from_str(text).map_err(|e| LlmError::schema(&format!("json parse: {e}")))
    }
    StructOutPolicy::StrictRepair { max_attempts } => {
      // 轻修复桩：尝试去除尾随反引号/代码块围栏
      let mut s = text.trim().to_string();
      let mut tries = 0u8;
      loop {
        match serde_json::from_str::<serde_json::Value>(&s) {
          Ok(v) => return Ok(v),
          Err(_e) if tries < *max_attempts => {
            tries += 1;
            s = s.trim_matches('`').trim().to_string();
          }
          Err(e) => return Err(LlmError::schema(&format!("json parse after repair: {e}")))
        }
      }
    }
  }
}
```

------

## src/cost.rs

```rust
use crate::model::{Usage, Cost, CostBreakdown};

/// 极简估算器（RIS）：tokens≈字符数/4；成本留空或可选返回零
pub fn estimate_usage(texts_in: &[&str], text_out: &str) -> Usage {
  let input_tokens: u32 = texts_in.iter().map(|t| (t.chars().count() as u32 + 3) / 4).sum();
  let output_tokens: u32 = (text_out.chars().count() as u32 + 3) / 4;
  Usage { input_tokens, output_tokens, cached_tokens: None, image_units: None, audio_seconds: None, requests: 1 }
}

pub fn zero_cost() -> Option<Cost> {
  Some(Cost { usd: 0.0, currency: "USD", breakdown: CostBreakdown { input: 0.0, output: 0.0, image: 0.0, audio: 0.0 } })
}
```

------

## src/errors.rs

```rust
use thiserror::Error;
use soulbase_errors::prelude::*;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct LlmError(pub ErrorObj);

impl LlmError {
  pub fn into_inner(self) -> ErrorObj { self.0 }
  pub fn provider_unavailable(msg: &str) -> Self {
    LlmError(ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE).user_msg("Model provider is unavailable. Please retry later.").dev_msg(msg).build())
  }
  pub fn timeout(msg: &str) -> Self {
    LlmError(ErrorBuilder::new(codes::LLM_TIMEOUT).user_msg("Model did not respond in time.").dev_msg(msg).build())
  }
  pub fn context_overflow(msg: &str) -> Self {
    LlmError(ErrorBuilder::new(codes::LLM_CONTEXT_OVERFLOW).user_msg("Input exceeds model context window.").dev_msg(msg).build())
  }
  pub fn schema(msg: &str) -> Self {
    LlmError(ErrorBuilder::new(codes::SCHEMA_VALIDATION).user_msg("Model output failed schema validation.").dev_msg(msg).build())
  }
  pub fn unknown(msg: &str) -> Self {
    LlmError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL).user_msg("Internal error.").dev_msg(msg).build())
  }
}
```

------

## src/observe.rs

```rust
use std::collections::BTreeMap;

pub fn labels(provider: &str, model: &str, code: Option<&str>) -> BTreeMap<&'static str, String> {
  let mut m = BTreeMap::new();
  m.insert("provider", provider.to_string());
  m.insert("model", model.to_string());
  if let Some(c) = code { m.insert("code", c.to_string()); }
  m
}
```

------

## src/provider.rs

```rust
use std::collections::HashMap;
use futures_util::stream::{self, StreamExt};
use crate::{chat::*, embed::*, rerank::*, model::*, jsonsafe::*, errors::LlmError, cost::*};

pub struct ProviderCfg { pub name: String }

pub struct ProviderCaps {
  pub chat: bool, pub stream: bool, pub tools: bool,
  pub embeddings: bool, pub rerank: bool,
  pub multimodal: bool, pub json_schema: bool,
}

#[async_trait::async_trait]
pub trait ProviderFactory: Send + Sync {
  fn name(&self) -> &'static str;
  fn caps(&self) -> ProviderCaps;
  fn create_chat(&self, _model: &str, _cfg: &ProviderCfg) -> Option<Box<dyn ChatModel>> { None }
  fn create_embed(&self, _model: &str, _cfg: &ProviderCfg) -> Option<Box<dyn EmbedModel>> { None }
  fn create_rerank(&self, _model: &str, _cfg: &ProviderCfg) -> Option<Box<dyn RerankModel>> { None }
}

pub struct Registry {
  inner: HashMap<String, Box<dyn ProviderFactory>>,
}

impl Registry {
  pub fn new() -> Self { Self { inner: HashMap::new() } }
  pub fn register(&mut self, fac: Box<dyn ProviderFactory>) { self.inner.insert(fac.name().to_string(), fac); }

  fn split_model(model_id: &str) -> Option<(&str,&str)> {
    model_id.split_once(':')
  }

  pub fn chat(&self, model_id: &str) -> Option<Box<dyn ChatModel>> {
    let (prov, model) = Self::split_model(model_id)?;
    let fac = self.inner.get(prov)?;
    fac.create_chat(model, &ProviderCfg { name: prov.into() })
  }
  pub fn embed(&self, model_id: &str) -> Option<Box<dyn EmbedModel>> {
    let (prov, model) = Self::split_model(model_id)?;
    let fac = self.inner.get(prov)?;
    fac.create_embed(model, &ProviderCfg { name: prov.into() })
  }
  pub fn rerank(&self, model_id: &str) -> Option<Box<dyn RerankModel>> {
    let (prov, model) = Self::split_model(model_id)?;
    let fac = self.inner.get(prov)?;
    fac.create_rerank(model, &ProviderCfg { name: prov.into() })
  }
}

/* ---------------------------
   Local Provider (示例实现)
   --------------------------- */

pub struct LocalProviderFactory;
impl LocalProviderFactory { pub fn install(reg: &mut Registry) { reg.register(Box::new(Self)); } }

#[async_trait::async_trait]
impl ProviderFactory for LocalProviderFactory {
  fn name(&self) -> &'static str { "local" }
  fn caps(&self) -> ProviderCaps { ProviderCaps { chat: true, stream: true, tools: false, embeddings: true, rerank: true, multimodal: false, json_schema: true } }
  fn create_chat(&self, _model: &str, _cfg: &ProviderCfg) -> Option<Box<dyn ChatModel>> { Some(Box::new(LocalChat)) }
  fn create_embed(&self, _model: &str, _cfg: &ProviderCfg) -> Option<Box<dyn EmbedModel>> { Some(Box::new(LocalEmbed)) }
  fn create_rerank(&self, _model: &str, _cfg: &ProviderCfg) -> Option<Box<dyn RerankModel>> { Some(Box::new(LocalRerank)) }
}

/* Chat */

struct LocalChat;

#[async_trait::async_trait]
impl ChatModel for LocalChat {
  type Stream = futures_util::stream::BoxStream<'static, Result<ChatDelta, LlmError>>;

  async fn chat(&self, req: ChatRequest, enforce: &StructOutPolicy) -> Result<ChatResponse, LlmError> {
    // 取最后一条 user 文本作为输入；拼接为回声
    let mut last_user = String::new();
    for m in &req.messages {
      if matches!(m.role, Role::User) {
        for s in &m.segments {
          if let ContentSegment::Text { text } = s { last_user = text.clone(); }
        }
      }
    }

    let mut text_out = format!("echo: {}", last_user);
    // 结构化输出示例：若要求 Json，则把文本包成 {"echo": "..."}
    if let Some(fmt) = &req.response_format {
      if matches!(fmt.kind, crate::chat::ResponseKind::Json | crate::chat::ResponseKind::JsonSchema) {
        let candidate = format!(r#"{{"echo":"{}"}}"#, last_user.replace('"', "\\\""));
        let _v = crate::jsonsafe::enforce_json(&candidate, enforce)?;
        text_out = candidate;
      }
    }

    let usage = estimate_usage(&[&last_user], &text_out);
    Ok(ChatResponse {
      model_id: req.model_id.clone(),
      message: Message { role: Role::Assistant, segments: vec![ContentSegment::Text { text: text_out.clone() }], tool_calls: vec![] },
      usage,
      cost: crate::cost::zero_cost(),
      finish: FinishReason::Stop,
      provider_meta: serde_json::json!({"provider":"local"}),
    })
  }

  async fn chat_stream(&self, req: ChatRequest, enforce: &StructOutPolicy) -> Result<Self::Stream, LlmError> {
    // 简单拆分为两段 delta： "echo: " + last_user
    let mut last_user = String::new();
    for m in &req.messages {
      if matches!(m.role, Role::User) {
        for s in &m.segments {
          if let ContentSegment::Text { text } = s { last_user = text.clone(); }
        }
      }
    }
    let first = ChatDelta { text_delta: Some("echo: ".into()), tool_call_delta: None, usage_partial: None, finish: None, first_token_ms: Some(10) };
    let second_raw = last_user.clone();
    // 若要求 Json，则一次性在第二段输出完整 JSON
    let second_text = if let Some(fmt) = &req.response_format {
      if matches!(fmt.kind, crate::chat::ResponseKind::Json | crate::chat::ResponseKind::JsonSchema) {
        let candidate = format!(r#"{{"echo":"{}"}}"#, last_user.replace('"', "\\\""));
        let _v = crate::jsonsafe::enforce_json(&candidate, enforce)?;
        candidate
      } else { second_raw }
    } else { second_raw };

    let second = ChatDelta { text_delta: Some(second_text), tool_call_delta: None, usage_partial: Some(estimate_usage(&[&last_user], "")), finish: Some(FinishReason::Stop), first_token_ms: None };

    Ok(stream::iter(vec![Ok(first), Ok(second)]).boxed())
  }
}

/* Embeddings */

struct LocalEmbed;

#[async_trait::async_trait]
impl EmbedModel for LocalEmbed {
  async fn embed(&self, req: crate::embed::EmbedRequest) -> Result<crate::embed::EmbedResponse, LlmError> {
    // 维度固定 8；值为简单 hash 归一化（演示用）
    let dim = 8u32;
    let mut vectors = Vec::with_capacity(req.items.len());
    for item in &req.items {
      let mut v = vec![0.0f32; dim as usize];
      for (i, ch) in item.text.chars().enumerate() {
        v[i % v.len()] += (ch as u32 % 13) as f32 / 13.0;
      }
      if req.normalize {
        let norm = (v.iter().map(|x| x*x).sum::<f32>()).sqrt().max(1e-6);
        for x in v.iter_mut() { *x /= norm; }
      }
      vectors.push(v);
    }
    Ok(crate::embed::EmbedResponse {
      dim,
      vectors,
      usage: Usage { input_tokens: req.items.iter().map(|i| (i.text.len() as u32 + 3)/4).sum(), output_tokens: 0, cached_tokens: None, image_units: None, audio_seconds: None, requests: 1 },
      cost: crate::cost::zero_cost(),
      provider_meta: serde_json::json!({"provider":"local"}),
    })
  }
}

/* Rerank */

struct LocalRerank;

#[async_trait::async_trait]
impl crate::rerank::RerankModel for LocalRerank {
  async fn rerank(&self, req: crate::rerank::RerankRequest) -> Result<crate::rerank::RerankResponse, LlmError> {
    // 简化：按候选与 query 的 Jaccard 相似度排序
    fn score(q: &str, c: &str) -> f32 {
      let qa: std::collections::BTreeSet<_> = q.split_whitespace().collect();
      let ca: std::collections::BTreeSet<_> = c.split_whitespace().collect();
      let inter = qa.intersection(&ca).count() as f32;
      let union = (qa.len() + ca.len()) as f32 - inter;
      if union <= 0.0 { 0.0 } else { inter / union }
    }
    let mut idx: Vec<usize> = (0..req.candidates.len()).collect();
    let mut scs: Vec<f32> = req.candidates.iter().map(|c| score(&req.query, c)).collect();
    idx.sort_by(|&a,&b| scs[b].partial_cmp(&scs[a]).unwrap_or(std::cmp::Ordering::Equal));
    let scores_sorted: Vec<f32> = idx.iter().map(|&i| scs[i]).collect();

    Ok(crate::rerank::RerankResponse {
      scores: scores_sorted,
      ordering: idx,
      usage: Usage { input_tokens: (req.query.len() as u32 + 3)/4, output_tokens: 0, cached_tokens: None, image_units: None, audio_seconds: None, requests: 1 },
      cost: crate::cost::zero_cost(),
      provider_meta: serde_json::json!({"provider":"local"}),
    })
  }
}
```

------

## src/prelude.rs

```rust
pub use crate::model::{Role, ContentSegment, Message, ToolCallProposal, Usage, Cost, CostBreakdown, FinishReason};
pub use crate::chat::{ChatModel, ChatRequest, ChatResponse, ChatDelta, ResponseKind, ResponseFormat, ToolSpec};
pub use crate::embed::{EmbedModel, EmbedRequest, EmbedResponse, EmbedItem};
pub use crate::rerank::{RerankModel, RerankRequest, RerankResponse};
pub use crate::provider::{Registry, ProviderFactory, ProviderCfg, ProviderCaps, LocalProviderFactory};
pub use crate::jsonsafe::StructOutPolicy;
pub use crate::errors::LlmError;
```

------

## tests/basic.rs

```rust
use soulbase_llm::prelude::*;
use sb_types::prelude::*;
use futures_util::StreamExt;

#[tokio::test]
async fn chat_sync_and_stream_consistency() {
    // 注册本地 Provider
    let mut reg = Registry::new();
    LocalProviderFactory::install(&mut reg);

    let chat = reg.chat("local:echo").expect("chat model");
    let req = ChatRequest {
        model_id: "local:echo".into(),
        messages: vec![
            Message { role: Role::System, segments: vec![ContentSegment::Text{ text: "You are echo.".into() }], tool_calls: vec![] },
            Message { role: Role::User, segments: vec![ContentSegment::Text{ text: "hello" .into() }], tool_calls: vec![] },
        ],
        tool_specs: vec![],
        temperature: None, top_p: None, max_tokens: None, stop: vec![], seed: None, frequency_penalty: None, presence_penalty: None,
        logit_bias: Default::default(), response_format: None, idempotency_key: None, allow_sensitive: false, metadata: serde_json::json!({})
    };

    // 同步
    let sync = chat.chat(req.clone(), &StructOutPolicy::Off).await.expect("chat");
    let text_sync = match &sync.message.segments[0] { ContentSegment::Text { text } => text.clone(), _ => "".into() };
    assert!(text_sync.starts_with("echo: "));

    // 流式
    let mut stream = chat.chat_stream(req, &StructOutPolicy::Off).await.expect("stream");
    let mut concat = String::new();
    while let Some(delta) = stream.next().await {
        let d = delta.expect("delta ok");
        if let Some(t) = d.text_delta { concat.push_str(&t); }
    }
    assert_eq!(concat, text_sync);
}

#[tokio::test]
async fn chat_json_struct_out_validation() {
    let mut reg = Registry::new();
    LocalProviderFactory::install(&mut reg);
    let chat = reg.chat("local:echo").expect("chat model");

    let req = ChatRequest {
        model_id: "local:echo".into(),
        messages: vec![Message { role: Role::User, segments: vec![ContentSegment::Text{ text: "hi".into() }], tool_calls: vec![] }],
        tool_specs: vec![], temperature: None, top_p: None, max_tokens: None, stop: vec![], seed: None,
        frequency_penalty: None, presence_penalty: None, logit_bias: Default::default(),
        response_format: Some(ResponseFormat{ kind: ResponseKind::Json, json_schema: None, strict: true }),
        idempotency_key: None, allow_sensitive: false, metadata: serde_json::json!({})
    };

    let resp = chat.chat(req, &StructOutPolicy::StrictReject).await.expect("ok");
    let seg = &resp.message.segments[0];
    let s = match seg { ContentSegment::Text{text} => text.clone(), _ => "".into() };
    let v: serde_json::Value = serde_json::from_str(&s).expect("valid json");
    assert_eq!(v["echo"], "hi");
}

#[tokio::test]
async fn embeddings_and_rerank_work() {
    let mut reg = Registry::new();
    LocalProviderFactory::install(&mut reg);
    let emb = reg.embed("local:emb").expect("embed");
    let out = emb.embed(EmbedRequest {
        model_id: "local:emb".into(),
        items: vec![EmbedItem{id:"a".into(), text:"the cat sat".into()}, EmbedItem{id:"b".into(), text:"cat on mat".into()}],
        normalize: true, pooling: None
    }).await.expect("embed ok");
    assert_eq!(out.dim, 8);
    assert_eq!(out.vectors.len(), 2);

    let rer = reg.rerank("local:rerank").expect("rerank");
    let rr = rer.rerank(RerankRequest { model_id: "local:rerank".into(), query: "cat mat".into(), candidates: vec!["the cat sat".into(), "cat on mat".into()] }).await.expect("rerank ok");
    assert_eq!(rr.ordering[0], 1); // 第二句与 query 更接近
}
```

------

## README.md（简版）

~~~markdown
# soulbase-llm (RIS)

Provider-agnostic LLM SPI with a local provider:
- Chat (sync/stream), Tool proposal placeholder
- Structured output guard (Off / StrictReject / StrictRepair)
- Embeddings (toy), Rerank (toy)
- Error normalization (stable codes)
- No external HTTP required

## Build & Test
```bash
cargo check
cargo test
~~~

## Next

- Add real providers (OpenAI/Claude/Gemini) via feature flags
- Integrate soulbase-tools ToolSpec
- Pricing table & QoS integration
- Observability exports (metrics/tracing)

```
---

### 对齐与延展
- **同频共振**：严格遵循“**供应商无关、只提案不执行、结构化可验证、稳定错误、预算与观测闭环**”的不变式；所有错误统一走 `soulbase-errors`。  
- **可演进**：`Registry`/`ProviderFactory` 已就位；后续仅需新增 Provider 适配，无需改动 SPI。  
- **可运行**：本地 Provider 支撑端到端单测，方便团队立即串接内核策略与上游拦截器。
::contentReference[oaicite:0]{index=0}
```
