use async_trait::async_trait;

use crate::{
    errors,
    model::{ConfigMap, Provenance},
};

use super::{merge_value, provenance_entry, Source, SourceSnapshot};

#[derive(Clone, Debug)]
pub struct EnvSource {
    pub prefix: String,
    pub separator: String,
}

#[async_trait]
impl Source for EnvSource {
    fn id(&self) -> &'static str {
        "env"
    }

    async fn load(&self) -> Result<SourceSnapshot, errors::ConfigError> {
        let mut map = ConfigMap::new();
        let mut provenance = Provenance::default();
        let prefix_upper = self.prefix.to_uppercase();
        for (key, value) in std::env::vars() {
            if !key.starts_with(&prefix_upper) {
                continue;
            }
            let trimmed = key
                .trim_start_matches(&prefix_upper)
                .trim_start_matches('_');
            if trimmed.is_empty() {
                continue;
            }
            let path = trimmed
                .split(&self.separator)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_lowercase())
                .collect::<Vec<_>>()
                .join(".");
            merge_value(&mut map, &path, serde_json::Value::String(value));
        }
        provenance.0.push(provenance_entry("env", "env"));
        Ok(SourceSnapshot { map, provenance })
    }
}
