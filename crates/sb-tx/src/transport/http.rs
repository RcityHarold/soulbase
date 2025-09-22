use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};

use crate::errors::TxError;
use crate::model::OutboxMessage;
use crate::outbox::OutboxTransport;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HttpTransportConfig {
    #[serde(default = "HttpTransportConfig::default_method")]
    pub method: String,
    #[serde(default)]
    pub timeout_ms: u64,
    #[serde(default)]
    pub default_headers: Vec<(String, String)>,
}

impl Default for HttpTransportConfig {
    fn default() -> Self {
        Self {
            method: "POST".into(),
            timeout_ms: 15_000,
            default_headers: Vec::new(),
        }
    }
}

impl HttpTransportConfig {
    fn default_method() -> String {
        "POST".into()
    }
}

pub struct HttpTransport {
    client: Client,
    method: Method,
    headers: HeaderMap,
    timeout: Duration,
}

impl HttpTransport {
    pub fn new(config: HttpTransportConfig) -> Result<Self, TxError> {
        let method = config
            .method
            .parse::<Method>()
            .map_err(|err| TxError::schema(format!("invalid HTTP method: {err}")))?;

        let timeout = if config.timeout_ms == 0 {
            Duration::from_secs(15)
        } else {
            Duration::from_millis(config.timeout_ms)
        };

        let mut headers = HeaderMap::new();
        for (name, value) in config.default_headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|err| TxError::schema(format!("invalid header name {name}: {err}")))?;
            let header_value = HeaderValue::from_str(&value).map_err(|err| {
                TxError::schema(format!("invalid header value for {header_name}: {err}"))
            })?;
            headers.append(header_name, header_value);
        }

        let client = Client::builder().timeout(timeout).build().map_err(|err| {
            TxError::provider_unavailable(format!("http client build failed: {err}"))
        })?;

        Ok(Self {
            client,
            method,
            headers,
            timeout,
        })
    }

    fn build_url<'a>(&self, message: &'a OutboxMessage) -> Result<&'a str, TxError> {
        let url = message.topic.as_str();
        if url.starts_with("http://") || url.starts_with("https://") {
            Ok(url)
        } else {
            Err(TxError::schema(
                "outbox topic must be http(s) URL for HttpTransport",
            ))
        }
    }
}

#[async_trait]
impl OutboxTransport for HttpTransport {
    async fn send(&self, message: &OutboxMessage) -> Result<(), TxError> {
        let url = self.build_url(message)?;
        let mut request = self
            .client
            .request(self.method.clone(), url)
            .timeout(self.timeout)
            .json(&message.payload);

        if !self.headers.is_empty() {
            request = request.headers(self.headers.clone());
        }

        let response = request
            .send()
            .await
            .map_err(|err| TxError::provider_unavailable(format!("http send failed: {err}")))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(TxError::provider_unavailable(format!(
                "http transport status {}",
                response.status()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NewOutboxMessage;
    use crate::util::now_ms;
    use axum::http::StatusCode;
    use sb_errors::prelude::codes;
    use sb_types::prelude::{Id, TenantId};
    use serde_json::json;
    use tokio::task::JoinHandle;

    fn make_message(url: &str) -> OutboxMessage {
        crate::outbox::build_outbox_message(NewOutboxMessage {
            id: Id::from("test"),
            tenant: TenantId::from("tenant"),
            envelope_id: Id::from("env"),
            topic: url.into(),
            payload: json!({"hello": "world"}),
            not_before: Some(now_ms()),
            dispatch_key: None,
        })
    }

    async fn spawn_echo_server(status: StatusCode) -> (String, JoinHandle<()>) {
        use axum::response::IntoResponse;
        use axum::{routing::any, Router};

        let app = Router::new().route(
            "/*path",
            any(move |body: String| async move {
                let _ = body;
                status.into_response()
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        (format!("http://{}", addr), handle)
    }

    #[tokio::test]
    async fn http_transport_success() {
        let (base, handle) = spawn_echo_server(StatusCode::OK).await;
        let transport = HttpTransport::new(HttpTransportConfig::default()).unwrap();
        let message = make_message(&format!("{base}/ok"));
        transport.send(&message).await.unwrap();
        handle.abort();
    }

    #[tokio::test]
    async fn http_transport_failure() {
        let (base, handle) = spawn_echo_server(StatusCode::INTERNAL_SERVER_ERROR).await;
        let transport = HttpTransport::new(HttpTransportConfig::default()).unwrap();
        let message = make_message(&format!("{base}/fail"));
        let err = transport.send(&message).await.unwrap_err();
        assert_eq!(err.as_public().code, codes::PROVIDER_UNAVAILABLE.0);
        handle.abort();
    }
}
