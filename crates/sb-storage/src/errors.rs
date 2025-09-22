use sb_errors::prelude::{codes, ErrorBuilder, ErrorObj};
use thiserror::Error;

#[derive(Debug, Error)]
#[error("{public}")]
pub struct StorageError {
    inner: ErrorObj,
    public: String,
}

impl StorageError {
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
                .user_msg("storage provider unavailable")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::STORAGE_CONFLICT)
                .user_msg("storage conflict detected")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::STORAGE_NOT_FOUND)
                .user_msg("storage entry not found")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn schema(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::SCHEMA_VALIDATION_FAILED)
                .user_msg("storage schema validation failed")
                .dev_msg(msg.into())
                .build(),
        )
    }

    pub fn unknown(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::UNKNOWN_INTERNAL)
                .user_msg("internal storage error")
                .dev_msg(msg.into())
                .build(),
        )
    }
}

impl From<ErrorObj> for StorageError {
    fn from(value: ErrorObj) -> Self {
        Self::new(value)
    }
}

pub type StorageResult<T> = Result<T, StorageError>;
