use sb_errors::prelude::*;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("{public}")]
pub struct InterceptError {
    inner: ErrorObj,
    public: String,
}

impl InterceptError {
    pub fn new(inner: ErrorObj) -> Self {
        let view = inner.to_public();
        let public = format!("{}: {}", view.code, view.message);
        Self { inner, public }
    }

    pub fn from_public(code: ErrorCode, message: impl Into<String>) -> Self {
        let inner = ErrorBuilder::new(code).user_msg(message).build();
        Self::new(inner)
    }

    pub fn deny_policy(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::POLICY_DENY_TOOL)
                .dev_msg(detail)
                .build(),
        )
    }

    pub fn into_inner(self) -> ErrorObj {
        self.inner
    }

    pub fn inner(&self) -> &ErrorObj {
        &self.inner
    }
}

impl From<ErrorObj> for InterceptError {
    fn from(value: ErrorObj) -> Self {
        Self::new(value)
    }
}

impl From<sb_auth::prelude::AuthError> for InterceptError {
    fn from(err: sb_auth::prelude::AuthError) -> Self {
        Self::new(err.into_inner())
    }
}
