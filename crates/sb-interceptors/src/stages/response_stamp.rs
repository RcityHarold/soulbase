use crate::context::{InterceptContext, ProtoResponse};
use crate::errors::InterceptError;
use crate::stages::ResponseStage;

pub struct ResponseStampStage;

impl ResponseStage for ResponseStampStage {
    fn handle_response(
        &self,
        cx: &mut InterceptContext,
        rsp: &mut dyn ProtoResponse,
    ) -> Result<(), InterceptError> {
        rsp.insert_header("X-Request-Id", &cx.request_id);
        if let Some(trace_id) = cx.trace.trace_id.clone() {
            rsp.insert_header("X-Trace-Id", &trace_id);
        }
        if let Some(version) = cx.config_version.clone() {
            rsp.insert_header("X-Config-Version", &version);
        }
        if let Some(checksum) = cx.config_checksum.clone() {
            rsp.insert_header("X-Config-Checksum", &checksum);
        }
        if !cx.obligations.is_empty() {
            let kinds: Vec<&str> = cx.obligations.iter().map(|o| o.kind.as_str()).collect();
            rsp.insert_header("X-Obligations", &kinds.join(","));
        }
        if cx.idempotency_replay {
            rsp.insert_header("X-Idempotent-Replay", "true");
        }
        Ok(())
    }
}
