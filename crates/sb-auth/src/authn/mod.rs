use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use sb_types::prelude::{Id, Subject, SubjectKind, TenantId};

use crate::errors::AuthError;

#[derive(Clone, Debug)]
pub enum AuthnInput {
    Bearer(String),
    ApiKey(String),
    ServiceToken(String),
}

#[async_trait]
pub trait Authenticator: Send + Sync {
    async fn authenticate(&self, input: AuthnInput) -> Result<Subject, AuthError>;
}

#[derive(Clone, Default)]
pub struct StaticTokenAuthenticator {
    pub tokens: Arc<HashMap<String, Subject>>, // token -> subject
}

impl StaticTokenAuthenticator {
    pub fn new(tokens: HashMap<String, Subject>) -> Self {
        Self {
            tokens: Arc::new(tokens),
        }
    }
}

#[async_trait]
impl Authenticator for StaticTokenAuthenticator {
    async fn authenticate(&self, input: AuthnInput) -> Result<Subject, AuthError> {
        let token = match input {
            AuthnInput::Bearer(t) | AuthnInput::ApiKey(t) | AuthnInput::ServiceToken(t) => t,
        };
        self.tokens
            .get(&token)
            .cloned()
            .ok_or_else(|| AuthError::unauthenticated("token not recognized"))
    }
}

pub fn subject_from_claims(
    tenant: impl Into<String>,
    subject_id: impl Into<String>,
    kind: SubjectKind,
) -> Subject {
    Subject {
        kind,
        subject_id: Id(subject_id.into()),
        tenant: TenantId(tenant.into()),
        claims: serde_json::Map::new(),
    }
}
