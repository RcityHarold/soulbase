use sb_types::prelude::*;

fn make_actor() -> Subject {
    Subject {
        kind: SubjectKind::User,
        subject_id: Id("user_123".into()),
        tenant: TenantId("tenantA".into()),
        claims: Default::default(),
    }
}

#[test]
fn envelope_validation_succeeds() {
    let envelope = Envelope::new(
        Id("env_1".into()),
        Timestamp(1_726_000_000_000),
        "tenantA:conv_1".into(),
        make_actor(),
        "1.0.0",
        serde_json::json!({"hello": "world"}),
    )
    .with_correlation(CorrelationId("corr".into()))
    .with_causation(CausationId("cause".into()));

    assert!(envelope.validate().is_ok());
}

#[test]
fn invalid_semver_is_rejected() {
    let envelope = Envelope::new(
        Id("env_1".into()),
        Timestamp(1_726_000_000_000),
        "tenantA:conv_1".into(),
        make_actor(),
        "not-a-version",
        (),
    );

    let err = envelope
        .validate()
        .expect_err("expected validation failure");
    assert!(matches!(err, ValidateError::InvalidSemVer(_)));
}

#[test]
fn scope_requires_resource_and_action() {
    let mut scope = Scope::new("tool:browser", "");
    assert_eq!(
        scope.validate(),
        Err(ValidateError::EmptyField("scope.action"))
    );

    scope.action = "invoke".into();
    assert_eq!(scope.validate(), Ok(()));
}

#[test]
fn consent_validates_all_scopes() {
    let invalid_scope = Scope {
        resource: "".into(),
        action: "invoke".into(),
        attrs: Default::default(),
    };
    let consent = Consent {
        scopes: vec![invalid_scope],
        expires_at: None,
        purpose: None,
    };

    assert_eq!(
        consent.validate(),
        Err(ValidateError::EmptyField("scope.resource"))
    );
}
