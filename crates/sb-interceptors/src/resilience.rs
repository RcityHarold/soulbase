use crate::context::{InterceptContext, ProtoRequest};
use crate::errors::InterceptError;
use sb_errors::prelude::{codes, ErrorBuilder};
use sb_errors::retry::RetryClass;
use serde_json::Value;
use std::time::Duration;
use tokio::time::{sleep, timeout};

#[derive(Clone, Debug)]
pub struct ResiliencePolicy {
    pub timeout: Option<Duration>,
    pub max_retries: usize,
    pub retry_backoff: Option<Duration>,
}

impl Default for ResiliencePolicy {
    fn default() -> Self {
        Self {
            timeout: Some(Duration::from_secs(10)),
            max_retries: 0,
            retry_backoff: Some(Duration::from_millis(200)),
        }
    }
}

pub async fn execute_with_resilience<F, Fut>(
    handler: &mut F,
    cx: &mut InterceptContext,
    req: &mut dyn ProtoRequest,
    policy: &ResiliencePolicy,
) -> Result<serde_json::Value, InterceptError>
where
    F: FnMut(&mut InterceptContext, &mut dyn ProtoRequest) -> Fut + Send,
    Fut: std::future::Future<Output = Result<serde_json::Value, InterceptError>> + Send,
{
    let mut attempt = 0;
    loop {
        attempt += 1;
        let fut = handler(cx, req);
        let result = if let Some(timeout_dur) = policy.timeout {
            match timeout(timeout_dur, fut).await {
                Ok(res) => res,
                Err(_) => {
                    return Err(timeout_error(timeout_dur));
                }
            }
        } else {
            fut.await
        };

        match result {
            Ok(body) => return Ok(body),
            Err(err) => {
                let retryable = matches!(err.inner().retryable, RetryClass::Transient);
                if retryable && attempt <= policy.max_retries {
                    if let Some(backoff) = policy.retry_backoff {
                        sleep(backoff).await;
                    }
                    continue;
                }
                return Err(err);
            }
        }
    }
}

fn timeout_error(timeout: Duration) -> InterceptError {
    InterceptError::new(
        ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE)
            .user_msg("请求处理超时，请稍后重试。")
            .meta_kv("timeout_ms", Value::from(timeout.as_millis() as i64))
            .build(),
    )
}
