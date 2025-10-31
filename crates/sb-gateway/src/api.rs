use crate::intercept::InterceptorFacade;
use crate::service::GatewayService;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{Response, StatusCode};
use axum::Json;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use tracing::error;

pub struct AppState {
    pub interceptor: InterceptorFacade,
    pub service: Arc<GatewayService>,
}

impl AppState {
    pub fn new(interceptor: InterceptorFacade, service: Arc<GatewayService>) -> Self {
        Self { interceptor, service }
    }
}

pub async fn healthz() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

pub async fn tools_execute_route(
    State(state): State<Arc<AppState>>,
    Path(path): Path<HashMap<String, String>>,
    req: axum::http::Request<Body>,
) -> Result<Response<Body>, Infallible> {
    let Some(tenant_id) = parse_tenant_id(&path) else {
        return Ok(bad_request_response("tenant_id 必须为数字"));
    };

    let state_clone = state.clone();
    match state.interceptor.execute(req, move |cx, proto_req| {
        let service = state_clone.service.clone();
        async move { service.clone().handle_tools_execute(tenant_id, cx, proto_req).await }
    }).await {
        Ok(resp) => Ok(resp),
        Err(err) => Ok(error_response(err)),
    }
}

pub async fn collab_execute_route(
    State(state): State<Arc<AppState>>,
    Path(path): Path<HashMap<String, String>>,
    req: axum::http::Request<Body>,
) -> Result<Response<Body>, Infallible> {
    let Some(tenant_id) = parse_tenant_id(&path) else {
        return Ok(bad_request_response("tenant_id 必须为数字"));
    };

    let state_clone = state.clone();
    match state.interceptor.execute(req, move |cx, proto_req| {
        let service = state_clone.service.clone();
        async move { service.clone().handle_collab_execute(tenant_id, cx, proto_req).await }
    }).await {
        Ok(resp) => Ok(resp),
        Err(err) => Ok(error_response(err)),
    }
}

fn parse_tenant_id(path: &HashMap<String, String>) -> Option<u64> {
    path.get("tenant_id")?.parse::<u64>().ok()
}

fn bad_request_response(msg: &str) -> Response<Body> {
    let payload = json!({
        "success": false,
        "error": {
            "code": "gateway.invalid_path",
            "message": msg
        }
    });
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap_or_default()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn error_response(err: sb_interceptors::errors::InterceptError) -> Response<Body> {
    let view = err.into_inner().to_public();
    error!("request failed: {} - {}", view.code, view.message);
    let payload = json!({
        "success": false,
        "error": {
            "code": view.code,
            "message": view.message,
            "correlation_id": view.correlation_id,
        }
    });
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap_or_default()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}
