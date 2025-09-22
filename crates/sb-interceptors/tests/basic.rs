use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use sb_auth::prelude::*;
use sb_interceptors::context::{InterceptContext, ProtoRequest, ProtoResponse};
use sb_interceptors::errors::InterceptError;
use sb_interceptors::idempotency::{IdempotencyLayer, MemoryIdempotencyStore};
use sb_interceptors::policy::{MatchCond, RouteBindingSpec, RoutePolicy, RoutePolicySpec};
#[cfg(feature = "schema-json")]
use sb_interceptors::schema::JsonSchemaRegistry;
use sb_interceptors::stages::authn_map::AuthnMapStage;
use sb_interceptors::stages::authz_quota::AuthzQuotaStage;
use sb_interceptors::stages::context_init::ContextInitStage;
use sb_interceptors::stages::idempotency::IdempotencyStage;
use sb_interceptors::stages::obligations::ObligationsStage;
use sb_interceptors::stages::response_stamp::ResponseStampStage;
use sb_interceptors::stages::route_policy::RoutePolicyStage;
use sb_interceptors::stages::schema_guard::SchemaGuardStage;
use sb_interceptors::stages::{InterceptorChain, Stage};
use sb_types::prelude::{Subject, SubjectKind};
use serde_json::json;
use std::time::Duration;

struct TestRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: serde_json::Value,
}

impl TestRequest {
    fn new(method: &str, path: &str, body: serde_json::Value) -> Self {
        Self {
            method: method.to_string(),
            path: path.to_string(),
            headers: HashMap::new(),
            body,
        }
    }
}

#[async_trait]
impl ProtoRequest for TestRequest {
    fn method(&self) -> &str {
        &self.method
    }

    fn path(&self) -> &str {
        &self.path
    }

    fn header(&self, name: &str) -> Option<String> {
        self.headers.get(name).cloned()
    }

    async fn read_json(&mut self) -> Result<serde_json::Value, InterceptError> {
        Ok(self.body.clone())
    }
}

struct TestResponse {
    status: u16,
    headers: HashMap<String, String>,
    pub body: Option<serde_json::Value>,
}

impl TestResponse {
    fn new() -> Self {
        Self {
            status: 200,
            headers: HashMap::new(),
            body: None,
        }
    }
}

#[async_trait]
impl ProtoResponse for TestResponse {
    fn set_status(&mut self, code: u16) {
        self.status = code;
    }

    fn insert_header(&mut self, name: &str, value: &str) {
        self.headers.insert(name.to_string(), value.to_string());
    }

    async fn write_json(&mut self, body: &serde_json::Value) -> Result<(), InterceptError> {
        self.body = Some(body.clone());
        Ok(())
    }
}

fn test_subject(tenant: &str, id: &str) -> Subject {
    sb_auth::prelude::subject_from_claims(tenant, id, SubjectKind::User)
}

struct ObligationAuthorizer;

#[async_trait]
impl Authorizer for ObligationAuthorizer {
    async fn decide(&self, request: &AuthzRequest) -> Result<Decision, AuthError> {
        let mut allow_pairs = HashSet::new();
        allow_pairs.insert(("soul:tool:browser".to_string(), Action::Invoke));
        if allow_pairs.contains(&(request.resource.0.clone(), request.action.clone())) {
            Ok(Decision {
                allow: true,
                reason: None,
                obligations: vec![Obligation {
                    kind: "mask".to_string(),
                    params: json!({"path": "secret", "replacement": "***"}),
                }],
                evidence: json!({"policy": "obligation"}),
                cache_ttl_ms: 0,
            })
        } else {
            Err(AuthError::policy_deny("route not allowed"))
        }
    }
}

