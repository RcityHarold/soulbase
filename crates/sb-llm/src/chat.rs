use crate::errors::LlmError;
use crate::jsonsafe::StructOutPolicy;
use crate::model::{ContentSegment, FinishReason, Message, ToolCallProposal, Usage};
use async_trait::async_trait;
use futures_core::Stream;
use futures_util::stream::BoxStream;
use serde::{Deserialize, Serialize};

#[cfg(feature = "schema-json")]
pub type JsonSchema = schemars::schema::RootSchema;
#[cfg(not(feature = "schema-json"))]
pub type JsonSchema = serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResponseKind {
    Text,
    Json,
    JsonSchema,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ResponseFormat {
    pub kind: ResponseKind,
    #[serde(default)]
    pub json_schema: Option<JsonSchema>,
    #[serde(default)]
    pub strict: bool,
}

/// 轻量 ToolSpec，占位，后续替换为  提供的类型。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    #[serde(default)]
    pub input_schema: Option<JsonSchema>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
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

#[async_trait]
pub trait ChatModel: Send + Sync {
    type Stream: Stream<Item = Result<ChatDelta, LlmError>> + Unpin + Send + 'static;

    async fn chat(
        &self,
        req: ChatRequest,
        enforce: &StructOutPolicy,
    ) -> Result<ChatResponse, LlmError>;

    async fn chat_stream(
        &self,
        req: ChatRequest,
        enforce: &StructOutPolicy,
    ) -> Result<Self::Stream, LlmError>;
}

pub type ChatStream = BoxStream<'static, Result<ChatDelta, LlmError>>;
pub type BoxChatModel = Box<dyn ChatModel<Stream = ChatStream>>;

/// Convenience helper to extract last user text content.
pub(crate) fn last_user_text(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, crate::model::Role::User))
        .and_then(|m| {
            m.segments.iter().rev().find_map(|seg| match seg {
                ContentSegment::Text { text } => Some(text.clone()),
                _ => None,
            })
        })
}
