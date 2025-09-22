use serde::Serialize;
use serde_json::{Map, Value};

use crate::{
    code::{self, CodeSpec, ErrorCode},
    kind::ErrorKind,
    retry::{BackoffHint, RetryClass},
    severity::Severity,
};

#[derive(Clone, Debug, Serialize)]
pub struct CauseEntry {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ErrorObj {
    pub code: ErrorCode,
    pub kind: ErrorKind,
    pub http_status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grpc_status: Option<i32>,
    pub retryable: RetryClass,
    pub severity: Severity,
    pub message_user: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_dev: Option<String>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub meta: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause_chain: Option<Vec<CauseEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backoff_hint: Option<BackoffHint>,
}

impl ErrorObj {
    fn from_spec(spec: &CodeSpec) -> Self {
        Self {
            code: spec.code,
            kind: spec.kind,
            http_status: spec.http_status,
            grpc_status: spec.grpc_status,
            retryable: spec.retryable,
            severity: spec.severity,
            message_user: spec.default_user_msg.to_string(),
            message_dev: None,
            meta: Map::new(),
            cause_chain: None,
            correlation_id: None,
            backoff_hint: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ErrorBuilder {
    base: ErrorObj,
}

impl ErrorBuilder {
    pub fn new(code: ErrorCode) -> Self {
        let spec = code::spec_of(code)
            .or_else(|| code::spec_of(code::codes::UNKNOWN_INTERNAL))
            .expect("unknown code fallback must exist");
        Self {
            base: ErrorObj::from_spec(spec),
        }
    }

    pub fn user_msg(mut self, msg: impl Into<String>) -> Self {
        self.base.message_user = msg.into();
        self
    }

    pub fn dev_msg(mut self, msg: impl Into<String>) -> Self {
        self.base.message_dev = Some(msg.into());
        self
    }

    pub fn meta_kv(mut self, key: impl Into<String>, value: Value) -> Self {
        self.base.meta.insert(key.into(), value);
        self
    }

    pub fn meta_map(mut self, map: Map<String, Value>) -> Self {
        self.base.meta.extend(map);
        self
    }

    pub fn correlation(mut self, id: impl Into<String>) -> Self {
        self.base.correlation_id = Some(id.into());
        self
    }

    pub fn cause(mut self, entry: CauseEntry) -> Self {
        match &mut self.base.cause_chain {
            Some(chain) => chain.push(entry),
            None => self.base.cause_chain = Some(vec![entry]),
        }
        self
    }

    pub fn retryable(mut self, retryable: RetryClass) -> Self {
        self.base.retryable = retryable;
        self
    }

    pub fn severity(mut self, severity: Severity) -> Self {
        self.base.severity = severity;
        self
    }

    pub fn http_status(mut self, status: u16) -> Self {
        self.base.http_status = status;
        self
    }

    pub fn grpc_status(mut self, status: Option<i32>) -> Self {
        self.base.grpc_status = status;
        self
    }

    pub fn backoff_hint(mut self, hint: BackoffHint) -> Self {
        self.base.backoff_hint = Some(hint);
        self
    }

    pub fn build(self) -> ErrorObj {
        self.base
    }
}