#[tokio::test]
async fn interceptor_chain_runs_happy_path() {
    let mut tokens = HashMap::new();
    tokens.insert("token123".to_string(), test_subject("tenantA", "user1"));
    let authenticator = StaticTokenAuthenticator::new(tokens);

    let attr_provider = StaticAttributeProvider::default();
    let quota = MemoryQuotaStore::with_limits(HashMap::new());
    let consent = BasicConsentVerifier::default();
    let cache = MemoryDecisionCache::default();

    let facade = AuthFacade::new(
        Arc::new(authenticator.clone()),
        Arc::new(attr_provider),
        Arc::new(ObligationAuthorizer),
        Arc::new(quota),
        Arc::new(consent),
        Arc::new(cache),
    );
    let facade = Arc::new(facade);

    let idempotency_layer = IdempotencyLayer::new(
        Arc::new(MemoryIdempotencyStore::new()),
        Duration::from_secs(60),
        16 * 1024,
    );
    let schema_stage = SchemaGuardStage::default();
    let idempotency_stage = IdempotencyStage::new(Some(idempotency_layer.clone()));

    let policy = RoutePolicy::new(vec![RoutePolicySpec {
        when: MatchCond::Http {
            method: "POST".to_string(),
            path_prefix: "/v1/tools".to_string(),
        },
        bind: RouteBindingSpec {
            resource: "soul:tool:browser".to_string(),
            action: "Invoke".to_string(),
            attrs_from_body: true,
            request_schema: None,
            response_schema: None,
        },
    }]);

    let request_stages: Vec<Box<dyn Stage>> = vec![
        Box::new(ContextInitStage::new()),
        Box::new(RoutePolicyStage::new(policy)),
        Box::new(schema_stage.clone()),
        Box::new(idempotency_stage.clone()),
        Box::new(AuthnMapStage::new(Box::new(authenticator))),
        Box::new(AuthzQuotaStage::new(facade.clone())),
    ];
    let response_stages: Vec<Box<dyn sb_interceptors::stages::ResponseStage>> = vec![
        Box::new(schema_stage),
        Box::new(ObligationsStage),
        Box::new(ResponseStampStage),
    ];
    let chain = InterceptorChain::new(request_stages, response_stages);

    let mut req = TestRequest::new("POST", "/v1/tools/execute", json!({"secret": "value"}));
    req.headers
        .insert("Authorization".into(), "Bearer token123".into());
    req.headers.insert("X-Soul-Tenant".into(), "tenantA".into());
    req.headers.insert("X-Request-Id".into(), "req-1".into());

    let mut rsp = TestResponse::new();

    let result = chain
        .run_with_handler(
            InterceptContext::default(),
            &mut req,
            &mut rsp,
            |_, _| async {
                Ok(json!({
                    "secret": "value",
                    "visible": "ok",
                }))
            },
        )
        .await;

    assert!(result.is_ok(), "chain should succeed: {:?}", result.err());
    let body = rsp.body.expect("response body");
    assert_eq!(body.get("secret").and_then(|v| v.as_str()), Some("***"));
    assert_eq!(body.get("visible").and_then(|v| v.as_str()), Some("ok"));
    assert_eq!(
        rsp.headers.get("X-Request-Id").map(|s| s.as_str()),
        Some("req-1")
    );
    assert_eq!(
        rsp.headers.get("X-Obligations").map(|s| s.as_str()),
        Some("mask")
    );
}

