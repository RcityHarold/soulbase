use async_trait::async_trait;
use serde_json::json;
use std::collections::HashSet;

use crate::errors::AuthError;
use crate::model::{Action, AuthzRequest, Decision};

#[async_trait]
pub trait Authorizer: Send + Sync {
    async fn decide(&self, request: &AuthzRequest) -> Result<Decision, AuthError>;
}

#[derive(Default)]
pub struct AllowAllAuthorizer;

#[async_trait]
impl Authorizer for AllowAllAuthorizer {
    async fn decide(&self, _request: &AuthzRequest) -> Result<Decision, AuthError> {
        Ok(Decision::allow_default())
    }
}

#[derive(Default)]
pub struct StaticPolicyAuthorizer {
    pub allow_pairs: HashSet<(String, Action)>,
}

#[async_trait]
impl Authorizer for StaticPolicyAuthorizer {
    async fn decide(&self, request: &AuthzRequest) -> Result<Decision, AuthError> {
        let key = (request.resource.0.clone(), request.action.clone());
        if self.allow_pairs.contains(&key) {
            Ok(Decision {
                allow: true,
                reason: None,
                obligations: Vec::new(),
                evidence: json!({"policy": "static_allow"}),
                cache_ttl_ms: 1000,
            })
        } else {
            Ok(Decision::deny("static policy deny"))
        }
    }
}
