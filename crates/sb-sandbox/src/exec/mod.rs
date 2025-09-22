use crate::errors::SandboxError;
use crate::model::{CapabilityKind, Profile, SideEffectRecord};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod browser;
pub mod fs;
pub mod net;
pub mod proc_exec;
pub mod tmp;

pub trait CancelToken: Send + Sync {
    fn is_cancelled(&self) -> bool;
}

#[derive(Default)]
pub struct NoopCancelToken;

impl CancelToken for NoopCancelToken {
    fn is_cancelled(&self) -> bool {
        false
    }
}

pub struct ExecCtx<'a> {
    pub profile: &'a Profile,
    pub cancel: &'a dyn CancelToken,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ExecOp {
    FsRead {
        path: String,
        offset: Option<u64>,
        len: Option<u64>,
    },
    FsWrite {
        path: String,
        bytes_b64: String,
        overwrite: bool,
    },
    FsList {
        path: String,
    },
    NetHttp {
        method: String,
        url: String,
        headers: serde_json::Value,
        body_b64: Option<String>,
    },
    BrowserNav {
        url: String,
    },
    BrowserScreenshot {
        selector: Option<String>,
        full_page: bool,
    },
    ProcExec {
        tool: String,
        args: Vec<String>,
        timeout_ms: Option<u64>,
    },
    TmpAlloc {
        size_bytes: u64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ExecUsage {
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub calls: u64,
    pub cpu_ms: u64,
    pub file_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecResult {
    pub ok: bool,
    pub code: Option<String>,
    pub message: Option<String>,
    pub out: serde_json::Value,
    pub usage: ExecUsage,
    #[serde(default)]
    pub side_effects: Vec<SideEffectRecord>,
}

impl ExecResult {
    pub fn success(
        out: serde_json::Value,
        usage: ExecUsage,
        side_effects: Vec<SideEffectRecord>,
    ) -> Self {
        Self {
            ok: true,
            code: None,
            message: None,
            out,
            usage,
            side_effects,
        }
    }

    pub fn failure(code: String, message: Option<String>) -> Self {
        Self {
            ok: false,
            code: Some(code),
            message,
            out: serde_json::Value::Null,
            usage: ExecUsage::default(),
            side_effects: Vec::new(),
        }
    }
}

#[async_trait]
pub trait SandboxExecutor: Send + Sync {
    fn kind(&self) -> CapabilityKind;
    async fn execute(&self, ctx: &ExecCtx<'_>, op: ExecOp) -> Result<ExecResult, SandboxError>;
}

#[derive(Default)]
pub struct NotImplementedExecutor;

#[async_trait]
impl SandboxExecutor for NotImplementedExecutor {
    fn kind(&self) -> CapabilityKind {
        CapabilityKind::TmpUse
    }

    async fn execute(&self, _ctx: &ExecCtx<'_>, _op: ExecOp) -> Result<ExecResult, SandboxError> {
        Err(SandboxError::policy_violation("executor not implemented"))
    }
}