#[tokio::test]
async fn denies_missing_auth_header() {
    let policy = RoutePolicy::new(vec![RoutePolicySpec {
        when: MatchCond::Http {
            method: "GET".to_string(),
            path_prefix: "/v1/items".to_string(),
        },
        bind: RouteBindingSpec {
            resource: "soul:storage:kv".to_string(),
            action: "Read".to_string(),
            attrs_from_body: false,
            request_schema: None,
            response_schema: None,
        },
    }]);

    let mut tokens = HashMap::new();
    tokens.insert("token123".to_string(), test_subject("tenantA", "user1"));
    let authenticator = StaticTokenAuthenticator::new(tokens);
    let attr_provider = StaticAttributeProvider::default();
    let quota = MemoryQuotaStore::with_limits(HashMap::new());
    let consent = BasicConsentVerifier::default();
    let cache = MemoryDecisionCache::default();

    let facade = AuthFacade::new(
        Arc::new(authenticator.clone()),
        Arc::new(attr_provider),
        Arc::new(ObligationAuthorizer),
        Arc::new(quota),
        Arc::new(consent),
        Arc::new(cache),
    );
    let facade = Arc::new(facade);

    let request_stages: Vec<Box<dyn Stage>> = vec![
        Box::new(ContextInitStage::new()),
        Box::new(RoutePolicyStage::new(policy)),
        Box::new(SchemaGuardStage::default()),
        Box::new(IdempotencyStage::default()),
        Box::new(AuthnMapStage::new(Box::new(authenticator))),
        Box::new(AuthzQuotaStage::new(facade.clone())),
    ];
    let response_stages: Vec<Box<dyn sb_interceptors::stages::ResponseStage>> = vec![
        Box::new(SchemaGuardStage::default()),
        Box::new(ResponseStampStage),
    ];
    let chain = InterceptorChain::new(request_stages, response_stages);

    let mut req = TestRequest::new("GET", "/v1/items", json!({}));
    req.headers.insert("X-Soul-Tenant".into(), "tenantA".into());
    let mut rsp = TestResponse::new();

    let result = chain
        .run_with_handler(
            InterceptContext::default(),
            &mut req,
            &mut rsp,
            |_, _| async { Ok(json!({})) },
        )
        .await;

    assert!(result.is_ok());
    assert_eq!(rsp.status, 401);
    let body = rsp.body.expect("error body");
    assert_eq!(
        body.get("code").and_then(|v| v.as_str()),
        Some("AUTH.UNAUTHENTICATED")
    );
}

#[tokio::test]
async fn idempotency_replays_cached_response() {
    let mut tokens = HashMap::new();
    tokens.insert("token".to_string(), test_subject("tenant", "user"));
    let authenticator = StaticTokenAuthenticator::new(tokens);

    let attr_provider = StaticAttributeProvider::default();
    let quota = MemoryQuotaStore::with_limits(HashMap::new());
    let consent = BasicConsentVerifier::default();
    let cache = MemoryDecisionCache::default();

    let facade = AuthFacade::new(
        Arc::new(authenticator.clone()),
        Arc::new(attr_provider),
        Arc::new(ObligationAuthorizer),
        Arc::new(quota),
        Arc::new(consent),
        Arc::new(cache),
    );
    let facade = Arc::new(facade);

    let policy = RoutePolicy::new(vec![RoutePolicySpec {
        when: MatchCond::Http {
            method: "POST".to_string(),
            path_prefix: "/v1/tasks".to_string(),
        },
        bind: RouteBindingSpec {
            resource: "soul:tool:browser".to_string(),
            action: "Invoke".to_string(),
            attrs_from_body: false,
            request_schema: None,
            response_schema: None,
        },
    }]);

    let idem_layer = IdempotencyLayer::new(
        Arc::new(MemoryIdempotencyStore::new()),
        Duration::from_secs(120),
        32 * 1024,
    );
    let idem_stage = IdempotencyStage::new(Some(idem_layer.clone()));

    let route_stage = RoutePolicyStage::new(policy);

    let request_stages: Vec<Box<dyn Stage>> = vec![
        Box::new(ContextInitStage::new()),
        Box::new(route_stage),
        Box::new(SchemaGuardStage::default()),
        Box::new(idem_stage.clone()),
        Box::new(AuthnMapStage::new(Box::new(authenticator.clone()))),
        Box::new(AuthzQuotaStage::new(facade.clone())),
    ];
    let response_stages: Vec<Box<dyn sb_interceptors::stages::ResponseStage>> = vec![
        Box::new(SchemaGuardStage::default()),
        Box::new(ResponseStampStage),
    ];
    let chain = InterceptorChain::new(request_stages, response_stages);

    let call_counter = Arc::new(AtomicUsize::new(0));

    let mut req1 = TestRequest::new("POST", "/v1/tasks", json!({"task": "ok"}));
    req1.headers
        .insert("Authorization".into(), "Bearer token".into());
    req1.headers.insert("X-Soul-Tenant".into(), "tenant".into());
    req1.headers
        .insert("Idempotency-Key".into(), "abc123".into());
    let mut rsp1 = TestResponse::new();

    let counter_clone = call_counter.clone();
    let res1 = chain
        .run_with_handler(
            InterceptContext::default(),
            &mut req1,
            &mut rsp1,
            move |_, _| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
                async { Ok(json!({"ok": true})) }
            },
        )
        .await;
    assert!(res1.is_ok());
    assert_eq!(call_counter.load(Ordering::SeqCst), 1);

    let mut req2 = TestRequest::new("POST", "/v1/tasks", json!({"task": "ok"}));
    req2.headers
        .insert("Authorization".into(), "Bearer token".into());
    req2.headers.insert("X-Soul-Tenant".into(), "tenant".into());
    req2.headers
        .insert("Idempotency-Key".into(), "abc123".into());
    let mut rsp2 = TestResponse::new();

    let res2 = chain
        .run_with_handler(
            InterceptContext::default(),
            &mut req2,
            &mut rsp2,
            |_, _| async {
                Err(InterceptError::from_public(
                    sb_errors::prelude::codes::UNKNOWN_INTERNAL,
                    "should not run",
                ))
            },
        )
        .await;

    assert!(res2.is_ok());
    assert_eq!(
        call_counter.load(Ordering::SeqCst),
        1,
        "handler should not rerun"
    );
    assert_eq!(rsp2.status, 200);
    assert_eq!(
        rsp2.headers.get("X-Idempotent-Replay").map(|s| s.as_str()),
        Some("true")
    );
    let body = rsp2.body.expect("cached body");
    assert_eq!(body.get("ok").and_then(|v| v.as_bool()), Some(true));
}

