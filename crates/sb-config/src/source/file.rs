use std::path::PathBuf;
use std::{fs, io};

use async_trait::async_trait;

use crate::{
    errors,
    model::{ConfigMap, Provenance},
};

use super::{merge_maps, provenance_entry, Source, SourceSnapshot};

#[derive(Clone, Debug, Default)]
pub struct FileSource {
    pub paths: Vec<PathBuf>,
}

#[async_trait]
impl Source for FileSource {
    fn id(&self) -> &'static str {
        "file"
    }

    async fn load(&self) -> Result<SourceSnapshot, errors::ConfigError> {
        let mut map = ConfigMap::new();
        let mut provenance = Provenance::default();

        for path in &self.paths {
            let content = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => {
                    return Err(errors::io_provider_unavailable("file", &format!("{}", err)))
                }
            };

            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
                if let serde_json::Value::Object(obj) = value {
                    merge_maps(&mut map, &obj);
                }
            }

            provenance
                .0
                .push(provenance_entry(path.to_string_lossy().as_ref(), "file"));
        }

        Ok(SourceSnapshot { map, provenance })
    }
}
