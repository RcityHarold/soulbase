use async_trait::async_trait;
use sb_types::prelude::{Id, TenantId};
use serde::{Deserialize, Serialize};

use crate::manifest::{SafetyClass, SideEffect, ToolId};
use crate::preflight::ToolOrigin;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolInvokeBegin {
    pub envelope_id: Id,
    pub tenant: TenantId,
    pub subject_id: Id,
    pub tool_id: ToolId,
    pub tool_version: String,
    pub call_id: Id,
    pub origin: ToolOrigin,
    pub safety: SafetyClass,
    pub side_effect: SideEffect,
    pub profile_hash: String,
    pub policy_hash: Option<String>,
    pub config_version: Option<String>,
    pub config_hash: Option<String>,
    pub args_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolInvokeEnd {
    pub envelope_id: Id,
    pub tenant: TenantId,
    pub subject_id: Id,
    pub tool_id: ToolId,
    pub tool_version: String,
    pub call_id: Id,
    pub origin: ToolOrigin,
    pub status: crate::invoker::InvokeStatus,
    pub error_code: Option<String>,
    pub profile_hash: String,
    pub policy_hash: Option<String>,
    pub config_version: Option<String>,
    pub config_hash: Option<String>,
    pub args_digest: String,
    pub output_digest: Option<String>,
    pub side_effects_digest: Option<String>,
    pub budget_calls: u64,
    pub budget_bytes_in: u64,
    pub budget_bytes_out: u64,
    pub budget_cpu_ms: u64,
    pub budget_gpu_ms: u64,
    pub budget_file_count: u64,
    pub duration_ms: i64,
}

#[async_trait]
pub trait ToolEventSink: Send + Sync {
    async fn on_invoke_begin(&self, _event: ToolInvokeBegin) {}
    async fn on_invoke_end(&self, _event: ToolInvokeEnd) {}
}

#[derive(Default)]
pub struct NoopToolEventSink;

#[async_trait]
impl ToolEventSink for NoopToolEventSink {}
