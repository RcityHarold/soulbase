use std::collections::HashMap;

use once_cell::sync::Lazy;

use crate::{kind::ErrorKind, retry::RetryClass, severity::Severity};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ErrorCode(pub &'static str);

#[derive(Clone, Debug)]
pub struct CodeSpec {
    pub code: ErrorCode,
    pub kind: ErrorKind,
    pub http_status: u16,
    pub grpc_status: Option<i32>,
    pub retryable: RetryClass,
    pub severity: Severity,
    pub default_user_msg: &'static str,
}

impl CodeSpec {
    pub const fn new(
        code: ErrorCode,
        kind: ErrorKind,
        http_status: u16,
        grpc_status: Option<i32>,
        retryable: RetryClass,
        severity: Severity,
        default_user_msg: &'static str,
    ) -> Self {
        Self {
            code,
            kind,
            http_status,
            grpc_status,
            retryable,
            severity,
            default_user_msg,
        }
    }
}

pub mod codes {
    use super::ErrorCode;

    pub const AUTH_UNAUTHENTICATED: ErrorCode = ErrorCode("AUTH.UNAUTHENTICATED");
    pub const AUTH_FORBIDDEN: ErrorCode = ErrorCode("AUTH.FORBIDDEN");
    pub const SCHEMA_VALIDATION_FAILED: ErrorCode = ErrorCode("SCHEMA.VALIDATION_FAILED");
    pub const QUOTA_RATE_LIMITED: ErrorCode = ErrorCode("QUOTA.RATE_LIMITED");
    pub const QUOTA_BUDGET_EXCEEDED: ErrorCode = ErrorCode("QUOTA.BUDGET_EXCEEDED");
    pub const POLICY_DENY_TOOL: ErrorCode = ErrorCode("POLICY.DENY_TOOL");
    pub const SANDBOX_PERMISSION_DENIED: ErrorCode = ErrorCode("SANDBOX.PERMISSION_DENIED");
    pub const SANDBOX_CAPABILITY_BLOCKED: ErrorCode = ErrorCode("SANDBOX.CAPABILITY_BLOCKED");
    pub const LLM_TIMEOUT: ErrorCode = ErrorCode("LLM.TIMEOUT");
    pub const LLM_CONTEXT_OVERFLOW: ErrorCode = ErrorCode("LLM.CONTEXT_OVERFLOW");
    pub const LLM_SAFETY_BLOCK: ErrorCode = ErrorCode("LLM.SAFETY_BLOCK");
    pub const PROVIDER_UNAVAILABLE: ErrorCode = ErrorCode("PROVIDER.UNAVAILABLE");
    pub const TOOL_EXECUTION_ERROR: ErrorCode = ErrorCode("TOOL.EXECUTION_ERROR");
    pub const STORAGE_CONFLICT: ErrorCode = ErrorCode("STORAGE.CONFLICT");
    pub const STORAGE_NOT_FOUND: ErrorCode = ErrorCode("STORAGE.NOT_FOUND");
    pub const TX_TIMEOUT: ErrorCode = ErrorCode("TX.TIMEOUT");
    pub const TX_IDEMPOTENT_BUSY: ErrorCode = ErrorCode("TX.IDEMPOTENT_BUSY");
    pub const TX_IDEMPOTENT_LAST_FAILED: ErrorCode = ErrorCode("TX.IDEMPOTENT_LAST_FAILED");
    pub const A2A_REPLAY: ErrorCode = ErrorCode("A2A.REPLAY");
    pub const A2A_CONSENT_REQUIRED: ErrorCode = ErrorCode("A2A.CONSENT_REQUIRED");
    pub const A2A_LEDGER_MISMATCH: ErrorCode = ErrorCode("A2A.LEDGER_MISMATCH");
    pub const UNKNOWN_INTERNAL: ErrorCode = ErrorCode("UNKNOWN.INTERNAL");
}

pub static REGISTRY: Lazy<HashMap<&'static str, CodeSpec>> = Lazy::new(|| {
    use codes::*;

    let mut map = HashMap::new();

    let entries = [
        CodeSpec::new(
            AUTH_UNAUTHENTICATED,
            ErrorKind::Auth,
            401,
            Some(16), // UNAUTHENTICATED
            RetryClass::None,
            Severity::Warn,
            "请先登录。",
        ),
        CodeSpec::new(
            AUTH_FORBIDDEN,
            ErrorKind::Auth,
            403,
            Some(7), // PERMISSION_DENIED
            RetryClass::None,
            Severity::Warn,
            "没有访问该资源的权限。",
        ),
        CodeSpec::new(
            SCHEMA_VALIDATION_FAILED,
            ErrorKind::Schema,
            422,
            Some(3), // INVALID_ARGUMENT
            RetryClass::Permanent,
            Severity::Warn,
            "请求数据格式不符合要求。",
        ),
        CodeSpec::new(
            QUOTA_RATE_LIMITED,
            ErrorKind::RateLimit,
            429,
            Some(8), // RESOURCE_EXHAUSTED
            RetryClass::Transient,
            Severity::Warn,
            "请求过于频繁，请稍后重试。",
        ),
        CodeSpec::new(
            QUOTA_BUDGET_EXCEEDED,
            ErrorKind::QosBudgetExceeded,
            402,
            Some(8),
            RetryClass::Permanent,
            Severity::Warn,
            "额度已用尽，请调整预算配置。",
        ),
        CodeSpec::new(
            POLICY_DENY_TOOL,
            ErrorKind::PolicyDeny,
            403,
            Some(7),
            RetryClass::Permanent,
            Severity::Warn,
            "当前策略不允许执行该操作。",
        ),
        CodeSpec::new(
            SANDBOX_PERMISSION_DENIED,
            ErrorKind::Sandbox,
            403,
            Some(7),
            RetryClass::Permanent,
            Severity::Warn,
            "沙箱禁止了本次执行。",
        ),
        CodeSpec::new(
            SANDBOX_CAPABILITY_BLOCKED,
            ErrorKind::Sandbox,
            403,
            Some(7),
            RetryClass::None,
            Severity::Warn,
            "操作所需能力被沙箱限制。",
        ),
        CodeSpec::new(
            LLM_TIMEOUT,
            ErrorKind::LlmError,
            503,
            Some(4), // DEADLINE_EXCEEDED
            RetryClass::Transient,
            Severity::Error,
            "模型响应超时，请稍后重试。",
        ),
        CodeSpec::new(
            LLM_CONTEXT_OVERFLOW,
            ErrorKind::LlmError,
            400,
            Some(3), // INVALID_ARGUMENT
            RetryClass::Permanent,
            Severity::Warn,
            "输入超出模型上下文窗口，请调整后重试。",
        ),
        CodeSpec::new(
            LLM_SAFETY_BLOCK,
            ErrorKind::LlmError,
            403,
            Some(7), // PERMISSION_DENIED
            RetryClass::Permanent,
            Severity::Warn,
            "模型因安全策略拒绝了本次请求。",
        ),
        CodeSpec::new(
            PROVIDER_UNAVAILABLE,
            ErrorKind::Provider,
            503,
            Some(14), // UNAVAILABLE
            RetryClass::Transient,
            Severity::Error,
            "外部服务暂时不可用，请稍后重试。",
        ),
        CodeSpec::new(
            TOOL_EXECUTION_ERROR,
            ErrorKind::ToolError,
            500,
            Some(2), // UNKNOWN
            RetryClass::None,
            Severity::Error,
            "工具执行失败。",
        ),
        CodeSpec::new(
            STORAGE_CONFLICT,
            ErrorKind::Conflict,
            409,
            Some(6), // ALREADY_EXISTS
            RetryClass::Transient,
            Severity::Warn,
            "资源冲突，请重试。",
        ),
        CodeSpec::new(
            STORAGE_NOT_FOUND,
            ErrorKind::Storage,
            404,
            Some(5), // NOT_FOUND
            RetryClass::None,
            Severity::Warn,
            "未找到对应资源，请确认参数后重试。",
        ),
        CodeSpec::new(
            TX_TIMEOUT,
            ErrorKind::Timeout,
            504,
            Some(4), // DEADLINE_EXCEEDED
            RetryClass::Transient,
            Severity::Error,
            "事务处理超时，请稍后重试。",
        ),
        CodeSpec::new(
            TX_IDEMPOTENT_BUSY,
            ErrorKind::Conflict,
            409,
            Some(10), // ABORTED
            RetryClass::Transient,
            Severity::Warn,
            "请求仍在处理，请稍后重试。",
        ),
        CodeSpec::new(
            TX_IDEMPOTENT_LAST_FAILED,
            ErrorKind::Conflict,
            409,
            Some(10), // ABORTED
            RetryClass::None,
            Severity::Error,
            "上一次幂等操作失败，请人工确认。",
        ),
        CodeSpec::new(
            A2A_REPLAY,
            ErrorKind::A2AError,
            409,
            Some(10), // ABORTED
            RetryClass::None,
            Severity::Warn,
            "跨域请求被判定为重放。",
        ),
        CodeSpec::new(
            A2A_CONSENT_REQUIRED,
            ErrorKind::A2AError,
            428,
            Some(9), // FAILED_PRECONDITION
            RetryClass::Permanent,
            Severity::Warn,
            "缺少必要的互认同意，请补齐凭证。",
        ),
        CodeSpec::new(
            A2A_LEDGER_MISMATCH,
            ErrorKind::A2AError,
            409,
            Some(10), // ABORTED
            RetryClass::None,
            Severity::Error,
            "跨域账本校验不一致，请人工对账。",
        ),
        CodeSpec::new(
            UNKNOWN_INTERNAL,
            ErrorKind::Unknown,
            500,
            Some(13), // INTERNAL
            RetryClass::Transient,
            Severity::Error,
            "出现未知错误，请稍后重试。",
        ),
    ];

    for spec in entries {
        let key = spec.code.0;
        if map.insert(key, spec).is_some() {
            panic!("duplicate error code registered: {key}");
        }
    }

    map
});

pub fn spec_of(code: ErrorCode) -> Option<&'static CodeSpec> {
    REGISTRY.get(code.0)
}

pub fn iter_specs() -> impl Iterator<Item = &'static CodeSpec> {
    REGISTRY.values()
}