#[cfg(feature = "schema-json")]
#[tokio::test]
async fn schema_guard_blocks_invalid_payload() {
    let mut registry = JsonSchemaRegistry::new();
    registry
        .register(
            "task.request",
            json!({
                "type": "object",
                "required": ["task"],
                "properties": {
                    "task": {"type": "string"}
                }
            }),
        )
        .expect("compile schema");

    let request_stage = SchemaGuardStage::new(Some(Arc::new(registry)));

    let policy = RoutePolicy::new(vec![RoutePolicySpec {
        when: MatchCond::Http {
            method: "POST".to_string(),
            path_prefix: "/v1/schema".to_string(),
        },
        bind: RouteBindingSpec {
            resource: "soul:test:schema".to_string(),
            action: "Invoke".to_string(),
            attrs_from_body: false,
            request_schema: Some("task.request".to_string()),
            response_schema: None,
        },
    }]);

    let request_stages: Vec<Box<dyn Stage>> = vec![
        Box::new(ContextInitStage::new()),
        Box::new(RoutePolicyStage::new(policy)),
        Box::new(request_stage.clone()),
        Box::new(IdempotencyStage::default()),
    ];
    let response_stages: Vec<Box<dyn sb_interceptors::stages::ResponseStage>> =
        vec![Box::new(request_stage), Box::new(ResponseStampStage)];
    let chain = InterceptorChain::new(request_stages, response_stages);

    let mut req = TestRequest::new("POST", "/v1/schema", json!({"bad": true}));
    let mut rsp = TestResponse::new();

    let res = chain
        .run_with_handler(
            InterceptContext::default(),
            &mut req,
            &mut rsp,
            |_, _| async { Ok(json!({})) },
        )
        .await;

    assert!(res.is_ok());
    assert_eq!(rsp.status, 422);
    let body = rsp.body.expect("schema error body");
    assert_eq!(
        body.get("code").and_then(|v| v.as_str()),
        Some("SCHEMA.VALIDATION_FAILED")
    );
}
