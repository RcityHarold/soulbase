use serde::{Deserialize, Serialize};

use sb_config::prelude::{ConfigSnapshot, KeyPath};

use crate::backoff::RetryPolicy;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TxConfig {
    pub outbox: OutboxConfig,
    pub idempotency: IdempotencyConfig,
    pub saga: SagaConfig,
    pub dead_letter: DeadLetterConfig,
    pub worker: WorkerConfig,
}

impl TxConfig {
    pub const ROOT_KEY: &'static str = "tx";

    pub fn load(snapshot: &ConfigSnapshot) -> Self {
        let path = KeyPath(Self::ROOT_KEY.to_string());
        snapshot.get::<TxConfig>(&path).unwrap_or_default()
    }

    pub fn build_budget_guard(&self) -> std::sync::Arc<dyn crate::qos::BudgetGuard> {
        crate::qos::build_budget_guard(&self.outbox.budget)
    }
}

impl Default for TxConfig {
    fn default() -> Self {
        Self {
            outbox: OutboxConfig::default(),
            idempotency: IdempotencyConfig::default(),
            saga: SagaConfig::default(),
            dead_letter: DeadLetterConfig::default(),
            worker: WorkerConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct OutboxConfig {
    pub lease_ms: i64,
    pub batch: usize,
    pub max_attempts: u32,
    pub backoff: BackoffConfig,
    pub group_by_dispatch_key: bool,
    pub prefetch_multiplier: usize,
    pub budget: BudgetConfig,
}

impl OutboxConfig {
    pub fn retry_policy(&self) -> RetryPolicy {
        self.backoff
            .clone()
            .into_retry_policy_with_attempts(self.max_attempts)
    }
}

impl Default for OutboxConfig {
    fn default() -> Self {
        Self {
            lease_ms: 15_000,
            batch: 64,
            max_attempts: 12,
            backoff: BackoffConfig::default(),
            group_by_dispatch_key: true,
            prefetch_multiplier: 4,
            budget: BudgetConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BackoffConfig {
    pub base_ms: u64,
    pub factor: f64,
    pub jitter: f64,
    pub cap_ms: u64,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            base_ms: 500,
            factor: 2.0,
            jitter: 0.3,
            cap_ms: 300_000,
        }
    }
}

impl From<BackoffConfig> for RetryPolicy {
    fn from(value: BackoffConfig) -> Self {
        Self {
            max_attempts: u32::MAX,
            base_ms: value.base_ms,
            factor: value.factor,
            jitter: value.jitter,
            cap_ms: value.cap_ms,
        }
    }
}

impl BackoffConfig {
    pub fn into_retry_policy_with_attempts(self, max_attempts: u32) -> RetryPolicy {
        RetryPolicy {
            max_attempts,
            ..self.into()
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BudgetConfig {
    pub max_inflight: Option<u32>,
    pub max_dispatch_per_window: Option<u32>,
    pub window_seconds: Option<u64>,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            max_inflight: None,
            max_dispatch_per_window: None,
            window_seconds: Some(1),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct IdempotencyConfig {
    pub default_ttl_ms: u64,
}

impl Default for IdempotencyConfig {
    fn default() -> Self {
        Self {
            default_ttl_ms: 86_400_000,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SagaConfig {
    pub heartbeat_ms: i64,
    pub default_retry: BackoffConfig,
}

impl Default for SagaConfig {
    fn default() -> Self {
        Self {
            heartbeat_ms: 5_000,
            default_retry: BackoffConfig {
                base_ms: 1_000,
                factor: 2.0,
                jitter: 0.2,
                cap_ms: 60_000,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DeadLetterConfig {
    pub retention_ms: u64,
}

impl Default for DeadLetterConfig {
    fn default() -> Self {
        Self {
            retention_ms: 7 * 24 * 60 * 60 * 1000,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkerConfig {
    pub enable_dispatcher: bool,
    pub dispatcher_interval_ms: u64,
    pub dead_letter_interval_ms: u64,
    pub dead_letter_retention_ms: Option<u64>,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            enable_dispatcher: true,
            dispatcher_interval_ms: 500,
            dead_letter_interval_ms: 60_000,
            dead_letter_retention_ms: None,
        }
    }
}
