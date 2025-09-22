use crate::invoker::InvokeStatus;
use crate::manifest::ToolId;
use crate::preflight::ToolOrigin;
use sb_types::prelude::TenantId;
use std::time::Duration;

pub trait ToolMetrics: Send + Sync {
    fn record_preflight(
        &self,
        _tenant: &TenantId,
        _tool: &ToolId,
        _origin: ToolOrigin,
        _allow: bool,
        _code: Option<&str>,
    ) {
    }
    fn record_invocation(
        &self,
        _tenant: &TenantId,
        _tool: &ToolId,
        _origin: ToolOrigin,
        _status: InvokeStatus,
        _code: Option<&str>,
        _duration: Duration,
    ) {
    }
    fn record_budget(
        &self,
        _tenant: &TenantId,
        _tool: &ToolId,
        _origin: ToolOrigin,
        _bytes_in: u64,
        _bytes_out: u64,
    ) {
    }
}

#[derive(Default)]
pub struct NoopToolMetrics;

impl ToolMetrics for NoopToolMetrics {}
