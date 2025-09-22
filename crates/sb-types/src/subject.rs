use serde::{Deserialize, Serialize};
use serde_json::Map;
use serde_json::Value;

use crate::id::Id;
use crate::tenant::TenantId;

#[cfg(feature = "schema")]
use schemars::JsonSchema;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum SubjectKind {
    User,
    Service,
    Agent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Subject {
    pub kind: SubjectKind,
    pub subject_id: Id,
    pub tenant: TenantId,
    #[serde(default)]
    pub claims: Map<String, Value>,
}

impl Subject {
    pub fn new(kind: SubjectKind, subject_id: Id, tenant: TenantId) -> Self {
        Self {
            kind,
            subject_id,
            tenant,
            claims: Map::new(),
        }
    }
}
