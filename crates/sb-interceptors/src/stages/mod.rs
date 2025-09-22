use crate::context::{InterceptContext, ProtoRequest, ProtoResponse};
use crate::errors::InterceptError;
use crate::idempotency::{oversized_body_error, StoredResponse};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::resilience::{execute_with_resilience, ResiliencePolicy};
use sb_errors::prelude::codes;
use serde_json;

#[async_trait]
pub trait Stage: Send + Sync {
    async fn handle(
        &self,
        cx: &mut InterceptContext,
        req: &mut dyn ProtoRequest,
        rsp: &mut dyn ProtoResponse,
    ) -> Result<StageOutcome, InterceptError>;
}

pub trait ResponseStage: Send + Sync {
    fn handle_response(
        &self,
        cx: &mut InterceptContext,
        rsp: &mut dyn ProtoResponse,
    ) -> Result<(), InterceptError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageOutcome {
    Continue,
    ShortCircuit,
}

pub struct InterceptorChain {
    request_stages: Vec<Box<dyn Stage>>,
    response_stages: Vec<Box<dyn ResponseStage>>,
    error_responder: Box<dyn ErrorResponder>,
    resilience: ResiliencePolicy,
    concurrency_limit: Option<Arc<Semaphore>>,
}

impl InterceptorChain {
    pub fn new(
        request_stages: Vec<Box<dyn Stage>>,
        response_stages: Vec<Box<dyn ResponseStage>>,
    ) -> Self {
        Self {
            request_stages,
            response_stages,
            error_responder: Box::new(DefaultErrorResponder::default()),
            resilience: ResiliencePolicy::default(),
            concurrency_limit: None,
        }
    }

    pub fn with_error_responder(mut self, responder: Box<dyn ErrorResponder>) -> Self {
        self.error_responder = responder;
        self
    }

    pub fn with_resilience(mut self, policy: ResiliencePolicy) -> Self {
        self.resilience = policy;
        self
    }

    pub fn with_concurrency_limit(mut self, limit: usize) -> Self {
        self.concurrency_limit = Some(Arc::new(Semaphore::new(limit)));
        self
    }

    pub async fn run_with_handler<F, Fut>(
        &self,
        mut cx: InterceptContext,
        req: &mut dyn ProtoRequest,
        rsp: &mut dyn ProtoResponse,
        mut handler: F,
    ) -> Result<(), InterceptError>
    where
        F: FnMut(&mut InterceptContext, &mut dyn ProtoRequest) -> Fut + Send,
        Fut: std::future::Future<Output = Result<serde_json::Value, InterceptError>> + Send,
    {
        let mut short_circuit = false;
        for stage in &self.request_stages {
            match stage.handle(&mut cx, req, rsp).await {
                Ok(StageOutcome::Continue) => {}
                Ok(StageOutcome::ShortCircuit) => {
                    short_circuit = true;
                    break;
                }
                Err(err) => {
                    self.error_responder.handle_error(&mut cx, err, rsp).await?;
                    return Ok(());
                }
            }
        }

        let _permit_guard = if !short_circuit {
            if let Some(sem) = &self.concurrency_limit {
                Some(sem.clone().acquire_owned().await.map_err(|_| {
                    InterceptError::from_public(codes::QUOTA_RATE_LIMITED, "服务当前并发已满。")
                })?)
            } else {
                None
            }
        } else {
            None
        };

        if !short_circuit && cx.response_body.is_none() {
            match execute_with_resilience(&mut handler, &mut cx, req, &self.resilience).await {
                Ok(body) => {
                    cx.response_body = Some(body);
                }
                Err(err) => {
                    self.error_responder.handle_error(&mut cx, err, rsp).await?;
                    return Ok(());
                }
            }
        }

        for stage in &self.response_stages {
            stage.handle_response(&mut cx, rsp)?;
        }

        let status = cx.response_status.unwrap_or(200);
        let headers_snapshot = cx.response_headers.clone();
        let body_snapshot = cx.response_body.clone();

        if let (Some(layer), Some(key)) = (cx.idempotency_layer.clone(), cx.idempotency_key.clone())
        {
            if !cx.idempotency_replay {
                if let Some(body_to_store) = body_snapshot.clone() {
                    let json_bytes = serde_json::to_vec(&body_to_store).map_err(|err| {
                        InterceptError::from_public(
                            codes::UNKNOWN_INTERNAL,
                            format!("无法序列化响应用于幂等缓存: {err}"),
                        )
                    })?;
                    if json_bytes.len() > layer.max_body_size() {
                        return Err(oversized_body_error(
                            json_bytes.len(),
                            layer.max_body_size(),
                        ));
                    }
                    let record = StoredResponse {
                        status,
                        body: body_to_store.clone(),
                        headers: headers_snapshot.clone(),
                    };
                    layer.store().put(key, record, layer.ttl()).await;
                }
            }
        }

        rsp.set_status(status);

        for (name, value) in &cx.response_headers {
            rsp.insert_header(name, value);
        }

        let body = body_snapshot.unwrap_or_else(|| serde_json::Value::Null);
        rsp.write_json(&body).await?;
        Ok(())
    }
}

#[async_trait]
pub trait ErrorResponder: Send + Sync {
    async fn handle_error(
        &self,
        cx: &mut InterceptContext,
        err: InterceptError,
        rsp: &mut dyn ProtoResponse,
    ) -> Result<(), InterceptError>;
}

pub struct DefaultErrorResponder;

impl Default for DefaultErrorResponder {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl ErrorResponder for DefaultErrorResponder {
    async fn handle_error(
        &self,
        cx: &mut InterceptContext,
        err: InterceptError,
        rsp: &mut dyn ProtoResponse,
    ) -> Result<(), InterceptError> {
        let inner = err.into_inner();
        let status = sb_errors::mapping_http::to_http_status(&inner);
        let public = inner.to_public();
        let (resource, action_str) = cx
            .route
            .as_ref()
            .map(|route| {
                (
                    Some(route.resource.0.as_str().to_string()),
                    Some(action_name(&route.action)),
                )
            })
            .unwrap_or((None, None));
        let labels =
            crate::observe::error_labels(&inner, resource.as_deref(), action_str.as_deref());
        if let Ok(value) = serde_json::to_value(&labels) {
            cx.extensions.insert("last_error_labels".to_string(), value);
        }
        cx.response_status = Some(status.as_u16());
        cx.response_body = Some(serde_json::to_value(&public).unwrap_or_else(|_| {
            serde_json::json!({
                "code": public.code,
                "message": public.message,
            })
        }));
        rsp.set_status(status.as_u16());
        rsp.insert_header("Content-Type", "application/json");
        rsp.write_json(cx.response_body.as_ref().unwrap()).await
    }
}

fn action_name(action: &sb_auth::prelude::Action) -> String {
    match action {
        sb_auth::prelude::Action::Read => "Read",
        sb_auth::prelude::Action::Write => "Write",
        sb_auth::prelude::Action::Invoke => "Invoke",
        sb_auth::prelude::Action::List => "List",
        sb_auth::prelude::Action::Admin => "Admin",
        sb_auth::prelude::Action::Configure => "Configure",
    }
    .to_string()
}

pub mod authn_map;
pub mod authz_quota;
pub mod context_init;
pub mod idempotency;
pub mod obligations;
pub mod response_stamp;
pub mod route_policy;
pub mod schema_guard;
