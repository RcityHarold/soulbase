use sb_errors::prelude::*;
use std::fmt;

#[derive(Debug)]
pub struct AuthError(pub ErrorObj);

impl AuthError {
    pub fn builder(code: ErrorCode) -> ErrorBuilder {
        ErrorBuilder::new(code)
    }

    pub fn into_inner(self) -> ErrorObj {
        self.0
    }

    pub fn unauthenticated(detail: impl Into<String>) -> Self {
        Self(
            ErrorBuilder::new(codes::AUTH_UNAUTHENTICATED)
                .user_msg("Authentication required.")
                .dev_msg(detail)
                .build(),
        )
    }

    pub fn forbidden(detail: impl Into<String>) -> Self {
        Self(
            ErrorBuilder::new(codes::AUTH_FORBIDDEN)
                .user_msg("Access forbidden.")
                .dev_msg(detail)
                .build(),
        )
    }

    pub fn rate_limited() -> Self {
        Self(
            ErrorBuilder::new(codes::QUOTA_RATE_LIMITED)
                .user_msg("Rate limit exceeded.")
                .build(),
        )
    }

    pub fn budget_exceeded() -> Self {
        Self(
            ErrorBuilder::new(codes::QUOTA_BUDGET_EXCEEDED)
                .user_msg("Budget exceeded.")
                .build(),
        )
    }

    pub fn policy_deny(reason: impl Into<String>) -> Self {
        Self(
            ErrorBuilder::new(codes::POLICY_DENY_TOOL)
                .user_msg("Request denied by policy.")
                .dev_msg(reason)
                .build(),
        )
    }
}

impl From<ErrorObj> for AuthError {
    fn from(value: ErrorObj) -> Self {
        AuthError(value)
    }
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let view = self.0.to_public();
        write!(f, "{}: {}", view.code, view.message)
    }
}

impl std::error::Error for AuthError {}
