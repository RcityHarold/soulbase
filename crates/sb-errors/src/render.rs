use serde::Serialize;

use crate::{labels, model::ErrorObj};

#[derive(Debug, Serialize)]
pub struct PublicErrorView {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuditErrorView<'a> {
    pub code: &'static str,
    pub kind: &'static str,
    pub http_status: u16,
    pub retryable: &'static str,
    pub severity: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_dev: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause_chain: Option<&'a Vec<crate::model::CauseEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backoff_hint: Option<&'a crate::retry::BackoffHint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<&'a serde_json::Map<String, serde_json::Value>>,
}

impl ErrorObj {
    pub fn to_public(&self) -> PublicErrorView {
        PublicErrorView {
            code: self.code.0,
            message: self.message_user.clone(),
            correlation_id: self.correlation_id.clone(),
        }
    }

    pub fn to_audit(&self) -> AuditErrorView<'_> {
        AuditErrorView {
            code: self.code.0,
            kind: self.kind.as_str(),
            http_status: self.http_status,
            retryable: self.retryable.as_str(),
            severity: self.severity.as_str(),
            message_dev: self.message_dev.as_deref(),
            correlation_id: self.correlation_id.as_deref(),
            cause_chain: self.cause_chain.as_ref(),
            backoff_hint: self.backoff_hint.as_ref(),
            meta: if self.meta.is_empty() {
                None
            } else {
                Some(&self.meta)
            },
        }
    }
}

pub fn labels_view(err: &ErrorObj) -> std::collections::BTreeMap<&'static str, String> {
    labels::labels(err)
}
