#![cfg(feature = "surreal")]

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use sb_errors::prelude::codes;
use sb_storage::errors::StorageError;
use sb_storage::prelude::{Datastore, NamedArgs};
use sb_storage::surreal::datastore::SurrealDatastore;
use sb_types::prelude::{Id, TenantId};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::a2a::{A2AHooks, NoopA2AHooks};
use crate::config::TxConfig;
use crate::errors::{TxError, TxResult};
use crate::idempo::build_record;
use crate::model::{
    DeadKind, DeadLetter, DeadLetterPayload, DeadLetterRef, IdempoRecord, IdempoStatus,
    NewOutboxMessage, OutboxMessage, SagaInstance, SagaState,
};
use crate::observe::{NoopTxMetrics, TxMetrics};
use crate::outbox::{build_dead_letter, build_outbox_message, OutboxStore};
use crate::qos::BudgetGuard;
use crate::replay::DeadStore as DeadStoreTrait;
use crate::saga::SagaStore;
use crate::util::now_ms;

use super::mapper::{
    dead_doc_from_letter, dead_letter_from_doc, idempo_doc_from_record, idempo_record_from_doc,
    json_null, outbox_doc_from_message, outbox_message_from_doc, record_id_for,
    saga_doc_from_instance, saga_instance_from_doc, DeadLetterDoc, IdempoDoc, OutboxDoc, SagaDoc,
    DEAD_TABLE, IDEMPO_TABLE, OUTBOX_TABLE, SAGA_TABLE,
};

fn map_storage_error(err: StorageError) -> TxError {
    TxError::from(err.into_inner())
}

fn map_idempo_create_error(err: StorageError) -> TxError {
    let code = err.to_public().code.to_owned();
    if code == codes::STORAGE_CONFLICT.0 {
        TxError::idempo_busy()
    } else {
        map_storage_error(err)
    }
}

fn base_args(table: &str, tenant: &TenantId, kind: &str) -> NamedArgs {
    let mut args = NamedArgs::new();
    args.insert("table".into(), json!(table));
    args.insert("tenant".into(), json!(tenant.as_ref()));
    args.insert("__kind".into(), json!(kind));
    args
}

fn decode_vec<T>(value: Option<Value>, ctx: &str) -> TxResult<Vec<T>>
where
    T: DeserializeOwned,
{
    match value {
        Some(Value::Array(arr)) => arr
            .into_iter()
            .map(|item| super::mapper::decode_row::<T>(item, ctx))
            .collect(),
        Some(other) => super::mapper::decode_row::<T>(other, ctx).map(|item| vec![item]),
        None => Ok(Vec::new()),
    }
}

async fn query_json(
    datastore: &Arc<SurrealDatastore>,
    statement: &str,
    params: &NamedArgs,
) -> Result<Option<Value>, StorageError> {
    let mut session = datastore.session().await?;
    session.query_json(statement, params).await
}

async fn execute_query(
    datastore: &Arc<SurrealDatastore>,
    statement: &str,
    params: &NamedArgs,
) -> Result<(), StorageError> {
    let mut session = datastore.session().await?;
    session.query(statement, params).await?;
    Ok(())
}

#[derive(Clone)]
pub struct SurrealOutboxStore {
    datastore: Arc<SurrealDatastore>,
}

impl SurrealOutboxStore {
    pub fn new(datastore: Arc<SurrealDatastore>) -> Self {
        Self { datastore }
    }

    async fn fetch(&self, tenant: &TenantId, id: &Id) -> TxResult<Option<OutboxDoc>> {
        let mut params = base_args(OUTBOX_TABLE, tenant, "read");
        params.insert(
            "id".into(),
            json!(record_id_for(OUTBOX_TABLE, tenant, id.as_str())),
        );
        let value = query_json(&self.datastore, SELECT_SINGLE, &params)
            .await
            .map_err(map_storage_error)?;
        super::mapper::decode_optional_row(value, "outbox fetch")
    }

    async fn try_lease(
        &self,
        tenant: &TenantId,
        doc: &OutboxDoc,
        now_ms: i64,
        lease_until: i64,
        worker_id: &str,
    ) -> TxResult<Option<OutboxDoc>> {
        let mut params = base_args(OUTBOX_TABLE, tenant, "write");
        params.insert("id".into(), json!(&doc.id));
        params.insert("lease_until".into(), json!(lease_until));
        params.insert("worker".into(), json!(worker_id));
        params.insert("now".into(), json!(now_ms));

        let value = query_json(&self.datastore, LEASE_UPDATE, &params)
            .await
            .map_err(map_storage_error)?;
        match value {
            Some(val) => super::mapper::decode_optional_row(Some(val), "outbox lease"),
            None => Ok(None),
        }
    }

