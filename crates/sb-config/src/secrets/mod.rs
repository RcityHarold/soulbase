use std::collections::HashMap;

use async_trait::async_trait;

use crate::{errors::ConfigError, model::ConfigMap};

#[async_trait]
pub trait SecretResolver: Send + Sync {
    async fn resolve(&self, _map: &mut ConfigMap) -> Result<(), ConfigError> {
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct NoopSecretResolver;

#[async_trait]
impl SecretResolver for NoopSecretResolver {}

pub fn mark_sensitive(_map: &mut HashMap<String, String>) {
    // 占位：预留敏感字段处理
}
