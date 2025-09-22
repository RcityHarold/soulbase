use serde::{Deserialize, Serialize};
use serde_json::Map;
use serde_json::Value;

#[cfg(feature = "schema")]
use schemars::JsonSchema;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TraceContext {
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    #[serde(default)]
    pub baggage: Map<String, Value>,
}

impl TraceContext {
    pub fn new(trace_id: Option<String>, span_id: Option<String>) -> Self {
        Self {
            trace_id,
            span_id,
            baggage: Map::new(),
        }
    }
}
