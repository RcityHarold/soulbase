use crate::context::{ProtoRequest, ProtoResponse};
use crate::errors::InterceptError;
use std::convert::TryFrom;
use tonic::{Request, Response, Status};

pub struct GrpcJsonRequest {
    inner: Request<serde_json::Value>,
}

impl GrpcJsonRequest {
    pub fn new(inner: Request<serde_json::Value>) -> Self {
        Self { inner }
    }

    pub fn into_inner(self) -> Request<serde_json::Value> {
        self.inner
    }
}

#[async_trait::async_trait]
impl ProtoRequest for GrpcJsonRequest {
    fn method(&self) -> &str {
        self.inner.method().as_str()
    }

    fn path(&self) -> &str {
        self.inner.uri().path()
    }

    fn header(&self, name: &str) -> Option<String> {
        self.inner
            .metadata()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(|s| s.to_string())
    }

    async fn read_json(&mut self) -> Result<serde_json::Value, InterceptError> {
        Ok(self.inner.get_ref().clone())
    }
}

pub struct GrpcJsonResponse {
    metadata: tonic::metadata::MetadataMap,
    body: Option<serde_json::Value>,
}

impl GrpcJsonResponse {
    pub fn new() -> Self {
        Self {
            metadata: tonic::metadata::MetadataMap::new(),
            body: None,
        }
    }

    pub fn into_response(self) -> Result<Response<serde_json::Value>, Status> {
        let mut response = Response::new(self.body.unwrap_or_else(|| serde_json::Value::Null));
        *response.metadata_mut() = self.metadata;
        Ok(response)
    }
}

#[async_trait::async_trait]
impl ProtoResponse for GrpcJsonResponse {
    fn set_status(&mut self, code: u16) {
        if let Ok(value) = tonic::metadata::MetadataValue::from_str(&code.to_string()) {
            self.metadata.insert("http-status", value);
        }
    }

    fn insert_header(&mut self, name: &str, value: &str) {
        if let (Ok(key), Ok(val)) = (name.parse(), value.parse()) {
            self.metadata.insert(key, val);
        }
    }

    async fn write_json(&mut self, body: &serde_json::Value) -> Result<(), InterceptError> {
        self.body = Some(body.clone());
        Ok(())
    }
}
