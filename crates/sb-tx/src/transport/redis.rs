#![cfg(feature = "transport-redis")]

use std::time::Duration;

use async_trait::async_trait;
use redis::aio::MultiplexedConnection;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

use crate::errors::TxError;
use crate::model::OutboxMessage;
use crate::outbox::OutboxTransport;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RedisTransportConfig {
    /// Redis connection string (e.g. redis://127.0.0.1:6379/)
    pub url: String,
    /// List key used to retain dispatched messages for auditing/consumers.
    pub list_key: String,
    /// Optional pub/sub channel; when set messages are published as well.
    pub channel: Option<String>,
    /// Optional timeout for connection acquisition in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl Default for RedisTransportConfig {
    fn default() -> Self {
        Self {
            url: "redis://127.0.0.1:6379/".into(),
            list_key: "sb:tx:outbox".into(),
            channel: Some("sb.tx.outbox".into()),
            timeout_ms: Some(15_000),
        }
    }
}

#[derive(Clone)]
pub struct RedisTransport {
    client: redis::Client,
    list_key: String,
    channel: Option<String>,
    timeout: Duration,
}

impl RedisTransport {
    pub fn new(config: RedisTransportConfig) -> Result<Self, TxError> {
        let client = redis::Client::open(config.url).map_err(|err| {
            TxError::provider_unavailable(format!("redis client create failed: {err}"))
        })?;
        let timeout = config
            .timeout_ms
            .map(|ms| Duration::from_millis(ms))
            .unwrap_or_else(|| Duration::from_secs(15));
        Ok(Self {
            client,
            list_key: config.list_key,
            channel: config.channel,
            timeout,
        })
    }

    async fn connection(&self) -> Result<MultiplexedConnection, TxError> {
        let conn =
            tokio::time::timeout(self.timeout, self.client.get_multiplexed_tokio_connection())
                .await
                .map_err(|_| TxError::provider_unavailable("redis connect timed out"))?
                .map_err(|err| {
                    TxError::provider_unavailable(format!("redis connect failed: {err}"))
                })?;
        Ok(conn)
    }
}

#[async_trait]
impl OutboxTransport for RedisTransport {
    async fn send(&self, message: &OutboxMessage) -> Result<(), TxError> {
        #[derive(Serialize)]
        struct RedisEnvelope<'a> {
            id: &'a str,
            tenant: &'a str,
            topic: &'a str,
            attempts: u32,
            payload: &'a serde_json::Value,
        }

        let envelope = RedisEnvelope {
            id: message.id.as_str(),
            tenant: message.tenant.as_ref(),
            topic: message.topic.as_str(),
            attempts: message.attempts + 1,
            payload: &message.payload,
        };

        let body = serde_json::to_string(&envelope)
            .map_err(|err| TxError::schema(format!("serialize redis envelope failed: {err}")))?;

        let mut conn = self.connection().await?;
        conn.rpush::<_, _, ()>(&self.list_key, &body)
            .await
            .map_err(|err| TxError::provider_unavailable(format!("redis RPUSH failed: {err}")))?;

        if let Some(channel) = &self.channel {
            conn.publish::<_, _, ()>(channel, &body)
                .await
                .map_err(|err| {
                    TxError::provider_unavailable(format!(
                        "redis publish to {channel} failed: {err}"
                    ))
                })?;
        }

        Ok(())
    }
}
