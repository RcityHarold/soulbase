use async_trait::async_trait;
use std::sync::Arc;

use crate::errors::ConfigError;
use crate::snapshot::ConfigSnapshot;

#[derive(Clone, Debug)]
pub struct WatchEvent {
    pub snapshot: Arc<ConfigSnapshot>,
}

#[async_trait]
pub trait Watcher: Send + Sync {
    async fn run(&self) -> Result<(), ConfigError>;
}

#[derive(Default)]
pub struct NoopWatcher;

#[async_trait]
impl Watcher for NoopWatcher {
    async fn run(&self) -> Result<(), ConfigError> {
        Ok(())
    }
}
