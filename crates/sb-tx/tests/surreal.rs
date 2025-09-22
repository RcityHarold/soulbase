#![cfg(feature = "surreal")]

use std::env;

use async_trait::async_trait;
use sb_storage::surreal::config::{SurrealConfig, SurrealCredentials, SurrealProtocol};
use sb_storage::surreal::datastore::SurrealDatastore;
use sb_tx::backoff::RetryPolicy;
use sb_tx::model::{
    DeadKind, DeadLetterRef, NewOutboxMessage, SagaDefinition, SagaState, SagaStepDef,
};
use sb_tx::observe::NoopTxMetrics;
use sb_tx::outbox::{Dispatcher, OutboxTransport};
use sb_tx::prelude::*;
use sb_tx::surreal::apply_migrations;
use sb_types::prelude::{Id, TenantId};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

const TEST_TENANT: &str = "sb-tx-surreal";

fn load_config() -> Option<SurrealConfig> {
    let endpoint = env::var("SURREAL_URL").ok()?;
    let namespace = env::var("SURREAL_NAMESPACE").unwrap_or_else(|_| "soul".into());
    let database = env::var("SURREAL_DATABASE").unwrap_or_else(|_| "base".into());
    let protocol = if endpoint.starts_with("http") {
        SurrealProtocol::Http
    } else {
        SurrealProtocol::Ws
    };

    let mut config = SurrealConfig {
        endpoint,
        namespace,
        database,
        protocol,
        credentials: None,
        max_connections: 4,
        strict: true,
    };

    if let (Ok(username), Ok(password)) =
        (env::var("SURREAL_USERNAME"), env::var("SURREAL_PASSWORD"))
    {
        config = config.with_credentials(SurrealCredentials::new(username, password));
    }

    Some(config)
}

async fn clear_schema(datastore: &SurrealDatastore) {
    let pool = datastore.pool();
    let params = sb_storage::named! { "tenant" => TEST_TENANT };
    let _ = pool
        .run_raw(
            "DELETE tx_outbox WHERE tenant = ;\n             DELETE tx_idempo WHERE tenant = ;\n             DELETE tx_saga WHERE tenant = ;\n             DELETE tx_dead_letter WHERE tenant = ;",
            &params,
        )
        .await;
}

#[derive(Clone, Default)]
struct RecordingTransport {
    calls: Arc<Mutex<Vec<String>>>,
}

impl RecordingTransport {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn recorded(&self) -> Vec<String> {
        self.calls.lock().await.clone()
    }
}

#[async_trait]
impl OutboxTransport for RecordingTransport {
    async fn send(&self, message: &OutboxMessage) -> Result<(), TxError> {
        self.calls.lock().await.push(message.id.to_string());
        Ok(())
    }
}

#[derive(Clone)]
struct LocalParticipant {
    fail_second: bool,
    attempt: std::sync::Arc<Mutex<u32>>,
}

#[async_trait]
impl SagaParticipant for LocalParticipant {
    async fn execute(&self, uri: &str, _saga: &SagaInstance) -> Result<bool, TxError> {
        let mut count = self.attempt.lock().await;
        *count += 1;
        if uri == "fail" && self.fail_second && *count == 2 {
            Ok(false)
        } else {
            Ok(true)
        }
    }

    async fn compensate(&self, _uri: &str, _saga: &SagaInstance) -> Result<bool, TxError> {
        Ok(true)
    }
}

