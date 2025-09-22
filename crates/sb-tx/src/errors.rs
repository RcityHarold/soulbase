use sb_errors::prelude::{codes, ErrorBuilder, ErrorObj};
use thiserror::Error;

#[derive(Debug, Error)]
#[error("{public}")]
pub struct TxError {
    inner: ErrorObj,
    public: String,
}

impl TxError {
    pub fn new(inner: ErrorObj) -> Self {
        let public = {
            let view = inner.to_public();
            format!("{}: {}", view.code, view.message)
        };
        Self { inner, public }
    }

    pub fn into_inner(self) -> ErrorObj {
        self.inner
    }

    pub fn as_public(&self) -> sb_errors::render::PublicErrorView {
        self.inner.to_public()
    }

    pub fn provider_unavailable(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE)
                .user_msg("upstream is unavailable, please retry later")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE)
                .user_msg("operation timed out")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn idempo_busy() -> Self {
        Self::new(
            ErrorBuilder::new(codes::QUOTA_RATE_LIMITED)
                .user_msg("request is already being processed")
                .build(),
        )
    }

    pub fn idempo_failed() -> Self {
        Self::new(
            ErrorBuilder::new(codes::UNKNOWN_INTERNAL)
                .user_msg("previous attempt failed")
                .build(),
        )
    }

    pub fn schema(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::SCHEMA_VALIDATION_FAILED)
                .user_msg("invalid input payload")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::STORAGE_CONFLICT)
                .user_msg("conflict detected")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn budget_exhausted(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::QUOTA_BUDGET_EXCEEDED)
                .user_msg("budget exhausted for tenant")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn unknown(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::UNKNOWN_INTERNAL)
                .user_msg("internal transaction error")
                .dev_msg(msg.into())
                .build(),
        )
    }
}

impl From<ErrorObj> for TxError {
    fn from(value: ErrorObj) -> Self {
        Self::new(value)
    }
}

pub type TxResult<T> = Result<T, TxError>;
