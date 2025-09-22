use async_trait::async_trait;

use crate::{
    errors,
    model::{ConfigMap, Provenance},
};

use super::{merge_value, provenance_entry, Source, SourceSnapshot};

#[derive(Clone, Debug)]
pub struct CliArgsSource {
    pub args: Vec<String>,
}

#[async_trait]
impl Source for CliArgsSource {
    fn id(&self) -> &'static str {
        "cli"
    }

    async fn load(&self) -> Result<SourceSnapshot, errors::ConfigError> {
        let mut map = ConfigMap::new();
        for arg in &self.args {
            if let Some(stripped) = arg.strip_prefix("--") {
                if let Some((key, value)) = stripped.split_once('=') {
                    merge_value(&mut map, key, serde_json::Value::String(value.to_string()));
                }
            } else if let Some((key, value)) = arg.split_once('=') {
                merge_value(&mut map, key, serde_json::Value::String(value.to_string()));
            }
        }
        let mut provenance = Provenance::default();
        provenance.0.push(provenance_entry("cli", "cli"));
        Ok(SourceSnapshot { map, provenance })
    }
}
