use crate::context::{EnvelopeSeed, InterceptContext, ProtoRequest, ProtoResponse};
use crate::errors::InterceptError;
use crate::stages::{Stage, StageOutcome};
use async_trait::async_trait;
use chrono::Utc;
use sb_config::prelude::ConfigSnapshot;
use sb_types::prelude::TraceContext;
use std::sync::Arc;
use uuid::Uuid;

pub struct ContextInitStage {
    config: Option<Arc<dyn ConfigSnapshotProvider>>,
}

impl ContextInitStage {
    pub fn new() -> Self {
        Self { config: None }
    }

    pub fn with_config_provider(mut self, provider: Arc<dyn ConfigSnapshotProvider>) -> Self {
        self.config = Some(provider);
        self
    }
}

impl Default for ContextInitStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Stage for ContextInitStage {
    async fn handle(
        &self,
        cx: &mut InterceptContext,
        req: &mut dyn ProtoRequest,
        _rsp: &mut dyn ProtoResponse,
    ) -> Result<StageOutcome, InterceptError> {
        let request_id = req
            .header("X-Request-Id")
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        cx.request_id = request_id;
        let trace_id = req.header("X-Trace-Id");
        let span_id = req.header("X-Span-Id");
        cx.trace = TraceContext::new(trace_id.clone(), span_id);
        cx.tenant_header = req.header("X-Soul-Tenant");
        cx.consent_token = req.header("X-Consent-Token");
        if let Some(provider) = &self.config {
            if let Some(snapshot) = provider.snapshot() {
                cx.config_version = Some(snapshot.metadata().version.0.clone());
                cx.config_checksum = Some(snapshot.checksum().0.clone());
            }
        }
        if cx.config_version.is_none() {
            cx.config_version = req.header("X-Config-Version");
        }
        if cx.config_checksum.is_none() {
            cx.config_checksum = req.header("X-Config-Checksum");
        }

        let partition = cx
            .tenant_header
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let first_segment = req
            .path()
            .trim_start_matches('/')
            .split('/')
            .next()
            .unwrap_or("-");
        cx.envelope_seed = EnvelopeSeed {
            correlation_id: req.header("X-Correlation-Id"),
            causation_id: req.header("X-Causation-Id"),
            partition_key: format!("{partition}:{first_segment}"),
            produced_at_ms: Utc::now().timestamp_millis(),
        };

        Ok(StageOutcome::Continue)
    }
}

pub trait ConfigSnapshotProvider: Send + Sync {
    fn snapshot(&self) -> Option<ConfigSnapshot>;
}

pub struct StaticSnapshotProvider {
    snapshot: ConfigSnapshot,
}

impl StaticSnapshotProvider {
    pub fn new(snapshot: ConfigSnapshot) -> Self {
        Self { snapshot }
    }
}

impl ConfigSnapshotProvider for StaticSnapshotProvider {
    fn snapshot(&self) -> Option<ConfigSnapshot> {
        Some(self.snapshot.clone())
    }
}
