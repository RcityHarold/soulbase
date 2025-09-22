use sb_errors::prelude::*;
use serde_json::json;

#[test]
fn build_and_render_public() {
    let err = ErrorBuilder::new(codes::AUTH_UNAUTHENTICATED)
        .user_msg("请先登录。")
        .dev_msg("missing bearer token")
        .meta_kv("tenant", json!("tenantA"))
        .correlation("req-123")
        .build();

    let public = err.to_public();
    assert_eq!(public.code, "AUTH.UNAUTHENTICATED");
    assert_eq!(public.message, "请先登录。");
    assert_eq!(public.correlation_id.as_deref(), Some("req-123"));

    let audit = err.to_audit();
    assert_eq!(audit.kind, "Auth");
    assert_eq!(audit.http_status, 401);

    let lbl = labels(&err);
    assert_eq!(lbl.get("code").unwrap(), "AUTH.UNAUTHENTICATED");
    assert_eq!(lbl.get("tenant").unwrap(), "\"tenantA\"");
}

#[test]
fn registry_contains_recent_codes() {
    let tx_timeout = spec_of(codes::TX_TIMEOUT).unwrap();
    assert_eq!(tx_timeout.kind, ErrorKind::Timeout);
    assert_eq!(tx_timeout.http_status, 504);
    assert_eq!(tx_timeout.retryable, RetryClass::Transient);

    let consent = spec_of(codes::A2A_CONSENT_REQUIRED).unwrap();
    assert_eq!(consent.kind, ErrorKind::A2AError);
    assert_eq!(consent.http_status, 428);
    assert_eq!(consent.retryable, RetryClass::Permanent);

    let sandbox = spec_of(codes::SANDBOX_CAPABILITY_BLOCKED).unwrap();
    assert_eq!(sandbox.kind, ErrorKind::Sandbox);
    assert_eq!(sandbox.http_status, 403);
    assert_eq!(sandbox.retryable, RetryClass::None);
}
