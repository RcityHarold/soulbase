use async_trait::async_trait;
use sb_auth::prelude::{Action, AuthnInput, Obligation, ResourceUrn};
use sb_types::prelude::{Subject, TraceContext};
use serde_json::Map;

#[derive(Clone, Debug)]
pub struct InterceptContext {
    pub request_id: String,
    pub trace: TraceContext,
    pub tenant_header: Option<String>,
    pub consent_token: Option<String>,
    pub route: Option<RouteBinding>,
    pub subject: Option<Subject>,
    pub obligations: Vec<Obligation>,
    pub envelope_seed: EnvelopeSeed,
    pub config_version: Option<String>,
    pub config_checksum: Option<String>,
    pub auth_input: Option<AuthnInput>,
    pub response_body: Option<serde_json::Value>,
    pub response_status: Option<u16>,
    pub response_headers: Vec<(String, String)>,
    pub idempotency_replay: bool,
    pub idempotency_key: Option<String>,
    pub idempotency_layer: Option<crate::idempotency::IdempotencyLayer>,
    pub extensions: Map<String, serde_json::Value>,
}

impl InterceptContext {
    pub fn new() -> Self {
        Self {
            request_id: String::new(),
            trace: TraceContext::new(None, None),
            tenant_header: None,
            consent_token: None,
            route: None,
            subject: None,
            obligations: Vec::new(),
            envelope_seed: EnvelopeSeed::default(),
            config_version: None,
            config_checksum: None,
            auth_input: None,
            response_body: None,
            response_status: None,
            response_headers: Vec::new(),
            idempotency_replay: false,
            idempotency_key: None,
            idempotency_layer: None,
            extensions: Map::new(),
        }
    }

    pub fn set_response(&mut self, status: u16, body: serde_json::Value) {
        self.response_status = Some(status);
        self.response_body = Some(body);
    }

    pub fn ensure_response_body(&mut self) -> &mut serde_json::Value {
        if self.response_body.is_none() {
            self.response_body = Some(serde_json::Value::Null);
        }
        self.response_body.as_mut().unwrap()
    }
}

impl Default for InterceptContext {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Default)]
pub struct EnvelopeSeed {
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub partition_key: String,
    pub produced_at_ms: i64,
}

#[derive(Clone, Debug)]
pub struct RouteBinding {
    pub resource: ResourceUrn,
    pub action: Action,
    pub attrs: serde_json::Value,
    pub request_schema: Option<String>,
    pub response_schema: Option<String>,
}

#[async_trait]
pub trait ProtoRequest: Send {
    fn method(&self) -> &str;
    fn path(&self) -> &str;
    fn header(&self, name: &str) -> Option<String>;
    async fn read_json(&mut self) -> Result<serde_json::Value, crate::errors::InterceptError>;
}

#[async_trait]
pub trait ProtoResponse: Send {
    fn set_status(&mut self, code: u16);
    fn insert_header(&mut self, name: &str, value: &str);
    async fn write_json(
        &mut self,
        body: &serde_json::Value,
    ) -> Result<(), crate::errors::InterceptError>;
}
