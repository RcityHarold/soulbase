use std::collections::BTreeMap;

use crate::{model::ErrorObj, retry::RetryClass, severity::Severity};

pub fn labels(err: &ErrorObj) -> BTreeMap<&'static str, String> {
    let mut out = BTreeMap::new();
    out.insert("code", err.code.0.to_string());
    out.insert("kind", err.kind.as_str().to_string());
    out.insert(
        "retryable",
        match err.retryable {
            RetryClass::None => "none",
            RetryClass::Transient => "transient",
            RetryClass::Permanent => "permanent",
        }
        .to_string(),
    );
    out.insert(
        "severity",
        match err.severity {
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Error => "error",
            Severity::Critical => "critical",
        }
        .to_string(),
    );
    if let Some(v) = err.meta.get("provider") {
        out.insert("provider", v.to_string());
    }
    if let Some(v) = err.meta.get("tool") {
        out.insert("tool", v.to_string());
    }
    if let Some(v) = err.meta.get("tenant") {
        out.insert("tenant", v.to_string());
    }
    out
}
