use async_trait::async_trait;
use sb_types::prelude::Consent;

use crate::errors::AuthError;
use crate::model::AuthzRequest;

#[async_trait]
pub trait ConsentVerifier: Send + Sync {
    async fn verify(&self, consent: &Consent, request: &AuthzRequest) -> Result<bool, AuthError>;
}

#[derive(Default)]
pub struct BasicConsentVerifier;

#[async_trait]
impl ConsentVerifier for BasicConsentVerifier {
    async fn verify(&self, consent: &Consent, request: &AuthzRequest) -> Result<bool, AuthError> {
        if let Some(exp) = consent.expires_at {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            if exp.0 < now_ms {
                return Ok(false);
            }
        }
        for scope in &consent.scopes {
            if scope.resource == request.resource.0
                && scope.action == format_action(&request.action)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn format_action(action: &crate::model::Action) -> String {
    match action {
        crate::model::Action::Read => "read",
        crate::model::Action::Write => "write",
        crate::model::Action::Invoke => "invoke",
        crate::model::Action::List => "list",
        crate::model::Action::Admin => "admin",
        crate::model::Action::Configure => "configure",
    }
    .to_string()
}