    async fn select_candidates(
        &self,
        tenant: &TenantId,
        now_ms: i64,
        worker_id: &str,
        limit: usize,
    ) -> TxResult<Vec<OutboxDoc>> {
        let mut params = base_args(OUTBOX_TABLE, tenant, "read");
        params.insert("now".into(), json!(now_ms));
        params.insert("worker".into(), json!(worker_id));
        params.insert("limit".into(), json!(limit as u64));
        let value = query_json(&self.datastore, SELECT_CANDIDATES, &params)
            .await
            .map_err(map_storage_error)?;
        decode_vec(value, "outbox select")
    }
}

#[async_trait]
impl OutboxStore for SurrealOutboxStore {
    async fn enqueue(&self, message: NewOutboxMessage) -> TxResult<OutboxMessage> {
        let message = build_outbox_message(message);
        let doc = outbox_doc_from_message(&message);

        let mut params = base_args(OUTBOX_TABLE, &message.tenant, "write");
        params.insert("id".into(), json!(&doc.id));
        params.insert(
            "data".into(),
            serde_json::to_value(&doc)
                .map_err(|err| TxError::schema(format!("serialize outbox doc failed: {err}")))?,
        );

        let value = query_json(&self.datastore, CREATE_RECORD, &params)
            .await
            .map_err(map_storage_error)?
            .ok_or_else(|| TxError::unknown("outbox enqueue returned empty response"))?;

        let stored = super::mapper::decode_row::<OutboxDoc>(value, "outbox enqueue")?;
        outbox_message_from_doc(stored)
    }

    async fn lease_batch(
        &self,
        tenant: &TenantId,
        now_ms: i64,
        lease_ms: i64,
        batch: usize,
        worker_id: &str,
        group_by_key: bool,
    ) -> TxResult<Vec<OutboxMessage>> {
        let prefetch = (batch * 4).max(batch);
        let candidates = self
            .select_candidates(tenant, now_ms, worker_id, prefetch)
            .await?;

        let mut selected = Vec::with_capacity(batch);
        let mut seen_dispatch = HashSet::new();

        for doc in candidates {
            if group_by_key {
                if let Some(key) = doc.dispatch_key.as_deref() {
                    if !seen_dispatch.insert(key.to_owned()) {
                        continue;
                    }
                }
            }

            let lease_until = now_ms + lease_ms;
            if let Some(leased_doc) = self
                .try_lease(tenant, &doc, now_ms, lease_until, worker_id)
                .await?
            {
                let message = outbox_message_from_doc(leased_doc)?;
                selected.push(message);
            }

            if selected.len() >= batch {
                break;
            }
        }

        Ok(selected)
    }

    async fn ack_done(&self, tenant: &TenantId, id: &Id) -> TxResult<()> {
        let mut params = base_args(OUTBOX_TABLE, tenant, "write");
        params.insert(
            "id".into(),
            json!(record_id_for(OUTBOX_TABLE, tenant, id.as_str())),
        );
        params.insert("status".into(), json!("Done"));
        params.insert("now".into(), json!(now_ms()));
        params.insert("last_error".into(), json_null());
        params.insert("worker".into(), json_null());
        params.insert("lease_until".into(), json_null());

        query_json(&self.datastore, ACK_UPDATE, &params)
            .await
            .map_err(map_storage_error)?;
        Ok(())
    }

    async fn nack_backoff(
        &self,
        tenant: &TenantId,
        id: &Id,
        not_before: i64,
        error: Option<String>,
    ) -> TxResult<()> {
        let mut params = base_args(OUTBOX_TABLE, tenant, "write");
        params.insert(
            "id".into(),
            json!(record_id_for(OUTBOX_TABLE, tenant, id.as_str())),
        );
        params.insert("not_before".into(), json!(not_before));
        params.insert("error".into(), json!(error));
        params.insert("now".into(), json!(now_ms()));

        query_json(&self.datastore, NACK_UPDATE, &params)
            .await
            .map_err(map_storage_error)?;
        Ok(())
    }

