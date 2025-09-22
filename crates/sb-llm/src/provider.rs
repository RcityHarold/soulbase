use std::collections::HashMap;

use crate::chat::{
    last_user_text, BoxChatModel, ChatDelta, ChatModel, ChatRequest, ChatResponse, ChatStream,
    ResponseKind,
};
use crate::cost::{estimate_usage, zero_cost};
use crate::embed::{EmbedModel, EmbedRequest, EmbedResponse};
use crate::errors::LlmError;
use crate::jsonsafe::{enforce_json, validate_against_schema, StructOutPolicy};
use crate::model::{ContentSegment, FinishReason, Message, Role, Usage};
use crate::rerank::{RerankModel, RerankRequest, RerankResponse};
use async_trait::async_trait;
use futures_util::{stream, StreamExt};
use serde_json::json;

pub struct ProviderCfg {
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct ProviderCaps {
    pub chat: bool,
    pub stream: bool,
    pub tools: bool,
    pub embeddings: bool,
    pub rerank: bool,
    pub multimodal: bool,
    pub json_schema: bool,
}

#[async_trait]
pub trait ProviderFactory: Send + Sync {
    fn name(&self) -> &'static str;
    fn caps(&self) -> ProviderCaps;
    fn create_chat(&self, _model: &str, _cfg: &ProviderCfg) -> Option<BoxChatModel> {
        None
    }
    fn create_embed(&self, _model: &str, _cfg: &ProviderCfg) -> Option<Box<dyn EmbedModel>> {
        None
    }
    fn create_rerank(&self, _model: &str, _cfg: &ProviderCfg) -> Option<Box<dyn RerankModel>> {
        None
    }
}

pub struct Registry {
    inner: HashMap<String, Box<dyn ProviderFactory>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    pub fn register(&mut self, factory: Box<dyn ProviderFactory>) {
        self.inner.insert(factory.name().to_string(), factory);
    }

    fn split_model(model_id: &str) -> Option<(&str, &str)> {
        model_id.split_once(':')
    }

    pub fn chat(&self, model_id: &str) -> Option<BoxChatModel> {
        let (provider, model) = Self::split_model(model_id)?;
        let factory = self.inner.get(provider)?;
        factory.create_chat(
            model,
            &ProviderCfg {
                name: provider.into(),
            },
        )
    }

    pub fn embed(&self, model_id: &str) -> Option<Box<dyn EmbedModel>> {
        let (provider, model) = Self::split_model(model_id)?;
        let factory = self.inner.get(provider)?;
        factory.create_embed(
            model,
            &ProviderCfg {
                name: provider.into(),
            },
        )
    }

    pub fn rerank(&self, model_id: &str) -> Option<Box<dyn RerankModel>> {
        let (provider, model) = Self::split_model(model_id)?;
        let factory = self.inner.get(provider)?;
        factory.create_rerank(
            model,
            &ProviderCfg {
                name: provider.into(),
            },
        )
    }
}

/* ------------------------------------------------------------------
 * Local provider implementation (purely in-memory, used for RIS/tests)
 * ------------------------------------------------------------------ */

pub struct LocalProviderFactory;

impl LocalProviderFactory {
    pub fn install(registry: &mut Registry) {
        registry.register(Box::new(Self));
    }
}

#[async_trait]
impl ProviderFactory for LocalProviderFactory {
    fn name(&self) -> &'static str {
        "local"
    }

    fn caps(&self) -> ProviderCaps {
        ProviderCaps {
            chat: true,
            stream: true,
            tools: false,
            embeddings: true,
            rerank: true,
            multimodal: false,
            json_schema: true,
        }
    }

    fn create_chat(&self, _model: &str, _cfg: &ProviderCfg) -> Option<BoxChatModel> {
        Some(Box::new(LocalChat) as BoxChatModel)
    }

    fn create_embed(&self, _model: &str, _cfg: &ProviderCfg) -> Option<Box<dyn EmbedModel>> {
        Some(Box::new(LocalEmbed))
    }

    fn create_rerank(&self, _model: &str, _cfg: &ProviderCfg) -> Option<Box<dyn RerankModel>> {
        Some(Box::new(LocalRerank))
    }
}

struct LocalChat;

#[async_trait]
impl ChatModel for LocalChat {
    type Stream = ChatStream;

