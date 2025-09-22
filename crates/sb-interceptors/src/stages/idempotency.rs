use crate::context::{InterceptContext, ProtoRequest, ProtoResponse};
use crate::errors::InterceptError;
use crate::idempotency::{build_idempotency_key, idempotency_error, IdempotencyLayer};
use crate::stages::{Stage, StageOutcome};
use async_trait::async_trait;

const IDEMPOTENT_METHODS: &[&str] = &["POST", "PUT", "PATCH", "DELETE"];

#[derive(Clone)]
pub struct IdempotencyStage {
    layer: Option<IdempotencyLayer>,
}

impl IdempotencyStage {
    pub fn new(layer: Option<IdempotencyLayer>) -> Self {
        Self { layer }
    }
}

impl Default for IdempotencyStage {
    fn default() -> Self {
        Self { layer: None }
    }
}

#[async_trait]
impl Stage for IdempotencyStage {
    async fn handle(
        &self,
        cx: &mut InterceptContext,
        req: &mut dyn ProtoRequest,
        _rsp: &mut dyn ProtoResponse,
    ) -> Result<StageOutcome, InterceptError> {
        let Some(layer) = &self.layer else {
            cx.idempotency_layer = None;
            return Ok(StageOutcome::Continue);
        };
        let method = req.method().to_uppercase();
        if !IDEMPOTENT_METHODS.contains(&method.as_str()) {
            cx.idempotency_layer = None;
            return Ok(StageOutcome::Continue);
        }
        let Some(raw_key) = req.header("Idempotency-Key") else {
            cx.idempotency_layer = None;
            return Ok(StageOutcome::Continue);
        };
        if raw_key.trim().is_empty() {
            return Err(idempotency_error("Idempotency-Key is empty"));
        }
        let key = build_idempotency_key(cx, raw_key.trim(), &method, req.path());
        cx.idempotency_layer = Some(layer.clone());
        let store = layer.store();
        if let Some(existing) = store.get(&key).await {
            cx.idempotency_replay = true;
            cx.idempotency_key = Some(key);
            cx.response_status = Some(existing.status);
            cx.response_body = Some(existing.body.clone());
            cx.response_headers.extend(existing.headers.clone());
            return Ok(StageOutcome::ShortCircuit);
        }

        cx.idempotency_key = Some(key);
        Ok(StageOutcome::Continue)
    }
}
