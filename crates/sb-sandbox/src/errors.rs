use sb_errors::prelude::{codes, ErrorBuilder, ErrorObj};
use thiserror::Error;

#[derive(Debug, Error)]
#[error("{public}")]
pub struct SandboxError {
    inner: ErrorObj,
    public: String,
}

impl SandboxError {
    pub fn new(inner: ErrorObj) -> Self {
        let view = inner.to_public();
        let public = format!("{}: {}", view.code, view.message);
        Self { inner, public }
    }

    pub fn permission_denied(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::SANDBOX_PERMISSION_DENIED)
                .user_msg("操作被沙箱禁止。")
                .dev_msg(detail)
                .build(),
        )
    }

    pub fn policy_violation(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::POLICY_DENY_TOOL)
                .user_msg("未通过策略校验。")
                .dev_msg(detail)
                .build(),
        )
    }

    pub fn capability_missing(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::SANDBOX_CAPABILITY_BLOCKED)
                .user_msg("缺少执行所需能力。")
                .dev_msg(detail)
                .build(),
        )
    }

    pub fn forbidden(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorBuilder::new(codes::AUTH_FORBIDDEN)
                .user_msg("当前请求被拒绝。")
                .dev_msg(detail)
                .build(),
        )
    }

    pub fn into_inner(self) -> ErrorObj {
        self.inner
    }

    pub fn to_public(&self) -> sb_errors::render::PublicErrorView {
        self.inner.to_public()
    }

    pub fn inner(&self) -> &ErrorObj {
        &self.inner
    }
}

impl From<ErrorObj> for SandboxError {
    fn from(value: ErrorObj) -> Self {
        Self::new(value)
    }
}
