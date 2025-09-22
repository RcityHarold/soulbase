#![cfg(feature = "surreal")]

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SurrealProtocol {
    Ws,
    Http,
}

#[derive(Clone, Debug)]
pub struct SurrealCredentials {
    pub username: String,
    pub password: String,
}

impl SurrealCredentials {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SurrealConfig {
    pub endpoint: String,
    pub namespace: String,
    pub database: String,
    pub protocol: SurrealProtocol,
    pub credentials: Option<SurrealCredentials>,
    pub max_connections: usize,
    pub strict: bool,
}

impl Default for SurrealConfig {
    fn default() -> Self {
        Self {
            endpoint: "ws://127.0.0.1:8000".into(),
            namespace: "soul".into(),
            database: "base".into(),
            protocol: SurrealProtocol::Ws,
            credentials: None,
            max_connections: 8,
            strict: true,
        }
    }
}

impl SurrealConfig {
    pub fn with_credentials(mut self, credentials: SurrealCredentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    pub fn with_protocol(mut self, protocol: SurrealProtocol) -> Self {
        self.protocol = protocol;
        self
    }

    pub fn with_pool(mut self, max_connections: usize) -> Self {
        self.max_connections = max_connections.max(1);
        self
    }

    pub fn strict_mode(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
}