    async fn dead_letter(
        &self,
        tenant: &TenantId,
        id: &Id,
        error: Option<String>,
    ) -> TxResult<DeadLetter> {
        let mut params = base_args(OUTBOX_TABLE, tenant, "write");
        params.insert(
            "id".into(),
            json!(record_id_for(OUTBOX_TABLE, tenant, id.as_str())),
        );
        params.insert("error".into(), json!(error));
        params.insert("now".into(), json!(now_ms()));

        let value = query_json(&self.datastore, DEAD_UPDATE, &params)
            .await
            .map_err(map_storage_error)?
            .ok_or_else(|| TxError::unknown("dead-letter update returned empty payload"))?;

        let doc = super::mapper::decode_row::<OutboxDoc>(value, "outbox dead-letter")?;
        let message = outbox_message_from_doc(doc.clone())?;
        Ok(build_dead_letter(&message, doc.last_error))
    }

    async fn heartbeat(
        &self,
        tenant: &TenantId,
        id: &Id,
        lease_until: i64,
        worker_id: &str,
    ) -> TxResult<()> {
        let mut params = base_args(OUTBOX_TABLE, tenant, "write");
        params.insert(
            "id".into(),
            json!(record_id_for(OUTBOX_TABLE, tenant, id.as_str())),
        );
        params.insert("lease_until".into(), json!(lease_until));
        params.insert("worker".into(), json!(worker_id));

        query_json(&self.datastore, HEARTBEAT_UPDATE, &params)
            .await
            .map_err(map_storage_error)?;
        Ok(())
    }

    async fn revive(&self, tenant: &TenantId, id: &Id, at: i64) -> TxResult<()> {
        let mut params = base_args(OUTBOX_TABLE, tenant, "write");
        params.insert(
            "id".into(),
            json!(record_id_for(OUTBOX_TABLE, tenant, id.as_str())),
        );
        params.insert("at".into(), json!(at));

        query_json(&self.datastore, REVIVE_UPDATE, &params)
            .await
            .map_err(map_storage_error)?;
        Ok(())
    }

    async fn get(&self, tenant: &TenantId, id: &Id) -> TxResult<Option<OutboxMessage>> {
        let doc = self.fetch(tenant, id).await?;
        match doc {
            Some(doc) => outbox_message_from_doc(doc).map(Some),
            None => Ok(None),
        }
    }
}

#[derive(Clone)]
pub struct SurrealIdempoStore {
    datastore: Arc<SurrealDatastore>,
}

impl SurrealIdempoStore {
    pub fn new(datastore: Arc<SurrealDatastore>) -> Self {
        Self { datastore }
    }

    async fn fetch(&self, tenant: &TenantId, key: &str) -> TxResult<Option<IdempoDoc>> {
        let mut params = base_args(IDEMPO_TABLE, tenant, "read");
        params.insert("id".into(), json!(record_id_for(IDEMPO_TABLE, tenant, key)));
        let value = query_json(&self.datastore, SELECT_SINGLE, &params)
            .await
            .map_err(map_storage_error)?;
        super::mapper::decode_optional_row(value, "idempotency fetch")
    }

    async fn delete(&self, tenant: &TenantId, key: &str) -> TxResult<()> {
        let mut params = base_args(IDEMPO_TABLE, tenant, "write");
        params.insert("id".into(), json!(record_id_for(IDEMPO_TABLE, tenant, key)));
        execute_query(&self.datastore, DELETE_RECORD, &params)
            .await
            .map_err(map_storage_error)?;
        Ok(())
    }
}

#[async_trait]
impl crate::idempo::IdempotencyStore for SurrealIdempoStore {
    async fn check_and_put(
        &self,
        tenant: &TenantId,
        key: &str,
        hash: &str,
        ttl_ms: u64,
    ) -> TxResult<Option<String>> {
        if let Some(doc) = self.fetch(tenant, key).await? {
            let record = idempo_record_from_doc(doc.clone())?;
            let now = now_ms();
            if now.saturating_sub(doc.updated_at) as u64 >= doc.ttl_ms {
                self.delete(tenant, key).await?;
            } else {
                if doc.hash != hash {
                    return Err(TxError::conflict("idempotency hash mismatch"));
                }
                return match record.status {
                    IdempoStatus::InFlight => Err(TxError::idempo_busy()),
                    IdempoStatus::Succeeded => Ok(record.result_digest.clone()),
                    IdempoStatus::Failed => Err(TxError::idempo_failed()),
                };
            }
        }

        let record = build_record(tenant.clone(), key, hash, ttl_ms);
        let doc = idempo_doc_from_record(&record);

        let mut params = base_args(IDEMPO_TABLE, tenant, "write");
        params.insert("id".into(), json!(&doc.id));
        params.insert(
            "data".into(),
            serde_json::to_value(&doc).map_err(|err| {
                TxError::schema(format!("serialize idempotency doc failed: {err}"))
            })?,
        );

        query_json(&self.datastore, CREATE_RECORD, &params)
            .await
            .map_err(map_idempo_create_error)?;

        Ok(None)
    }

