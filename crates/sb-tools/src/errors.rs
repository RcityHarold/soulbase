use sb_errors::prelude::{codes, ErrorBuilder, ErrorObj};
use thiserror::Error;

#[derive(Debug, Error)]
#[error("{public}")]
pub struct ToolError {
    inner: ErrorObj,
    public: String,
}

impl ToolError {
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

    pub fn invalid_manifest(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::SCHEMA_VALIDATION_FAILED)
                .user_msg("tool manifest validation failed")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::POLICY_DENY_TOOL)
                .user_msg("tool is unavailable or disabled")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::AUTH_FORBIDDEN)
                .user_msg("request was rejected")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn sandbox_blocked(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::SANDBOX_CAPABILITY_BLOCKED)
                .user_msg("sandbox policy blocked tool execution")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn execution_failed(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::UNKNOWN_INTERNAL)
                .user_msg("tool execution failed")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn schema(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::SCHEMA_VALIDATION_FAILED)
                .user_msg("tool input or output violates schema")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn unknown(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::UNKNOWN_INTERNAL)
                .user_msg("internal tool subsystem error")
                .dev_msg(msg.into())
                .build(),
        )
    }
}

impl From<ErrorObj> for ToolError {
    fn from(value: ErrorObj) -> Self {
        Self::new(value)
    }
}

pub type ToolResult<T> = Result<T, ToolError>;
