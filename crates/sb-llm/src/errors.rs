use sb_errors::prelude::{codes, ErrorBuilder, ErrorObj};
use thiserror::Error;

#[derive(Debug, Error)]
#[error("{public}")]
pub struct LlmError {
    inner: ErrorObj,
    public: String,
}

impl LlmError {
    pub fn new(inner: ErrorObj) -> Self {
        let view = inner.to_public();
        let public = format!("{}: {}", view.code, view.message);
        Self { inner, public }
    }

    pub fn into_inner(self) -> ErrorObj {
        self.inner
    }

    pub fn to_public(&self) -> sb_errors::render::PublicErrorView {
        self.inner.to_public()
    }

    pub fn provider_unavailable(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE)
                .user_msg("Model provider temporarily unavailable.")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::LLM_TIMEOUT)
                .user_msg("Model did not respond in time.")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn context_overflow(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::LLM_CONTEXT_OVERFLOW)
                .user_msg("Input exceeds the model context window.")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn safety_block(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::LLM_SAFETY_BLOCK)
                .user_msg("Model refused to answer due to safety policies.")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn schema(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::SCHEMA_VALIDATION_FAILED)
                .user_msg("Model output failed schema validation.")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn unknown(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::UNKNOWN_INTERNAL)
                .user_msg("Internal LLM error.")
                .dev_msg(msg.into())
                .build(),
        )
    }
}

impl From<ErrorObj> for LlmError {
    fn from(value: ErrorObj) -> Self {
        Self::new(value)
    }
}
