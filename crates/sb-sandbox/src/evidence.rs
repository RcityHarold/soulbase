use crate::model::{Budget, Capability, DataDigest, Profile, SideEffect, SideEffectRecord};
use chrono::{DateTime, Utc};
use sb_types::prelude::{Id, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Ok,
    Denied,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EvidenceEvent {
    Begin(BeginEvidence),
    End(EndEvidence),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BeginEvidence {
    pub envelope_id: Id,
    pub tenant: TenantId,
    pub subject_id: Id,
    pub tool_name: String,
    pub call_id: Id,
    pub profile_hash: String,
    pub capability: String,
    pub declared_side_effects: Vec<SideEffect>,
    pub safety: String,
    #[serde(default)]
    pub inputs_digest: Option<DataDigest>,
    #[serde(default)]
    pub policy_hash: Option<String>,
    #[serde(default)]
    pub config_version: Option<String>,
    #[serde(default)]
    pub config_hash: Option<String>,
    pub started_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EndEvidence {
    pub envelope_id: Id,
    pub tenant: TenantId,
    pub subject_id: Id,
    pub tool_name: String,
    pub call_id: Id,
    pub profile_hash: String,
    #[serde(default)]
    pub policy_hash: Option<String>,
    #[serde(default)]
    pub config_version: Option<String>,
    #[serde(default)]
    pub config_hash: Option<String>,
    pub finished_at: DateTime<Utc>,
    pub status: EvidenceStatus,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub inputs_digest: Option<DataDigest>,
    #[serde(default)]
    pub outputs_digest: Option<DataDigest>,
    #[serde(default)]
    pub side_effects: Vec<SideEffectRecord>,
    pub budget_used: Budget,
    pub duration_ms: i64,
}

pub struct EvidenceBuilder {
    profile: Profile,
    capability: Capability,
    envelope_id: Id,
    started_at: DateTime<Utc>,
}

impl EvidenceBuilder {
    pub fn new(profile: Profile, capability: Capability, envelope_id: Id) -> Self {
        Self {
            profile,
            capability,
            envelope_id,
            started_at: Utc::now(),
        }
    }

    pub fn begin(&self, inputs_digest: Option<DataDigest>) -> EvidenceEvent {
        EvidenceEvent::Begin(BeginEvidence {
            envelope_id: self.envelope_id.clone(),
            tenant: self.profile.tenant.clone(),
            subject_id: self.profile.subject_id.clone(),
            tool_name: self.profile.tool_name.clone(),
            call_id: self.profile.call_id.clone(),
            profile_hash: self.profile.profile_hash.clone(),
            capability: self.capability.describe(),
            declared_side_effects: self.profile.side_effects.clone(),
            safety: format!("{:?}", self.profile.safety),
            inputs_digest,
            policy_hash: self.profile.policy_hash.clone(),
            config_version: self.profile.config_version.clone(),
            config_hash: self.profile.config_hash.clone(),
            started_at: self.started_at,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn end(
        &self,
        status: EvidenceStatus,
        error_code: Option<String>,
        inputs_digest: Option<DataDigest>,
        outputs_digest: Option<DataDigest>,
        side_effects: Vec<SideEffectRecord>,
        budget_used: Budget,
    ) -> EvidenceEvent {
        let finished_at = Utc::now();
        let duration = finished_at.signed_duration_since(self.started_at);
        EvidenceEvent::End(EndEvidence {
            envelope_id: self.envelope_id.clone(),
            tenant: self.profile.tenant.clone(),
            subject_id: self.profile.subject_id.clone(),
            tool_name: self.profile.tool_name.clone(),
            call_id: self.profile.call_id.clone(),
            profile_hash: self.profile.profile_hash.clone(),
            policy_hash: self.profile.policy_hash.clone(),
            config_version: self.profile.config_version.clone(),
            config_hash: self.profile.config_hash.clone(),
            finished_at,
            status,
            error_code,
            inputs_digest,
            outputs_digest,
            side_effects,
            budget_used,
            duration_ms: duration.num_milliseconds(),
        })
    }
}

pub fn digest_value(value: &Value) -> DataDigest {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    DataDigest::sha256(&bytes)
}

pub fn digest_bytes(bytes: &[u8]) -> DataDigest {
    DataDigest::sha256(bytes)
}
