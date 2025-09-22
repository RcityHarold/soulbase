use async_trait::async_trait;

use crate::{
    errors::ConfigError,
    model::{ConfigMap, NamespaceId},
};

#[async_trait]
pub trait Validator: Send + Sync {
    async fn validate(
        &self,
        _namespace: &NamespaceId,
        _data: &ConfigMap,
    ) -> Result<(), ConfigError> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct BasicValidator;

#[async_trait]
impl Validator for BasicValidator {}
