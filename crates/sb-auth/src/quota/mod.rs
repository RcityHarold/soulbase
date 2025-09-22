use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use crate::errors::AuthError;
use crate::model::QuotaKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaOutcome {
    Allowed,
    RateLimited,
    BudgetExceeded,
}

#[async_trait]
pub trait QuotaStore: Send + Sync {
    async fn check_and_consume(&self, key: &QuotaKey, cost: i64)
        -> Result<QuotaOutcome, AuthError>;
}

#[derive(Clone, Default)]
pub struct MemoryQuotaStore {
    limits: Arc<Mutex<HashMap<QuotaKey, (i64, i64)>>>, // (limit, used)
}

impl MemoryQuotaStore {
    pub fn with_limits(limits: HashMap<QuotaKey, i64>) -> Self {
        let inner = limits
            .into_iter()
            .map(|(k, limit)| (k, (limit, 0)))
            .collect();
        Self {
            limits: Arc::new(Mutex::new(inner)),
        }
    }
}

#[async_trait]
impl QuotaStore for MemoryQuotaStore {
    async fn check_and_consume(
        &self,
        key: &QuotaKey,
        cost: i64,
    ) -> Result<QuotaOutcome, AuthError> {
        let mut guard = self.limits.lock();
        let entry = guard.entry(key.clone()).or_insert((i64::MAX, 0));
        let limit = entry.0;
        let used = &mut entry.1;
        if *used >= limit {
            return Ok(QuotaOutcome::BudgetExceeded);
        }
        if *used + cost > limit {
            return Ok(QuotaOutcome::RateLimited);
        }
        *used += cost;
        Ok(QuotaOutcome::Allowed)
    }
}