    async fn finish(&self, tenant: &TenantId, key: &str, result_digest: &str) -> TxResult<()> {
        let mut params = base_args(IDEMPO_TABLE, tenant, "write");
        params.insert("id".into(), json!(record_id_for(IDEMPO_TABLE, tenant, key)));
        params.insert("status".into(), json!("Succeeded"));
        params.insert("result".into(), json!(result_digest));
        params.insert("now".into(), json!(now_ms()));

        query_json(&self.datastore, IDEMPO_FINISH, &params)
            .await
            .map_err(map_storage_error)?;
        Ok(())
    }

    async fn fail(&self, tenant: &TenantId, key: &str, error: Option<String>) -> TxResult<()> {
        let mut params = base_args(IDEMPO_TABLE, tenant, "write");
        params.insert("id".into(), json!(record_id_for(IDEMPO_TABLE, tenant, key)));
        params.insert("status".into(), json!("Failed"));
        params.insert("error".into(), json!(error));
        params.insert("now".into(), json!(now_ms()));

        query_json(&self.datastore, IDEMPO_FAIL, &params)
            .await
            .map_err(map_storage_error)?;
        Ok(())
    }

    async fn get(&self, tenant: &TenantId, key: &str) -> TxResult<Option<IdempoRecord>> {
        let doc = self.fetch(tenant, key).await?;
        match doc {
            Some(doc) => idempo_record_from_doc(doc).map(Some),
            None => Ok(None),
        }
    }
}

#[derive(Clone)]
pub struct SurrealSagaStore {
    datastore: Arc<SurrealDatastore>,
}

impl SurrealSagaStore {
    pub fn new(datastore: Arc<SurrealDatastore>) -> Self {
        Self { datastore }
    }
}

#[async_trait]
impl SagaStore for SurrealSagaStore {
    async fn insert(&self, saga: SagaInstance) -> TxResult<()> {
        let doc = saga_doc_from_instance(&saga);
        let mut params = base_args(SAGA_TABLE, &saga.tenant, "write");
        params.insert("id".into(), json!(&doc.id));
        params.insert(
            "data".into(),
            serde_json::to_value(&doc)
                .map_err(|err| TxError::schema(format!("serialize saga doc failed: {err}")))?,
        );

        query_json(&self.datastore, CREATE_RECORD, &params)
            .await
            .map_err(map_storage_error)?;
        Ok(())
    }

    async fn load(&self, id: &Id) -> TxResult<Option<SagaInstance>> {
        let mut params = NamedArgs::new();
        params.insert("table".into(), json!(SAGA_TABLE));
        params.insert("tenant".into(), json!("__lookup__"));
        params.insert("__kind".into(), json!("read"));
        params.insert("saga_id".into(), json!(id.as_str()));

        let value = query_json(&self.datastore, SELECT_SAGA_BY_ID, &params)
            .await
            .map_err(map_storage_error)?;
        let docs = decode_vec::<SagaDoc>(value, "saga load")?;
        if let Some(doc) = docs.into_iter().next() {
            saga_instance_from_doc(doc).map(Some)
        } else {
            Ok(None)
        }
    }

