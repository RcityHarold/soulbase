use crate::errors::SandboxError;
use crate::evidence::EvidenceEvent;
use async_trait::async_trait;
use std::collections::BTreeMap;

#[async_trait]
pub trait EvidenceSink: Send + Sync {
    async fn emit(&self, event: EvidenceEvent);
}

#[derive(Default)]
pub struct NoopEvidenceSink;

#[async_trait]
impl EvidenceSink for NoopEvidenceSink {
    async fn emit(&self, _event: EvidenceEvent) {}
}

pub fn labels_from_error(
    err: &SandboxError,
    resource: &str,
    action: &str,
) -> BTreeMap<&'static str, String> {
    let mut labels = sb_errors::render::labels_view(err.inner());
    labels.insert("resource", resource.to_string());
    labels.insert("action", action.to_string());
    labels
}
