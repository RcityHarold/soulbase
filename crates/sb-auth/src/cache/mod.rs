use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::model::{Decision, DecisionKey};

#[async_trait]
pub trait DecisionCache: Send + Sync {
    async fn get(&self, key: &DecisionKey) -> Option<Decision>;
    async fn put(&self, key: DecisionKey, decision: &Decision);
    async fn invalidate(&self, key: &DecisionKey);
}

#[derive(Clone, Default)]
pub struct MemoryDecisionCache {
    inner: Arc<Mutex<HashMap<DecisionKey, (Decision, Instant, u32)>>>,
}

#[async_trait]
impl DecisionCache for MemoryDecisionCache {
    async fn get(&self, key: &DecisionKey) -> Option<Decision> {
        let mut guard = self.inner.lock();
        if let Some((decision, instant, ttl)) = guard.get(key) {
            if *ttl == 0 {
                return Some(decision.clone());
            }
            if instant.elapsed() <= Duration::from_millis(*ttl as u64) {
                return Some(decision.clone());
            }
        }
        guard.remove(key);
        None
    }

    async fn put(&self, key: DecisionKey, decision: &Decision) {
        let ttl = decision.cache_ttl_ms;
        self.inner
            .lock()
            .insert(key, (decision.clone(), Instant::now(), ttl));
    }

    async fn invalidate(&self, key: &DecisionKey) {
        self.inner.lock().remove(key);
    }
}
