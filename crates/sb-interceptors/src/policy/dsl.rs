use super::model::{MatchCond, RoutePolicySpec};

pub struct RoutePolicy {
    rules: Vec<RoutePolicySpec>,
}

impl RoutePolicy {
    pub fn new(rules: Vec<RoutePolicySpec>) -> Self {
        Self { rules }
    }

    pub fn from_slice(data: &[u8]) -> Result<Self, serde_json::Error> {
        let specs: Vec<RoutePolicySpec> = serde_json::from_slice(data)?;
        Ok(Self::new(specs))
    }

    pub fn match_http(&self, method: &str, path: &str) -> Option<&RoutePolicySpec> {
        self.rules.iter().find(|spec| match &spec.when {
            MatchCond::Http {
                method: m,
                path_prefix,
            } => m.eq_ignore_ascii_case(method) && path.starts_with(path_prefix),
        })
    }
}

impl Default for RoutePolicy {
    fn default() -> Self {
        Self { rules: Vec::new() }
    }
}
