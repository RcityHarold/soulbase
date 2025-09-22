#![cfg(feature = "transport-kafka")]

use std::time::Duration;

use async_trait::async_trait;
use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::util::Timeout;
use serde::{Deserialize, Serialize};

use crate::errors::TxError;
use crate::model::OutboxMessage;
use crate::outbox::OutboxTransport;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KafkaTransportConfig {
    pub brokers: String,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub linger_ms: Option<u64>,
    #[serde(default)]
    pub ack_timeout_ms: Option<u64>,
}

impl Default for KafkaTransportConfig {
    fn default() -> Self {
        Self {
            brokers: "localhost:9092".into(),
            topic: None,
            linger_ms: Some(5),
            ack_timeout_ms: Some(15_000),
        }
    }
}

pub struct KafkaTransport {
    producer: FutureProducer,
    topic_override: Option<String>,
    ack_timeout: Timeout,
}

impl KafkaTransport {
    pub fn new(config: KafkaTransportConfig) -> Result<Self, TxError> {
        let mut builder = ClientConfig::new();
        builder.set("bootstrap.servers", config.brokers);
        if let Some(linger) = config.linger_ms {
            builder.set("linger.ms", linger.to_string());
        }
        let producer = builder.create().map_err(|err| {
            TxError::provider_unavailable(format!("kafka producer build failed: {err}"))
        })?;

        let timeout = config
            .ack_timeout_ms
            .map(|ms| Timeout::After(Duration::from_millis(ms)))
            .unwrap_or(Timeout::After(Duration::from_secs(15)));

        Ok(Self {
            producer,
            topic_override: config.topic,
            ack_timeout: timeout,
        })
    }

    fn topic_for(&self, message: &OutboxMessage) -> Result<&str, TxError> {
        if let Some(topic) = &self.topic_override {
            Ok(topic.as_str())
        } else if let Some(stripped) = message.topic.strip_prefix("kafka://") {
            Ok(stripped)
        } else {
            Err(TxError::schema(
                "outbox topic must be kafka://<topic> or set in KafkaTransportConfig",
            ))
        }
    }
}

#[async_trait]
impl OutboxTransport for KafkaTransport {
    async fn send(&self, message: &OutboxMessage) -> Result<(), TxError> {
        let topic = self.topic_for(message)?;
        let payload = serde_json::to_vec(&message.payload)
            .map_err(|err| TxError::schema(format!("serialize kafka payload failed: {err}")))?;

        let record = FutureRecord::to(topic)
            .payload(&payload)
            .key(message.id.as_str());
        self.producer
            .send(record, self.ack_timeout)
            .await
            .map_err(|(err, _)| {
                TxError::provider_unavailable(format!("kafka send failed: {err}"))
            })?;

        Ok(())
    }
}
