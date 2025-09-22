use crate::context::{ProtoRequest, ProtoResponse};
use crate::errors::InterceptError;
use std::collections::HashMap;

pub struct MessageRequest {
    pub topic: String,
    pub headers: HashMap<String, String>,
    pub payload: serde_json::Value,
}

#[async_trait::async_trait]
impl ProtoRequest for MessageRequest {
    fn method(&self) -> &str {
        "MSG"
    }

    fn path(&self) -> &str {
        &self.topic
    }

    fn header(&self, name: &str) -> Option<String> {
        self.headers.get(name).cloned()
    }

    async fn read_json(&mut self) -> Result<serde_json::Value, InterceptError> {
        Ok(self.payload.clone())
    }
}

pub struct MessageResponse {
    status: u16,
    pub headers: HashMap<String, String>,
    pub body: Option<serde_json::Value>,
}

impl MessageResponse {
    pub fn new() -> Self {
        Self {
            status: 200,
            headers: HashMap::new(),
            body: None,
        }
    }
}

#[async_trait::async_trait]
impl ProtoResponse for MessageResponse {
    fn set_status(&mut self, code: u16) {
        self.status = code;
    }

    fn insert_header(&mut self, name: &str, value: &str) {
        self.headers.insert(name.to_string(), value.to_string());
    }

    async fn write_json(&mut self, body: &serde_json::Value) -> Result<(), InterceptError> {
        self.body = Some(body.clone());
        Ok(())
    }
}
