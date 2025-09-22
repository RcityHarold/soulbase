use serde::{Deserialize, Serialize};

use sb_types::prelude::{Id, TenantId};

use crate::backoff::RetryPolicy;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutboxStatus {
    Pending,
    Leased,
    Done,
    Dead,
}

impl OutboxStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Dead)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutboxMessage {
    pub id: Id,
    pub tenant: TenantId,
    pub envelope_id: Id,
    pub topic: String,
    pub payload: serde_json::Value,
    pub created_at: i64,
    pub not_before: i64,
    pub attempts: u32,
    pub status: OutboxStatus,
    pub last_error: Option<String>,
    pub dispatch_key: Option<String>,
    pub lease_until: Option<i64>,
    pub worker: Option<String>,
}

#[derive(Clone, Debug)]
pub struct NewOutboxMessage {
    pub id: Id,
    pub tenant: TenantId,
    pub envelope_id: Id,
    pub topic: String,
    pub payload: serde_json::Value,
    pub not_before: Option<i64>,
    pub dispatch_key: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdempoStatus {
    InFlight,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdempoRecord {
    pub key: String,
    pub tenant: TenantId,
    pub hash: String,
    pub status: IdempoStatus,
    pub result_digest: Option<String>,
    pub last_error: Option<String>,
    pub ttl_ms: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SagaState {
    Running,
    Compensating,
    Completed,
    Failed,
    Cancelled,
    Paused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepState {
    Ready,
    InFlight,
    Succeeded,
    Failed,
    Compensated,
    Skipped,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SagaStepDef {
    pub name: String,
    pub action_uri: String,
    pub compensate_uri: Option<String>,
    pub idempotent: bool,
    pub timeout_ms: u64,
    pub retry: RetryPolicy,
    pub concurrency_tag: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SagaDefinition {
    pub name: String,
    pub steps: Vec<SagaStepDef>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SagaStepState {
    pub def: SagaStepDef,
    pub state: StepState,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
}

impl SagaStepState {
    pub fn new(def: SagaStepDef) -> Self {
        Self {
            def,
            state: StepState::Ready,
            attempts: 0,
            last_error: None,
            started_at: None,
            completed_at: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SagaInstance {
    pub id: Id,
    pub tenant: TenantId,
    pub state: SagaState,
    pub def_name: String,
    pub steps: Vec<SagaStepState>,
    pub cursor: usize,
    pub created_at: i64,
    pub updated_at: i64,
    pub timeout_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeadKind {
    Outbox,
    Saga,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeadLetterRef {
    pub kind: DeadKind,
    pub id: Id,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DeadLetterPayload {
    Outbox(OutboxMessage),
    Saga(SagaInstance),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeadLetter {
    pub reference: DeadLetterRef,
    pub tenant: TenantId,
    pub error: Option<String>,
    pub occurred_at: i64,
    pub payload: DeadLetterPayload,
}

impl DeadLetter {
    pub fn kind(&self) -> DeadKind {
        self.reference.kind
    }
}
