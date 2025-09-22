use serde::{Deserialize, Serialize};
use serde_json::Map;
use serde_json::Value;

use crate::time::Timestamp;

#[cfg(feature = "schema")]
use schemars::JsonSchema;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Scope {
    pub resource: String,
    pub action: String,
    #[serde(default)]
    pub attrs: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Consent {
    #[serde(default)]
    pub scopes: Vec<Scope>,
    pub expires_at: Option<Timestamp>,
    #[serde(default)]
    pub purpose: Option<String>,
}

impl Scope {
    pub fn new(resource: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            resource: resource.into(),
            action: action.into(),
            attrs: Map::new(),
        }
    }
}

impl Consent {
    pub fn new(scopes: Vec<Scope>) -> Self {
        Self {
            scopes,
            expires_at: None,
            purpose: None,
        }
    }
}