    async fn save(&self, saga: &SagaInstance) -> TxResult<()> {
        let doc = saga_doc_from_instance(saga);
        let mut params = base_args(SAGA_TABLE, &saga.tenant, "write");
        params.insert("id".into(), json!(&doc.id));
        params.insert(
            "data".into(),
            serde_json::to_value(&doc)
                .map_err(|err| TxError::schema(format!("serialize saga doc failed: {err}")))?,
        );

        query_json(&self.datastore, UPSERT_RECORD, &params)
            .await
            .map_err(map_storage_error)?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct SurrealDeadStore {
    datastore: Arc<SurrealDatastore>,
    outbox: Arc<SurrealOutboxStore>,
    saga: Arc<SurrealSagaStore>,
}

impl SurrealDeadStore {
    pub fn new(
        datastore: Arc<SurrealDatastore>,
        outbox: Arc<SurrealOutboxStore>,
        saga: Arc<SurrealSagaStore>,
    ) -> Self {
        Self {
            datastore,
            outbox,
            saga,
        }
    }
}

#[async_trait]
impl DeadStoreTrait for SurrealDeadStore {
    async fn push(&self, letter: DeadLetter) -> TxResult<()> {
        let doc = dead_doc_from_letter(&letter)?;
        let mut params = base_args(DEAD_TABLE, &letter.tenant, "write");
        params.insert("id".into(), json!(&doc.id));
        params.insert(
            "data".into(),
            serde_json::to_value(&doc).map_err(|err| {
                TxError::schema(format!("serialize dead-letter doc failed: {err}"))
            })?,
        );

        query_json(&self.datastore, UPSERT_RECORD, &params)
            .await
            .map_err(map_storage_error)?;
        Ok(())
    }

    async fn list(
        &self,
        tenant: &TenantId,
        kind: Option<DeadKind>,
        limit: usize,
    ) -> TxResult<Vec<DeadLetter>> {
        let mut params = base_args(DEAD_TABLE, tenant, "read");
        params.insert("limit".into(), json!(limit as u64));
        if let Some(kind) = kind {
            params.insert("kind".into(), json!(super::mapper::dead_kind_to_str(kind)));
        } else {
            params.insert("kind".into(), json_null());
        }
        let value = query_json(&self.datastore, LIST_DEAD, &params)
            .await
            .map_err(map_storage_error)?;
        let docs = decode_vec::<DeadLetterDoc>(value, "dead-letter list")?;
        docs.into_iter().map(dead_letter_from_doc).collect()
    }

    async fn get(&self, reference: &DeadLetterRef) -> TxResult<Option<DeadLetter>> {
        let mut params = NamedArgs::new();
        params.insert("table".into(), json!(DEAD_TABLE));
        params.insert("tenant".into(), json!("__lookup__"));
        params.insert("__kind".into(), json!("read"));
        params.insert(
            "ref_kind".into(),
            json!(super::mapper::dead_kind_to_str(reference.kind)),
        );
        params.insert("ref_id".into(), json!(reference.id.as_str()));
        let value = query_json(&self.datastore, SELECT_DEAD_BY_REFERENCE, &params)
            .await
            .map_err(map_storage_error)?;
        let doc = super::mapper::decode_optional_row::<DeadLetterDoc>(value, "dead-letter get")?;
        match doc {
            Some(doc) => dead_letter_from_doc(doc).map(Some),
            None => Ok(None),
        }
    }

    async fn remove(&self, reference: &DeadLetterRef) -> TxResult<()> {
        let mut params = NamedArgs::new();
        params.insert("table".into(), json!(DEAD_TABLE));
        params.insert("tenant".into(), json!("__lookup__"));
        params.insert("__kind".into(), json!("write"));
        params.insert(
            "ref_kind".into(),
            json!(super::mapper::dead_kind_to_str(reference.kind)),
        );
        params.insert("ref_id".into(), json!(reference.id.as_str()));
        execute_query(&self.datastore, DELETE_DEAD_BY_REFERENCE, &params)
            .await
            .map_err(map_storage_error)?;
        Ok(())
    }

    async fn replay(&self, reference: &DeadLetterRef) -> TxResult<()> {
        let Some(letter) = self.get(reference).await? else {
            return Err(TxError::unknown("dead-letter not found"));
        };

        match letter.reference.kind {
            DeadKind::Outbox => {
                self.outbox
                    .revive(&letter.tenant, &letter.reference.id, now_ms())
                    .await?;
            }
            DeadKind::Saga => {
                if let DeadLetterPayload::Saga(mut saga) = letter.payload {
                    saga.state = SagaState::Running;
                    saga.updated_at = now_ms();
                    self.saga.save(&saga).await?;
                } else {
                    return Err(TxError::schema("dead-letter payload is not saga instance"));
                }
            }
        }

        self.remove(reference).await
    }

    async fn purge_older_than(&self, tenant: &TenantId, before_epoch_ms: i64) -> TxResult<()> {
        let mut params = base_args(DEAD_TABLE, tenant, "write");
        params.insert("before".into(), json!(before_epoch_ms));
        execute_query(&self.datastore, PURGE_DEAD, &params)
            .await
            .map_err(map_storage_error)
    }
}

#[derive(Clone)]
pub struct SurrealTxStore {
    datastore: Arc<SurrealDatastore>,
    outbox: Arc<SurrealOutboxStore>,
    idempo: Arc<SurrealIdempoStore>,
    saga: Arc<SurrealSagaStore>,
    dead: Arc<SurrealDeadStore>,
    config: TxConfig,
    metrics: Arc<dyn TxMetrics>,
    qos: Arc<dyn BudgetGuard>,
    a2a: Arc<dyn A2AHooks>,
}

impl SurrealTxStore {
    pub fn new(datastore: SurrealDatastore) -> Self {
        let config = TxConfig::default();
        let budget = config.build_budget_guard();
        Self::with_config(
            datastore,
            config,
            Arc::new(NoopTxMetrics),
            budget,
            Arc::new(NoopA2AHooks),
        )
    }

    pub fn with_config(
        datastore: SurrealDatastore,
        config: TxConfig,
        metrics: Arc<dyn TxMetrics>,
        qos: Arc<dyn BudgetGuard>,
        a2a: Arc<dyn A2AHooks>,
    ) -> Self {
        let datastore = Arc::new(datastore);
        let outbox = Arc::new(SurrealOutboxStore::new(datastore.clone()));
        let idempo = Arc::new(SurrealIdempoStore::new(datastore.clone()));
        let saga = Arc::new(SurrealSagaStore::new(datastore.clone()));
        let dead = Arc::new(SurrealDeadStore::new(
            datastore.clone(),
            outbox.clone(),
            saga.clone(),
        ));
        Self {
            datastore,
            outbox,
            idempo,
            saga,
            dead,
            config,
            metrics,
            qos,
            a2a,
        }
    }

    pub fn datastore(&self) -> Arc<SurrealDatastore> {
        self.datastore.clone()
    }

    pub fn outbox_store(&self) -> Arc<SurrealOutboxStore> {
        self.outbox.clone()
    }

    pub fn idempo_store(&self) -> Arc<SurrealIdempoStore> {
        self.idempo.clone()
    }

    pub fn saga_store(&self) -> Arc<SurrealSagaStore> {
        self.saga.clone()
    }

    pub fn dead_store(&self) -> Arc<SurrealDeadStore> {
        self.dead.clone()
    }
}

#[async_trait]
impl OutboxStore for SurrealTxStore {
    async fn enqueue(&self, message: NewOutboxMessage) -> TxResult<OutboxMessage> {
        let stored = self.outbox.enqueue(message).await?;
        self.metrics
            .record_outbox_enqueue(&stored.tenant, &stored.topic);
        self.qos.on_enqueue(&stored)?;
        Ok(stored)
    }

    async fn lease_batch(
        &self,
        tenant: &TenantId,
        now_ms: i64,
        lease_ms: i64,
        batch: usize,
        worker_id: &str,
        group_by_key: bool,
    ) -> TxResult<Vec<OutboxMessage>> {
        let grouping = if self.config.outbox.group_by_dispatch_key {
            group_by_key
        } else {
            false
        };
        self.outbox
            .lease_batch(tenant, now_ms, lease_ms, batch, worker_id, grouping)
            .await
    }

    async fn ack_done(&self, tenant: &TenantId, id: &Id) -> TxResult<()> {
        self.outbox.ack_done(tenant, id).await
    }

    async fn nack_backoff(
        &self,
        tenant: &TenantId,
        id: &Id,
        not_before: i64,
        error: Option<String>,
    ) -> TxResult<()> {
        self.outbox
            .nack_backoff(tenant, id, not_before, error)
            .await
    }

    async fn dead_letter(
        &self,
        tenant: &TenantId,
        id: &Id,
        error: Option<String>,
    ) -> TxResult<DeadLetter> {
        let letter = self.outbox.dead_letter(tenant, id, error).await?;
        if let DeadLetterPayload::Outbox(message) = &letter.payload {
            self.metrics
                .record_outbox_dead_letter(tenant, &message.topic, letter.error.as_deref());
            self.qos
                .on_dead_letter(tenant, message, letter.error.as_deref())?;
            self.a2a
                .on_outbox_dead_letter(tenant, message, letter.error.as_deref())?;
        }
        Ok(letter)
    }

    async fn heartbeat(
        &self,
        tenant: &TenantId,
        id: &Id,
        lease_until: i64,
        worker_id: &str,
    ) -> TxResult<()> {
        self.outbox
            .heartbeat(tenant, id, lease_until, worker_id)
            .await
    }

    async fn revive(&self, tenant: &TenantId, id: &Id, at: i64) -> TxResult<()> {
        self.outbox.revive(tenant, id, at).await
    }

    async fn get(&self, tenant: &TenantId, id: &Id) -> TxResult<Option<OutboxMessage>> {
        self.outbox.get(tenant, id).await
    }
}

#[async_trait]
impl crate::idempo::IdempotencyStore for SurrealTxStore {
    async fn check_and_put(
        &self,
        tenant: &TenantId,
        key: &str,
        hash: &str,
        ttl_ms: u64,
    ) -> TxResult<Option<String>> {
        let result = self.idempo.check_and_put(tenant, key, hash, ttl_ms).await;
        match &result {
            Ok(Some(_)) => self
                .metrics
                .record_idempotency(tenant, IdempoStatus::Succeeded),
            Ok(None) => self
                .metrics
                .record_idempotency(tenant, IdempoStatus::InFlight),
            Err(err) => {
                let code = err.as_public().code;
                if code == codes::TX_IDEMPOTENT_BUSY.0 {
                    self.metrics
                        .record_idempotency(tenant, IdempoStatus::InFlight);
                } else if code == codes::TX_IDEMPOTENT_LAST_FAILED.0 {
                    self.metrics
                        .record_idempotency(tenant, IdempoStatus::Failed);
                }
            }
        }
        result
    }

    async fn finish(&self, tenant: &TenantId, key: &str, result_digest: &str) -> TxResult<()> {
        let res = self.idempo.finish(tenant, key, result_digest).await;
        if res.is_ok() {
            self.metrics
                .record_idempotency(tenant, IdempoStatus::Succeeded);
        }
        res
    }

    async fn fail(&self, tenant: &TenantId, key: &str, error: Option<String>) -> TxResult<()> {
        let res = self.idempo.fail(tenant, key, error).await;
        if res.is_ok() {
            self.metrics
                .record_idempotency(tenant, IdempoStatus::Failed);
        }
        res
    }

    async fn get(&self, tenant: &TenantId, key: &str) -> TxResult<Option<IdempoRecord>> {
        self.idempo.get(tenant, key).await
    }
}

#[async_trait]
impl SagaStore for SurrealTxStore {
    async fn insert(&self, saga: SagaInstance) -> TxResult<()> {
        self.saga.insert(saga).await
    }

    async fn load(&self, id: &Id) -> TxResult<Option<SagaInstance>> {
        self.saga.load(id).await
    }

    async fn save(&self, saga: &SagaInstance) -> TxResult<()> {
        self.saga.save(saga).await
    }
}

#[async_trait]
impl DeadStoreTrait for SurrealTxStore {
    async fn push(&self, letter: DeadLetter) -> TxResult<()> {
        self.dead.push(letter.clone()).await?;
        if let DeadLetterPayload::Outbox(message) = &letter.payload {
            self.metrics.record_outbox_dead_letter(
                &letter.tenant,
                &message.topic,
                letter.error.as_deref(),
            );
        }
        Ok(())
    }

    async fn list(
        &self,
        tenant: &TenantId,
        kind: Option<DeadKind>,
        limit: usize,
    ) -> TxResult<Vec<DeadLetter>> {
        self.dead.list(tenant, kind, limit).await
    }

    async fn get(&self, reference: &DeadLetterRef) -> TxResult<Option<DeadLetter>> {
        self.dead.get(reference).await
    }

    async fn remove(&self, reference: &DeadLetterRef) -> TxResult<()> {
        self.dead.remove(reference).await
    }

    async fn replay(&self, reference: &DeadLetterRef) -> TxResult<()> {
        let tenant = self.dead.get(reference).await?.map(|letter| letter.tenant);
        self.dead.replay(reference).await?;
        if let Some(tenant) = tenant {
            self.metrics.record_dead_replay(&tenant, reference.kind);
            self.a2a.on_outbox_replay(&tenant, &reference.id)?;
        }
        Ok(())
    }

    async fn purge_older_than(&self, tenant: &TenantId, before_epoch_ms: i64) -> TxResult<()> {
        self.dead.purge_older_than(tenant, before_epoch_ms).await
    }
}

const CREATE_RECORD: &str = "CREATE type::thing($table, $id) CONTENT $data RETURN AFTER";
const UPSERT_RECORD: &str = "UPDATE type::thing($table, $id) CONTENT $data RETURN AFTER";
const DELETE_RECORD: &str = "DELETE type::thing($table, $id)";
const SELECT_SINGLE: &str = "SELECT * FROM type::thing($table, $id) WHERE tenant = $tenant LIMIT 1";

const SELECT_CANDIDATES: &str = r#"
    SELECT * FROM tx_outbox
    WHERE tenant = $tenant
      AND status IN ["Pending", "Leased"]
      AND not_before <= $now
      AND (status = "Pending" OR lease_until <= $now OR worker = $worker OR lease_until IS NONE)
    ORDER BY not_before, created_at
    LIMIT $limit
"#;

const LEASE_UPDATE: &str = r#"
    UPDATE type::thing($table, $id)
    SET status = "Leased",
        worker = $worker,
        lease_until = $lease_until
    WHERE tenant = $tenant
      AND status IN ["Pending", "Leased"]
      AND not_before <= $now
      AND (status = "Pending" OR lease_until <= $now OR worker = $worker OR lease_until IS NONE)
    RETURN AFTER
"#;

const ACK_UPDATE: &str = r#"
    UPDATE type::thing($table, $id)
    SET status = $status,
        worker = $worker,
        lease_until = $lease_until,
        last_error = $last_error,
        attempts = attempts,
        not_before = not_before
    WHERE tenant = $tenant
    RETURN AFTER
"#;

const NACK_UPDATE: &str = r#"
    UPDATE type::thing($table, $id)
    SET status = "Pending",
        worker = NULL,
        lease_until = NULL,
        last_error = $error,
        not_before = $not_before,
        attempts = attempts + 1
    WHERE tenant = $tenant
    RETURN AFTER
"#;

const DEAD_UPDATE: &str = r#"
    UPDATE type::thing($table, $id)
    SET status = "Dead",
        worker = NULL,
        lease_until = NULL,
        last_error = $error,
        attempts = attempts + 1
    WHERE tenant = $tenant
    RETURN AFTER
"#;

const HEARTBEAT_UPDATE: &str = r#"
    UPDATE type::thing($table, $id)
    SET lease_until = $lease_until
    WHERE tenant = $tenant AND worker = $worker
    RETURN AFTER
"#;

const REVIVE_UPDATE: &str = r#"
    UPDATE type::thing($table, $id)
    SET status = "Pending",
        worker = NULL,
        lease_until = NULL,
        last_error = NULL,
        attempts = 0,
        not_before = $at
    WHERE tenant = $tenant
    RETURN AFTER
"#;

const IDEMPO_FINISH: &str = r#"
    UPDATE type::thing($table, $id)
    SET status = $status,
        result_digest = $result,
        last_error = NULL,
        updated_at = $now
    WHERE tenant = $tenant
    RETURN AFTER
"#;

const IDEMPO_FAIL: &str = r#"
    UPDATE type::thing($table, $id)
    SET status = $status,
        last_error = $error,
        updated_at = $now
    WHERE tenant = $tenant
    RETURN AFTER
"#;

const LIST_DEAD: &str = r#"
    SELECT * FROM tx_dead_letter
    WHERE tenant = $tenant
      AND ($kind IS NONE OR reference_kind = $kind)
    ORDER BY occurred_at DESC
    LIMIT $limit
"#;

const SELECT_DEAD_BY_REFERENCE: &str = r#"
    SELECT * FROM tx_dead_letter
    WHERE reference_kind = $ref_kind AND reference_id = $ref_id
    LIMIT 1
"#;

const DELETE_DEAD_BY_REFERENCE: &str = r#"
    DELETE tx_dead_letter
    WHERE reference_kind = $ref_kind AND reference_id = $ref_id
"#;

const SELECT_SAGA_BY_ID: &str = r#"
    SELECT * FROM tx_saga WHERE saga_id = $saga_id LIMIT 1
"#;

const PURGE_DEAD: &str = r#"
    DELETE tx_dead_letter
    WHERE tenant = $tenant AND occurred_at < $before
"#;

pub async fn apply_migrations(datastore: &SurrealDatastore) -> TxResult<()> {
    for stmt in super::schema::migrations() {
        datastore
            .pool()
            .run_raw(stmt, &NamedArgs::default())
            .await
            .map_err(map_storage_error)?;
    }
    Ok(())
}
