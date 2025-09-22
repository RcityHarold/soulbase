use std::collections::HashMap;

use parking_lot::Mutex;
use sb_types::prelude::TenantId;

use crate::config::BudgetConfig;
use crate::errors::TxResult;
use crate::model::OutboxMessage;
use crate::util::now_ms;

pub trait BudgetGuard: Send + Sync {
    fn on_enqueue(&self, _message: &OutboxMessage) -> TxResult<()> {
        Ok(())
    }

    fn on_dispatch_attempt(&self, _tenant: &TenantId, _message: &OutboxMessage) -> TxResult<()> {
        Ok(())
    }

    fn on_dispatch_result(
        &self,
        _tenant: &TenantId,
        _message: &OutboxMessage,
        _success: bool,
    ) -> TxResult<()> {
        Ok(())
    }

    fn on_dead_letter(
        &self,
        _tenant: &TenantId,
        _message: &OutboxMessage,
        _code: Option<&str>,
    ) -> TxResult<()> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct NoopBudgetGuard;

impl BudgetGuard for NoopBudgetGuard {}

#[derive(Debug, Clone)]
struct TenantBudget {
    inflight: u32,
    window_start_ms: i64,
    window_count: u32,
}

#[derive(Debug)]
pub struct SimpleBudgetGuard {
    cfg: BudgetConfig,
    state: Mutex<HashMap<String, TenantBudget>>,
}

impl SimpleBudgetGuard {
    pub fn new(cfg: BudgetConfig) -> Self {
        Self {
            cfg,
            state: Mutex::new(HashMap::new()),
        }
    }

    fn with_tenant<F, R>(&self, tenant: &TenantId, f: F) -> R
    where
        F: FnOnce(&mut TenantBudget) -> R,
    {
        let mut guard = self.state.lock();
        let entry = guard
            .entry(tenant.as_str().to_owned())
            .or_insert(TenantBudget {
                inflight: 0,
                window_start_ms: 0,
                window_count: 0,
            });
        f(entry)
    }
}

impl BudgetGuard for SimpleBudgetGuard {
    fn on_dispatch_attempt(&self, tenant: &TenantId, _message: &OutboxMessage) -> TxResult<()> {
        let mut exceeded = None;
        let cfg = self.cfg.clone();
        let now = now_ms();
        self.with_tenant(tenant, |budget| {
            if let Some(max) = cfg.max_inflight {
                if budget.inflight >= max {
                    exceeded = Some(format!("tenant {} exceeds max inflight {}", tenant, max));
                    return;
                }
            }
            if let (Some(limit), Some(window_sec)) =
                (cfg.max_dispatch_per_window, cfg.window_seconds)
            {
                let window_ms = (window_sec as i64).max(1) * 1_000;
                if now - budget.window_start_ms >= window_ms {
                    budget.window_start_ms = now;
                    budget.window_count = 0;
                }
                if budget.window_count >= limit {
                    exceeded = Some(format!(
                        "tenant {} exceeds dispatch limit {}/{}s",
                        tenant, limit, window_sec
                    ));
                    return;
                }
                budget.window_count += 1;
            }
            budget.inflight += 1;
        });

        if let Some(msg) = exceeded {
            Err(crate::errors::TxError::budget_exhausted(msg))
        } else {
            Ok(())
        }
    }

    fn on_dispatch_result(
        &self,
        tenant: &TenantId,
        _message: &OutboxMessage,
        _success: bool,
    ) -> TxResult<()> {
        self.with_tenant(tenant, |budget| {
            if budget.inflight > 0 {
                budget.inflight -= 1;
            }
        });
        Ok(())
    }
}

pub fn build_budget_guard(cfg: &BudgetConfig) -> std::sync::Arc<dyn BudgetGuard> {
    if cfg.max_inflight.is_none() && cfg.max_dispatch_per_window.is_none() {
        std::sync::Arc::new(NoopBudgetGuard)
    } else {
        std::sync::Arc::new(SimpleBudgetGuard::new(cfg.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NewOutboxMessage;
    use crate::outbox::build_outbox_message;
    use sb_types::prelude::{Id, TenantId};
    use serde_json::json;

    fn sample_message() -> crate::model::OutboxMessage {
        build_outbox_message(NewOutboxMessage {
            id: Id::from("msg"),
            tenant: TenantId::from("tenant"),
            envelope_id: Id::from("env"),
            topic: "http://example.com".into(),
            payload: json!({"ok": true}),
            not_before: Some(now_ms()),
            dispatch_key: None,
        })
    }

    #[test]
    fn simple_guard_limits_inflight() {
        let guard = SimpleBudgetGuard::new(BudgetConfig {
            max_inflight: Some(1),
            max_dispatch_per_window: None,
            window_seconds: None,
        });
        let tenant = TenantId::from("t1");
        let msg = sample_message();
        guard.on_dispatch_attempt(&tenant, &msg).unwrap();
        let err = guard.on_dispatch_attempt(&tenant, &msg).unwrap_err();
        assert_eq!(err.as_public().code, "QUOTA.BUDGET_EXCEEDED");
        guard.on_dispatch_result(&tenant, &msg, true).unwrap();
        guard.on_dispatch_attempt(&tenant, &msg).unwrap();
    }
}
