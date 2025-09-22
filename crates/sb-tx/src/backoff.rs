use rand::{rngs::StdRng, Rng, SeedableRng};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_ms: u64,
    pub factor: f64,
    pub jitter: f64,
    pub cap_ms: u64,
}

pub trait BackoffPolicy {
    fn next_after(&self, now_ms: i64, attempts: u32) -> i64;
    fn allowed(&self, attempts: u32) -> bool;
}

impl BackoffPolicy for RetryPolicy {
    fn next_after(&self, now_ms: i64, attempts: u32) -> i64 {
        let mut rng = StdRng::from_entropy();
        let exp = (self.base_ms as f64) * self.factor.powi((attempts.saturating_sub(1)) as i32);
        let capped = exp.min(self.cap_ms as f64);
        let jitter = 1.0 + (rng.gen::<f64>() * 2.0 - 1.0) * self.jitter;
        let delay = (capped * jitter).clamp(self.base_ms as f64, self.cap_ms as f64);
        now_ms + delay as i64
    }

    fn allowed(&self, attempts: u32) -> bool {
        attempts < self.max_attempts
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_ms: 500,
            factor: 2.0,
            jitter: 0.3,
            cap_ms: 60_000,
        }
    }
}
