use axum::body::{self, Body};
use axum::http::{HeaderValue, Request, Response, StatusCode};
use std::convert::TryFrom;

use crate::context::{ProtoRequest, ProtoResponse};
use crate::errors::InterceptError;

pub struct AxumRequest {
    inner: Request<Body>,
    cached_json: Option<serde_json::Value>,
}

impl AxumRequest {
    pub fn new(inner: Request<Body>) -> Self {
        Self {
            inner,
            cached_json: None,
        }
    }

    pub fn into_inner(self) -> Request<Body> {
        self.inner
    }
}

#[async_trait::async_trait]
impl ProtoRequest for AxumRequest {
    fn method(&self) -> &str {
        self.inner.method().as_str()
    }

    fn path(&self) -> &str {
        self.inner.uri().path()
    }

    fn header(&self, name: &str) -> Option<String> {
        self.inner
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }

    async fn read_json(&mut self) -> Result<serde_json::Value, InterceptError> {
        if let Some(value) = self.cached_json.clone() {
            return Ok(value);
        }
        let bytes = body::to_bytes(self.inner.body_mut()).await.map_err(|_| {
            InterceptError::from_public(
                sb_errors::prelude::codes::SCHEMA_VALIDATION_FAILED,
                "无法读取请求体。",
            )
        })?;
        if bytes.is_empty() {
            let value = serde_json::Value::Null;
            self.cached_json = Some(value.clone());
            return Ok(value);
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| {
            InterceptError::from_public(
                sb_errors::prelude::codes::SCHEMA_VALIDATION_FAILED,
                "请求体不是 JSON。",
            )
        })?;
        self.cached_json = Some(value.clone());
        Ok(value)
    }
}

pub struct AxumResponse {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Option<serde_json::Value>,
}

impl AxumResponse {
    pub fn new() -> Self {
        Self {
            status: StatusCode::OK,
            headers: axum::http::HeaderMap::new(),
            body: None,
        }
    }

    pub fn into_response(self) -> Response<Body> {
        let mut builder = Response::builder().status(self.status);
        {
            let headers = builder.headers_mut().expect("headers");
            for (key, value) in self.headers.iter() {
                headers.insert(key, value.clone());
            }
            if self.body.is_some() {
                headers.insert(
                    axum::http::header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
            }
        }
        let body = match self.body {
            Some(json) => {
                let bytes = serde_json::to_vec(&json).unwrap_or_default();
                Body::from(bytes)
            }
            None => Body::empty(),
        };
        builder
            .body(body)
            .unwrap_or_else(|_| Response::new(Body::empty()))
    }
}

#[async_trait::async_trait]
impl ProtoResponse for AxumResponse {
    fn set_status(&mut self, code: u16) {
        if let Ok(status) = StatusCode::try_from(code) {
            self.status = status;
        }
    }

    fn insert_header(&mut self, name: &str, value: &str) {
        if let Ok(header_name) = axum::http::HeaderName::try_from(name) {
            if let Ok(header_value) = HeaderValue::from_str(value) {
                self.headers.insert(header_name, header_value);
            }
        }
    }

    async fn write_json(&mut self, body: &serde_json::Value) -> Result<(), InterceptError> {
        self.body = Some(body.clone());
        Ok(())
    }
}
