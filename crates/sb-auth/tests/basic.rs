use sb_auth::prelude::*;
use sb_types::prelude::SubjectKind;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

fn subject(tenant: &str, id: &str) -> sb_types::prelude::Subject {
    subject_from_claims(tenant.to_string(), id.to_string(), SubjectKind::User)
}

#[tokio::test]
async fn authorize_with_cache_and_quota() {
    let mut tokens = HashMap::new();
    tokens.insert("token123".to_string(), subject("tenantA", "user1"));
    let authenticator = StaticTokenAuthenticator::new(tokens);

    let attr_provider = StaticAttributeProvider::default();

    let mut allow_pairs = HashSet::new();
    allow_pairs.insert(("soul:tool:browser".to_string(), Action::Invoke));
    let authorizer = StaticPolicyAuthorizer { allow_pairs };

    let quota_key = QuotaKey {
        tenant: "tenantA".to_string(),
        subject_id: "user1".to_string(),
        resource: ResourceUrn("soul:tool:browser".into()),
        action: Action::Invoke,
    };
    let quota_limits = HashMap::from([(quota_key.clone(), 5)]);
    let quota = MemoryQuotaStore::with_limits(quota_limits);

    let consent = BasicConsentVerifier::default();
    let cache = MemoryDecisionCache::default();

    let facade = AuthFacade::new(
        Arc::new(authenticator),
        Arc::new(attr_provider),
        Arc::new(authorizer),
        Arc::new(quota),
        Arc::new(consent),
        Arc::new(cache),
    );

    let ctx = AuthContext {
        input: AuthnInput::Bearer("token123".into()),
        resource: ResourceUrn("soul:tool:browser".into()),
        action: Action::Invoke,
        attrs: json!({}),
        consent: None,
        correlation_id: None,
        cost: 1,
    };

    let result = facade.authorize(ctx.clone()).await.expect("decision");
    assert!(result.decision.allow);

    // second call should hit cache and still allow
    let result_cached = facade.authorize(ctx).await.expect("decision cached");
    assert!(result_cached.decision.allow);
}

#[tokio::test]
async fn quota_exceeded_returns_error() {
    let mut tokens = HashMap::new();
    tokens.insert("token456".to_string(), subject("tenantA", "user2"));
    let authenticator = StaticTokenAuthenticator::new(tokens);

    let attr_provider = StaticAttributeProvider::default();

    let mut allow_pairs = HashSet::new();
    allow_pairs.insert(("soul:tool:browser".to_string(), Action::Invoke));
    let authorizer = StaticPolicyAuthorizer { allow_pairs };

    let quota_key = QuotaKey {
        tenant: "tenantA".to_string(),
        subject_id: "user2".to_string(),
        resource: ResourceUrn("soul:tool:browser".into()),
        action: Action::Invoke,
    };
    let quota_limits = HashMap::from([(quota_key.clone(), 0)]);
    let quota = MemoryQuotaStore::with_limits(quota_limits);

    let consent = BasicConsentVerifier::default();
    let cache = MemoryDecisionCache::default();

    let facade = AuthFacade::new(
        Arc::new(authenticator),
        Arc::new(attr_provider),
        Arc::new(authorizer),
        Arc::new(quota),
        Arc::new(consent),
        Arc::new(cache),
    );

    let ctx = AuthContext {
        input: AuthnInput::Bearer("token456".into()),
        resource: ResourceUrn("soul:tool:browser".into()),
        action: Action::Invoke,
        attrs: json!({}),
        consent: None,
        correlation_id: None,
        cost: 1,
    };

    let err = facade.authorize(ctx).await.expect_err("quota exceeded");
    let public = err.into_inner().to_public();
    assert_eq!(public.code, "QUOTA.BUDGET_EXCEEDED");
}
