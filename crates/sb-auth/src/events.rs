use sb_types::prelude::{Envelope, Id, Subject, SubjectKind, TenantId, Timestamp};
use serde::{Deserialize, Serialize};
use serde_json;

use crate::model::{Action, Decision, ResourceUrn};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthDecisionEvent {
    pub subject_id: String,
    pub tenant: String,
    pub resource: String,
    pub action: String,
    pub allow: bool,
}

pub fn wrap_decision(
    subject: &Subject,
    resource: &ResourceUrn,
    action: &Action,
    decision: &Decision,
) -> Envelope<AuthDecisionEvent> {
    let event = AuthDecisionEvent {
        subject_id: subject.subject_id.0.clone(),
        tenant: subject.tenant.0.clone(),
        resource: resource.0.clone(),
        action: format_action(action),
        allow: decision.allow,
    };
    let envelope_id = Id(format!("auth-{}-{}", event.subject_id, resource.0));
    let produced_at = Timestamp(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
    );
    let actor = Subject {
        kind: SubjectKind::Service,
        subject_id: Id("sb-auth".into()),
        tenant: TenantId("global".into()),
        claims: serde_json::Map::new(),
    };
    Envelope::new(
        envelope_id,
        produced_at,
        "auth".to_string(),
        actor,
        "1.0.0",
        event,
    )
}

fn format_action(action: &Action) -> String {
    match action {
        Action::Read => "read",
        Action::Write => "write",
        Action::Invoke => "invoke",
        Action::List => "list",
        Action::Admin => "admin",
        Action::Configure => "configure",
    }
    .to_string()
}
