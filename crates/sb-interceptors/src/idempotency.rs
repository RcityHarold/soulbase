use crate::context::InterceptContext;
use crate::errors::InterceptError;
use sb_errors::prelude::{codes, ErrorBuilder};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Clone, Debug)]
pub struct StoredResponse {
    pub status: u16,
    pub body: serde_json::Value,
    pub headers: Vec<(String, String)>,
}

#[async_trait::async_trait]
pub trait IdempotencyStore: Send + Sync {
    async fn get(&self, key: &str) -> Option<StoredResponse>;
    async fn put(&self, key: String, response: StoredResponse, ttl: Duration);
}

#[derive(Default)]
pub struct MemoryIdempotencyStore {
    inner: Mutex<HashMap<String, (Instant, StoredResponse)>>,
}

impl MemoryIdempotencyStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl IdempotencyStore for MemoryIdempotencyStore {
    async fn get(&self, key: &str) -> Option<StoredResponse> {
        let mut guard = self.inner.lock().await;
        if let Some((expiry, stored)) = guard.get(key) {
            if *expiry > Instant::now() {
                return Some(stored.clone());
            }
        }
        guard.remove(key);
        None
    }

    async fn put(&self, key: String, response: StoredResponse, ttl: Duration) {
        let mut guard = self.inner.lock().await;
        guard.insert(key, (Instant::now() + ttl, response));
    }
}

#[derive(Clone)]
pub struct IdempotencyLayer {
    store: Arc<dyn IdempotencyStore>,
    ttl: Duration,
    max_body_size: usize,
}

impl IdempotencyLayer {
    pub fn new(store: Arc<dyn IdempotencyStore>, ttl: Duration, max_body_size: usize) -> Self {
        Self {
            store,
            ttl,
            max_body_size,
        }
    }

    pub fn store(&self) -> Arc<dyn IdempotencyStore> {
        self.store.clone()
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    pub fn max_body_size(&self) -> usize {
        self.max_body_size
    }
}

impl std::fmt::Debug for IdempotencyLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdempotencyLayer")
            .field("ttl", &self.ttl)
            .field("max_body_size", &self.max_body_size)
            .finish()
    }
}

pub fn idempotency_error(msg: impl Into<String>) -> InterceptError {
    InterceptError::new(
        ErrorBuilder::new(codes::SCHEMA_VALIDATION_FAILED)
            .user_msg("请求的幂等键无效。")
            .dev_msg(msg)
            .build(),
    )
}

pub fn oversized_body_error(size: usize, max: usize) -> InterceptError {
    InterceptError::new(
        ErrorBuilder::new(codes::POLICY_DENY_TOOL)
            .user_msg("响应体超出幂等缓存限制。")
            .dev_msg(format!("size={} max={}", size, max))
            .build(),
    )
}

pub fn build_idempotency_key(
    cx: &InterceptContext,
    raw_key: &str,
    method: &str,
    path: &str,
) -> String {
    let tenant = cx
        .tenant_header
        .as_deref()
        .or_else(|| cx.subject.as_ref().map(|s| s.tenant.0.as_str()))
        .unwrap_or("unknown");
    format!("{tenant}:{method}:{path}:{raw_key}")
}
