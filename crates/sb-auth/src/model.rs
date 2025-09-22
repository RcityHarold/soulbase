use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::hash::{Hash, Hasher};

use sb_types::prelude::{Consent, Subject};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceUrn(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    Read,
    Write,
    Invoke,
    List,
    Admin,
    Configure,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuthzRequest {
    pub subject: Subject,
    pub resource: ResourceUrn,
    pub action: Action,
    pub attrs: Value,
    pub consent: Option<Consent>,
    pub correlation_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Obligation {
    pub kind: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub allow: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub obligations: Vec<Obligation>,
    #[serde(default)]
    pub evidence: Value,
    #[serde(default)]
    pub cache_ttl_ms: u32,
}

impl Decision {
    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            allow: false,
            reason: Some(reason.into()),
            obligations: Vec::new(),
            evidence: Value::Null,
            cache_ttl_ms: 0,
        }
    }

    pub fn allow_default() -> Self {
        Self {
            allow: true,
            reason: None,
            obligations: Vec::new(),
            evidence: Value::Null,
            cache_ttl_ms: 0,
        }
    }
}

#[derive(Clone, Debug, Eq)]
pub struct DecisionKey {
    pub tenant: String,
    pub subject_id: String,
    pub resource: ResourceUrn,
    pub action: Action,
    pub attrs_hash: u64,
}

impl PartialEq for DecisionKey {
    fn eq(&self, other: &Self) -> bool {
        self.tenant == other.tenant
            && self.subject_id == other.subject_id
            && self.resource == other.resource
            && self.action == other.action
            && self.attrs_hash == other.attrs_hash
    }
}

impl Hash for DecisionKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.tenant.hash(state);
        self.subject_id.hash(state);
        self.resource.hash(state);
        self.action.hash(state);
        self.attrs_hash.hash(state);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct QuotaKey {
    pub tenant: String,
    pub subject_id: String,
    pub resource: ResourceUrn,
    pub action: Action,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AttributeMap(pub Value);

impl AttributeMap {
    pub fn merged(&self, other: &Self) -> Self {
        merge_json(&self.0, &other.0)
    }
}

fn merge_json(lhs: &Value, rhs: &Value) -> AttributeMap {
    match (lhs, rhs) {
        (Value::Object(a), Value::Object(b)) => {
            let mut merged = a.clone();
            for (k, v) in b {
                let entry = merged.entry(k.clone()).or_insert(Value::Null);
                *entry = merge_json(entry, v).0;
            }
            AttributeMap(Value::Object(merged))
        }
        (_, Value::Null) => AttributeMap(lhs.clone()),
        (_, v) if lhs.is_null() => AttributeMap(v.clone()),
        (_, v) => AttributeMap(v.clone()),
    }
}

pub fn hash_attrs(attrs: &Value) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    if let Ok(bytes) = serde_json::to_vec(attrs) {
        hasher.write(&bytes);
    }
    hasher.finish()
}

impl From<Value> for AttributeMap {
    fn from(value: Value) -> Self {
        AttributeMap(value)
    }
}

impl AttributeMap {
    pub fn into_inner(self) -> Value {
        self.0
    }
}