#[tokio::test]
async fn surreal_end_to_end_flow() -> Result<(), Box<dyn std::error::Error>> {
    let Some(config) = load_config() else {
        eprintln!("skipping surreal_end_to_end_flow: SURREAL_URL not set");
        return Ok(());
    };

    let datastore = SurrealDatastore::connect(config.clone()).await?;
    apply_migrations(&datastore).await?;
    clear_schema(&datastore).await;

    let tenant = TenantId::from(TEST_TENANT);
    let tx_store = SurrealTxStore::new(datastore.clone());

    // Outbox enqueue -> lease -> ack
    let new_msg = NewOutboxMessage {
        id: Id::from("msg-1"),
        tenant: tenant.clone(),
        envelope_id: Id::from("env-1"),
        topic: "http://example".into(),
        payload: json!({"hello": "world"}),
        not_before: None,
        dispatch_key: Some("k1".into()),
    };
    let stored = tx_store.enqueue(new_msg).await?;
    assert_eq!(stored.status, OutboxStatus::Pending);

    let transport = RecordingTransport::new();
    let dispatcher = Dispatcher::new(
        transport.clone(),
        tx_store.clone(),
        "worker-surreal",
        3,
        5_000,
        4,
        Arc::new(RetryPolicy::default()),
        true,
        Some(Arc::new(tx_store.clone()) as Arc<dyn DeadStore>),
        Arc::new(NoopTxMetrics),
        Arc::new(NoopBudgetGuard),
    );

    let now = now_ms();
    dispatcher.tick(&tenant, now).await?;
    let calls = transport.recorded().await;
    assert_eq!(calls, vec!["msg-1".to_string()]);
    let fetched = sb_tx::outbox::OutboxStore::get(&tx_store, &tenant, &Id::from("msg-1"))
        .await?
        .expect("outbox exists");
    assert_eq!(fetched.status, OutboxStatus::Done);

    // Idempotency hit
    let hit = tx_store
        .check_and_put(&tenant, "req-1", "hash-1", 10_000)
        .await?;
    assert!(hit.is_none());
    tx_store.finish(&tenant, "req-1", "digest-1").await?;
    let hit_again = tx_store
        .check_and_put(&tenant, "req-1", "hash-1", 10_000)
        .await?;
    assert_eq!(hit_again, Some("digest-1".into()));

    // Saga success & compensation
    let orchestrator = SagaOrchestrator {
        store: tx_store.clone(),
        participant: LocalParticipant {
            fail_second: false,
            attempt: std::sync::Arc::new(Mutex::new(0)),
        },
    };
    let saga_def = SagaDefinition {
        name: "success".into(),
        steps: vec![
            SagaStepDef {
                name: "step-a".into(),
                action_uri: "doA".into(),
                compensate_uri: Some("undoA".into()),
                idempotent: true,
                timeout_ms: 30_000,
                retry: RetryPolicy::default(),
                concurrency_tag: None,
            },
            SagaStepDef {
                name: "step-b".into(),
                action_uri: "doB".into(),
                compensate_uri: Some("undoB".into()),
                idempotent: true,
                timeout_ms: 30_000,
                retry: RetryPolicy::default(),
                concurrency_tag: None,
            },
        ],
    };
    let saga_id = orchestrator.start(&tenant, &saga_def, None).await?;
    orchestrator.tick(&saga_id).await?;
    orchestrator.tick(&saga_id).await?;
    let saga = orchestrator
        .store
        .load(&saga_id)
        .await?
        .expect("saga exists");
    assert_eq!(saga.state, SagaState::Completed);

    let orchestrator_fail = SagaOrchestrator {
        store: tx_store.clone(),
        participant: LocalParticipant {
            fail_second: true,
            attempt: std::sync::Arc::new(Mutex::new(0)),
        },
    };
    let saga_def_fail = SagaDefinition {
        name: "fail".into(),
        steps: vec![
            SagaStepDef {
                name: "step-a".into(),
                action_uri: "doA".into(),
                compensate_uri: Some("undoA".into()),
                idempotent: true,
                timeout_ms: 30_000,
                retry: RetryPolicy::default(),
                concurrency_tag: None,
            },
            SagaStepDef {
                name: "step-b".into(),
                action_uri: "fail".into(),
                compensate_uri: Some("undoB".into()),
                idempotent: true,
                timeout_ms: 30_000,
                retry: RetryPolicy::default(),
                concurrency_tag: None,
            },
        ],
    };
    let saga_fail_id = orchestrator_fail
        .start(&tenant, &saga_def_fail, None)
        .await?;
    orchestrator_fail.tick(&saga_fail_id).await?; // Step A
    orchestrator_fail.tick(&saga_fail_id).await?; // Step B (fails)
    orchestrator_fail.tick(&saga_fail_id).await?; // Compensation
    let saga_fail = orchestrator_fail
        .store
        .load(&saga_fail_id)
        .await?
        .expect("saga fail exists");
    assert_eq!(saga_fail.state, SagaState::Cancelled);

    // Dead-letter push and replay (Outbox)
    let msg_dead = NewOutboxMessage {
        id: Id::from("msg-dead"),
        tenant: tenant.clone(),
        envelope_id: Id::from("env-dead"),
        topic: "http://example".into(),
        payload: json!({"dead": true}),
        not_before: None,
        dispatch_key: Some("k2".into()),
    };
    tx_store.enqueue(msg_dead).await?;
    let letter = tx_store
        .dead_letter(&tenant, &Id::from("msg-dead"), Some("boom".into()))
        .await?;
    tx_store.push(letter.clone()).await?;
    let listed = tx_store.list(&tenant, Some(DeadKind::Outbox), 10).await?;
    assert!(listed.iter().any(|l| l.reference.id.as_str() == "msg-dead"));
    tx_store
        .replay(&DeadLetterRef {
            kind: DeadKind::Outbox,
            id: Id::from("msg-dead"),
        })
        .await?;
    let revived = sb_tx::outbox::OutboxStore::get(&tx_store, &tenant, &Id::from("msg-dead"))
        .await?
        .expect("revived message");
    assert_eq!(revived.status, OutboxStatus::Pending);

    clear_schema(&datastore).await;
    Ok(())
}
