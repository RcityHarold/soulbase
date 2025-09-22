use crate::model::KeyPath;
use crate::model::{Checksum, SnapshotVersion};
use sb_types::prelude::{Envelope, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigUpdateEvent {
    pub from_version: Option<SnapshotVersion>,
    pub to_version: SnapshotVersion,
    pub checksum: Checksum,
    pub changed_keys: Vec<KeyPath>,
    pub issued_at: Timestamp,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigErrorEvent {
    pub phase: String,
    pub message: String,
}

pub fn wrap_update(event: ConfigUpdateEvent) -> Envelope<ConfigUpdateEvent> {
    use sb_types::prelude::*;
    let envelope_id = Id(format!("cfg-{}", event.to_version.0));
    let produced_at = event.issued_at;
    let actor = Subject::new(
        SubjectKind::Service,
        Id("sb-config".into()),
        TenantId("global".into()),
    );
    Envelope::new(
        envelope_id,
        produced_at,
        "config".to_string(),
        actor,
        "1.0.0",
        event,
    )
}
