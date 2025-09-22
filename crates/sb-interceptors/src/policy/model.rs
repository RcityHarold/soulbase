use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RoutePolicySpec {
    pub when: MatchCond,
    pub bind: RouteBindingSpec,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind")]
pub enum MatchCond {
    #[serde(rename = "http")]
    Http { method: String, path_prefix: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RouteBindingSpec {
    pub resource: String,
    pub action: String,
    #[serde(default)]
    pub attrs_from_body: bool,
    #[serde(default)]
    pub request_schema: Option<String>,
    #[serde(default)]
    pub response_schema: Option<String>,
}
