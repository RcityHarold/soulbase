#![cfg(feature = "surreal")]

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

use sb_storage::model::make_record_id;
use sb_types::prelude::{Id, TenantId};

use crate::errors::{TxError, TxResult};
use crate::model::{
    DeadKind, DeadLetter, DeadLetterPayload, DeadLetterRef, IdempoRecord, IdempoStatus,
    OutboxMessage, OutboxStatus, SagaInstance, SagaState, SagaStepState,
};

pub const OUTBOX_TABLE: &str = "tx_outbox";
pub const IDEMPO_TABLE: &str = "tx_idempo";
pub const SAGA_TABLE: &str = "tx_saga";
pub const DEAD_TABLE: &str = "tx_dead_letter";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutboxDoc {
    pub id: String,
    pub tenant: String,
    pub message_id: String,
    pub envelope_id: String,
    pub topic: String,
    pub payload: Value,
    pub created_at: i64,
    pub not_before: i64,
    pub attempts: u32,
    pub status: String,
    pub last_error: Option<String>,
    pub dispatch_key: Option<String>,
    pub lease_until: Option<i64>,
    pub worker: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdempoDoc {
    pub id: String,
    pub tenant: String,
    pub key: String,
    pub hash: String,
    pub status: String,
    pub result_digest: Option<String>,
    pub last_error: Option<String>,
    pub ttl_ms: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SagaDoc {
    pub id: String,
    pub tenant: String,
    pub saga_id: String,
    pub state: String,
    pub def_name: String,
    pub steps: Vec<SagaStepState>,
    pub cursor: usize,
    pub created_at: i64,
    pub updated_at: i64,
    pub timeout_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeadLetterDoc {
    pub id: String,
    pub tenant: String,
    pub reference_kind: String,
    pub reference_id: String,
    pub payload: Value,
    pub error: Option<String>,
    pub occurred_at: i64,
}

pub fn outbox_doc_from_message(message: &OutboxMessage) -> OutboxDoc {
    OutboxDoc {
        id: make_record_id(OUTBOX_TABLE, &message.tenant, message.id.as_str()),
        tenant: message.tenant.as_ref().to_owned(),
        message_id: message.id.as_str().to_owned(),
        envelope_id: message.envelope_id.as_str().to_owned(),
        topic: message.topic.clone(),
        payload: message.payload.clone(),
        created_at: message.created_at,
        not_before: message.not_before,
        attempts: message.attempts,
        status: outbox_status_to_str(message.status).to_string(),
        last_error: message.last_error.clone(),
        dispatch_key: message.dispatch_key.clone(),
        lease_until: message.lease_until,
        worker: message.worker.clone(),
    }
}

pub fn outbox_message_from_doc(doc: OutboxDoc) -> TxResult<OutboxMessage> {
    let status = outbox_status_from_str(&doc.status)
        .ok_or_else(|| TxError::schema(format!("unknown outbox status: {}", doc.status)))?;
    Ok(OutboxMessage {
        id: Id::from(doc.message_id),
        tenant: TenantId::from(doc.tenant),
        envelope_id: Id::from(doc.envelope_id),
        topic: doc.topic,
        payload: doc.payload,
        created_at: doc.created_at,
        not_before: doc.not_before,
        attempts: doc.attempts,
        status,
        last_error: doc.last_error,
        dispatch_key: doc.dispatch_key,
        lease_until: doc.lease_until,
        worker: doc.worker,
    })
}

pub fn idempo_doc_from_record(record: &IdempoRecord) -> IdempoDoc {
    IdempoDoc {
        id: make_record_id(IDEMPO_TABLE, &record.tenant, &record.key),
        tenant: record.tenant.as_ref().to_owned(),
        key: record.key.clone(),
        hash: record.hash.clone(),
        status: idempo_status_to_str(record.status).to_string(),
        result_digest: record.result_digest.clone(),
        last_error: record.last_error.clone(),
        ttl_ms: record.ttl_ms,
        created_at: record.created_at,
        updated_at: record.updated_at,
    }
}

pub fn idempo_record_from_doc(doc: IdempoDoc) -> TxResult<IdempoRecord> {
    let status = idempo_status_from_str(&doc.status)
        .ok_or_else(|| TxError::schema(format!("unknown idempotency status: {}", doc.status)))?;
    Ok(IdempoRecord {
        key: doc.key,
        tenant: TenantId::from(doc.tenant),
        hash: doc.hash,
        status,
        result_digest: doc.result_digest,
        last_error: doc.last_error,
        ttl_ms: doc.ttl_ms,
        created_at: doc.created_at,
        updated_at: doc.updated_at,
    })
}

pub fn saga_doc_from_instance(instance: &SagaInstance) -> SagaDoc {
    SagaDoc {
        id: make_record_id(SAGA_TABLE, &instance.tenant, instance.id.as_str()),
        tenant: instance.tenant.as_ref().to_owned(),
        saga_id: instance.id.as_str().to_owned(),
        state: saga_state_to_str(instance.state).to_string(),
        def_name: instance.def_name.clone(),
        steps: instance.steps.clone(),
        cursor: instance.cursor,
        created_at: instance.created_at,
        updated_at: instance.updated_at,
        timeout_at: instance.timeout_at,
    }
}

pub fn saga_instance_from_doc(doc: SagaDoc) -> TxResult<SagaInstance> {
    let state = saga_state_from_str(&doc.state)
        .ok_or_else(|| TxError::schema(format!("unknown saga state: {}", doc.state)))?;
    Ok(SagaInstance {
        id: Id::from(doc.saga_id),
        tenant: TenantId::from(doc.tenant),
        state,
        def_name: doc.def_name,
        steps: doc.steps,
        cursor: doc.cursor,
        created_at: doc.created_at,
        updated_at: doc.updated_at,
        timeout_at: doc.timeout_at,
    })
}

pub fn dead_doc_from_letter(letter: &DeadLetter) -> TxResult<DeadLetterDoc> {
    Ok(DeadLetterDoc {
        id: make_record_id(
            DEAD_TABLE,
            &letter.tenant,
            &dead_suffix(letter.reference.kind, &letter.reference.id),
        ),
        tenant: letter.tenant.as_ref().to_owned(),
        reference_kind: dead_kind_to_str(letter.reference.kind).to_string(),
        reference_id: letter.reference.id.as_str().to_owned(),
        payload: serde_json::to_value(&letter.payload).map_err(|err| {
            TxError::schema(format!("serialize dead-letter payload failed: {err}"))
        })?,
        error: letter.error.clone(),
        occurred_at: letter.occurred_at,
    })
}

pub fn dead_letter_from_doc(doc: DeadLetterDoc) -> TxResult<DeadLetter> {
    let kind = dead_kind_from_str(&doc.reference_kind).ok_or_else(|| {
        TxError::schema(format!("unknown dead-letter kind: {}", doc.reference_kind))
    })?;
    let payload: DeadLetterPayload = serde_json::from_value(doc.payload)
        .map_err(|err| TxError::schema(format!("deserialize dead-letter payload failed: {err}")))?;
    Ok(DeadLetter {
        reference: DeadLetterRef {
            kind,
            id: Id::from(doc.reference_id),
        },
        tenant: TenantId::from(doc.tenant),
        error: doc.error,
        occurred_at: doc.occurred_at,
        payload,
    })
}

pub fn record_id_for(table: &str, tenant: &TenantId, suffix: &str) -> String {
    make_record_id(table, tenant, suffix)
}

pub fn json_null() -> Value {
    json!(null)
}

pub fn decode_row<T>(value: Value, ctx: &str) -> TxResult<T>
where
    T: DeserializeOwned,
{
    match value {
        Value::Array(arr) => {
            let item = arr
                .into_iter()
                .next()
                .ok_or_else(|| TxError::unknown(format!("{ctx}: empty response")))?;
            serde_json::from_value(item).map_err(|err| {
                TxError::schema(format!("{ctx}: deserialize array element failed: {err}"))
            })
        }
        other => serde_json::from_value(other)
            .map_err(|err| TxError::schema(format!("{ctx}: deserialize failed: {err}"))),
    }
}

pub fn decode_optional_row<T>(value: Option<Value>, ctx: &str) -> TxResult<Option<T>>
where
    T: DeserializeOwned,
{
    match value {
        Some(Value::Array(arr)) if arr.is_empty() => Ok(None),
        Some(other) => decode_row(other, ctx).map(Some),
        None => Ok(None),
    }
}

fn outbox_status_to_str(status: OutboxStatus) -> &'static str {
    match status {
        OutboxStatus::Pending => "Pending",
        OutboxStatus::Leased => "Leased",
        OutboxStatus::Done => "Done",
        OutboxStatus::Dead => "Dead",
    }
}

fn outbox_status_from_str(input: &str) -> Option<OutboxStatus> {
    match input {
        "Pending" => Some(OutboxStatus::Pending),
        "Leased" => Some(OutboxStatus::Leased),
        "Done" => Some(OutboxStatus::Done),
        "Dead" => Some(OutboxStatus::Dead),
        _ => None,
    }
}

fn idempo_status_to_str(status: IdempoStatus) -> &'static str {
    match status {
        IdempoStatus::InFlight => "InFlight",
        IdempoStatus::Succeeded => "Succeeded",
        IdempoStatus::Failed => "Failed",
    }
}

fn idempo_status_from_str(input: &str) -> Option<IdempoStatus> {
    match input {
        "InFlight" => Some(IdempoStatus::InFlight),
        "Succeeded" => Some(IdempoStatus::Succeeded),
        "Failed" => Some(IdempoStatus::Failed),
        _ => None,
    }
}

fn saga_state_to_str(state: SagaState) -> &'static str {
    match state {
        SagaState::Running => "Running",
        SagaState::Compensating => "Compensating",
        SagaState::Completed => "Completed",
        SagaState::Failed => "Failed",
        SagaState::Cancelled => "Cancelled",
        SagaState::Paused => "Paused",
    }
}

fn saga_state_from_str(input: &str) -> Option<SagaState> {
    match input {
        "Running" => Some(SagaState::Running),
        "Compensating" => Some(SagaState::Compensating),
        "Completed" => Some(SagaState::Completed),
        "Failed" => Some(SagaState::Failed),
        "Cancelled" => Some(SagaState::Cancelled),
        "Paused" => Some(SagaState::Paused),
        _ => None,
    }
}

pub(crate) fn dead_kind_to_str(kind: DeadKind) -> &'static str {
    match kind {
        DeadKind::Outbox => "Outbox",
        DeadKind::Saga => "Saga",
    }
}

pub(crate) fn dead_kind_from_str(input: &str) -> Option<DeadKind> {
    match input {
        "Outbox" => Some(DeadKind::Outbox),
        "Saga" => Some(DeadKind::Saga),
        _ => None,
    }
}

pub(crate) fn dead_suffix(kind: DeadKind, id: &Id) -> String {
    format!("{}-{}", dead_kind_to_str(kind).to_lowercase(), id.as_str())
}