    async fn chat(
        &self,
        req: ChatRequest,
        enforce: &StructOutPolicy,
    ) -> Result<ChatResponse, LlmError> {
        let last_user = last_user_text(&req.messages).unwrap_or_default();
        let mut text_out = format!("echo: {}", last_user);

        if let Some(format) = &req.response_format {
            if matches!(format.kind, ResponseKind::Json | ResponseKind::JsonSchema) {
                let json_candidate = json!({ "echo": last_user }).to_string();
                let value = enforce_json(&json_candidate, enforce)?;
                validate_against_schema(&value, &format.json_schema)?;
                text_out = json_candidate;
            }
        }

        let usage = estimate_usage(&[&last_user], &text_out);
        Ok(ChatResponse {
            model_id: req.model_id.clone(),
            message: Message {
                role: Role::Assistant,
                segments: vec![ContentSegment::Text {
                    text: text_out.clone(),
                }],
                tool_calls: Vec::new(),
            },
            usage,
            cost: zero_cost(),
            finish: FinishReason::Stop,
            provider_meta: json!({"provider": "local"}),
        })
    }

    async fn chat_stream(
        &self,
        req: ChatRequest,
        enforce: &StructOutPolicy,
    ) -> Result<Self::Stream, LlmError> {
        let last_user = last_user_text(&req.messages).unwrap_or_default();
        let intro = ChatDelta {
            text_delta: Some("echo: ".to_string()),
            tool_call_delta: None,
            usage_partial: None,
            finish: None,
            first_token_ms: Some(10),
        };

        let mut body_text = last_user.clone();
        if let Some(format) = &req.response_format {
            if matches!(format.kind, ResponseKind::Json | ResponseKind::JsonSchema) {
                let candidate = json!({ "echo": last_user }).to_string();
                let value = enforce_json(&candidate, enforce)?;
                validate_against_schema(&value, &format.json_schema)?;
                body_text = candidate;
            }
        }

        let body = ChatDelta {
            text_delta: Some(body_text),
            tool_call_delta: None,
            usage_partial: Some(estimate_usage(&[&last_user], "")),
            finish: Some(FinishReason::Stop),
            first_token_ms: None,
        };

        Ok(stream::iter(vec![Ok(intro), Ok(body)]).boxed())
    }
}

struct LocalEmbed;

#[async_trait]
impl EmbedModel for LocalEmbed {
    async fn embed(&self, req: EmbedRequest) -> Result<EmbedResponse, LlmError> {
        let dim = 8u32;
        let mut vectors = Vec::with_capacity(req.items.len());
        for item in &req.items {
            let mut vec = vec![0.0f32; dim as usize];
            let len = vec.len();
            for (idx, ch) in item.text.chars().enumerate() {
                let slot = idx % len;
                vec[slot] += (ch as u32 % 17) as f32 / 17.0;
            }
            if req.normalize {
                let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
                for v in vec.iter_mut() {
                    *v /= norm;
                }
            }
            vectors.push(vec);
        }

        Ok(EmbedResponse {
            dim,
            vectors,
            usage: Usage {
                input_tokens: req
                    .items
                    .iter()
                    .map(|item| ((item.text.len() as u32) + 3) / 4)
                    .sum(),
                output_tokens: 0,
                cached_tokens: None,
                image_units: None,
                audio_seconds: None,
                requests: 1,
            },
            cost: zero_cost(),
            provider_meta: json!({"provider": "local"}),
        })
    }
}

struct LocalRerank;

#[async_trait]
impl RerankModel for LocalRerank {
    async fn rerank(&self, req: RerankRequest) -> Result<RerankResponse, LlmError> {
        fn score(query: &str, cand: &str) -> f32 {
            let q: std::collections::BTreeSet<_> = query.split_whitespace().collect();
            let c: std::collections::BTreeSet<_> = cand.split_whitespace().collect();
            let inter = q.intersection(&c).count() as f32;
            let union = (q.len() + c.len()) as f32 - inter;
            if union <= 0.0 {
                0.0
            } else {
                inter / union
            }
        }

        let mut indices: Vec<usize> = (0..req.candidates.len()).collect();
        let scores: Vec<f32> = req
            .candidates
            .iter()
            .map(|cand| score(&req.query, cand))
            .collect();
        indices.sort_by(|&a, &b| {
            scores[b]
                .partial_cmp(&scores[a])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let sorted_scores = indices.iter().map(|&idx| scores[idx]).collect();

        Ok(RerankResponse {
            scores: sorted_scores,
            ordering: indices,
            usage: Usage {
                input_tokens: ((req.query.len() as u32) + 3) / 4,
                output_tokens: 0,
                cached_tokens: None,
                image_units: None,
                audio_seconds: None,
                requests: 1,
            },
            cost: zero_cost(),
            provider_meta: json!({"provider": "local"}),
        })
    }
}
