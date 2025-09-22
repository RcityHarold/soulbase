use std::sync::Arc;

use crate::attr::AttributeProvider;
use crate::authn::{Authenticator, AuthnInput};
use crate::cache::DecisionCache;
use crate::consent::ConsentVerifier;
use crate::errors::AuthError;
use crate::events::wrap_decision;
use crate::model::{
    hash_attrs, Action, AuthzRequest, Decision, DecisionKey, QuotaKey, ResourceUrn,
};
use crate::observe::decision_labels;
use crate::pdp::Authorizer;
use crate::quota::{QuotaOutcome, QuotaStore};

use sb_types::prelude::Consent;
use sb_types::prelude::Subject;

#[derive(Clone, Debug)]
pub struct AuthContext {
    pub input: AuthnInput,
    pub resource: ResourceUrn,
    pub action: Action,
    pub attrs: serde_json::Value,
    pub consent: Option<Consent>,
    pub correlation_id: Option<String>,
    pub cost: i64,
}

#[derive(Clone, Debug)]
pub struct AuthResult {
    pub subject: Subject,
    pub decision: Decision,
}

pub struct AuthFacade {
    authenticator: Arc<dyn Authenticator>,
    attr_provider: Arc<dyn AttributeProvider>,
    authorizer: Arc<dyn Authorizer>,
    quota: Arc<dyn QuotaStore>,
    consent: Arc<dyn ConsentVerifier>,
    cache: Arc<dyn DecisionCache>,
}

impl AuthFacade {
    pub fn new(
        authenticator: Arc<dyn Authenticator>,
        attr_provider: Arc<dyn AttributeProvider>,
        authorizer: Arc<dyn Authorizer>,
        quota: Arc<dyn QuotaStore>,
        consent: Arc<dyn ConsentVerifier>,
        cache: Arc<dyn DecisionCache>,
    ) -> Self {
        Self {
            authenticator,
            attr_provider,
            authorizer,
            quota,
            consent,
            cache,
        }
    }

    pub async fn authorize(&self, ctx: AuthContext) -> Result<AuthResult, AuthError> {
        let subject = self.authenticator.authenticate(ctx.input.clone()).await?;

        let attrs_from_provider = self
            .attr_provider
            .attributes_for(&subject, &ctx.resource)
            .await;
        let merged_attrs =
            attrs_from_provider.merged(&crate::model::AttributeMap::from(ctx.attrs.clone()));

        let decision_key = DecisionKey {
            tenant: subject.tenant.0.clone(),
            subject_id: subject.subject_id.0.clone(),
            resource: ctx.resource.clone(),
            action: ctx.action.clone(),
            attrs_hash: hash_attrs(&merged_attrs.0),
        };

        if let Some(decision) = self.cache.get(&decision_key).await {
            return Ok(AuthResult { subject, decision });
        }

        let request = AuthzRequest {
            subject: subject.clone(),
            resource: ctx.resource.clone(),
            action: ctx.action.clone(),
            attrs: merged_attrs.0.clone(),
            consent: ctx.consent.clone(),
            correlation_id: ctx.correlation_id.clone(),
        };

        let decision = self.authorizer.decide(&request).await?;
        if decision.allow {
            if let Some(consent) = &request.consent {
                if !self.consent.verify(consent, &request).await? {
                    return Err(AuthError::policy_deny("consent invalid"));
                }
            }
        }

        if decision.allow {
            let quota_key = QuotaKey {
                tenant: subject.tenant.0.clone(),
                subject_id: subject.subject_id.0.clone(),
                resource: ctx.resource.clone(),
                action: ctx.action.clone(),
            };
            match self.quota.check_and_consume(&quota_key, ctx.cost).await? {
                QuotaOutcome::Allowed => {}
                QuotaOutcome::RateLimited => return Err(AuthError::rate_limited()),
                QuotaOutcome::BudgetExceeded => return Err(AuthError::budget_exceeded()),
            }
        }

        if decision.cache_ttl_ms > 0 {
            self.cache.put(decision_key, &decision).await;
        }

        let _labels = decision_labels(
            &ctx.resource,
            &ctx.action,
            if decision.allow { "allow" } else { "deny" },
        );
        let _event = wrap_decision(&subject, &ctx.resource, &ctx.action, &decision);

        Ok(AuthResult { subject, decision })
    }
}
