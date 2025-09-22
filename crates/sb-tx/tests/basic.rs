use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use sb_tx::backoff::RetryPolicy;
use sb_tx::memory::{InMemoryIdempoStore, InMemorySagaStore, InMemoryTxStore};
use sb_tx::model::{
    DeadKind, DeadLetterRef, NewOutboxMessage, OutboxMessage, OutboxStatus, SagaDefinition,
    SagaInstance, SagaState, SagaStepDef,
};
use sb_tx::observe::NoopTxMetrics;
use sb_tx::outbox::{Dispatcher, OutboxTransport};
use sb_tx::prelude::*;
use sb_tx::util;
use sb_types::prelude::{Id, TenantId};

struct AlwaysOk;

#[async_trait]
impl OutboxTransport for AlwaysOk {
    async fn send(&self, _message: &OutboxMessage) -> Result<(), TxError> {
        Ok(())
    }
}

struct FailFirst {
    target: String,
    flag: Mutex<bool>,
}

#[async_trait]
impl OutboxTransport for FailFirst {
    async fn send(&self, message: &OutboxMessage) -> Result<(), TxError> {
        let mut guard = self.flag.lock();
        if message.id.as_str() == self.target && !*guard {
            *guard = true;
            Err(TxError::provider_unavailable("simulated failure"))
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn dispatcher_success_flow() {
    let store = InMemoryTxStore::default();
    let tenant = TenantId::from("tenant-a");

    store
        .enqueue(NewOutboxMessage {
            id: Id::from("o1"),
            tenant: tenant.clone(),
            envelope_id: Id::from("env-1"),
            topic: "http://example".into(),
            payload: serde_json::json!({"test": true}),
            not_before: None,
            dispatch_key: None,
        })
        .await
        .unwrap();

    let dispatcher = Dispatcher::new(
        AlwaysOk,
        store.clone(),
        "worker-1",
        3,
        1_000,
        10,
        Arc::new(RetryPolicy::default()),
        true,
        None,
        Arc::new(NoopTxMetrics),
        Arc::new(NoopBudgetGuard),
    );

    dispatcher.tick(&tenant, util::now_ms()).await.unwrap();

    assert!(matches!(
        store.status("tenant-a", "o1"),
        Some(OutboxStatus::Done)
    ));
}

#[tokio::test]
async fn dispatcher_dead_letter_and_replay() {
    let store = InMemoryTxStore::default();
    let tenant = TenantId::from("tenant-b");

    store
        .enqueue(NewOutboxMessage {
            id: Id::from("o2"),
            tenant: tenant.clone(),
            envelope_id: Id::from("env-2"),
            topic: "http://example".into(),
            payload: serde_json::json!({"boom": true}),
            not_before: None,
            dispatch_key: None,
        })
        .await
        .unwrap();

    let dead_arc: Arc<dyn DeadStore> = store.dead.clone();

    let dispatcher = Dispatcher::new(
        FailFirst {
            target: "o2".into(),
            flag: Mutex::new(false),
        },
        store.clone(),
        "worker-2",
        1,
        1_000,
        10,
        Arc::new(RetryPolicy::default()),
        true,
        Some(dead_arc.clone()),
        Arc::new(NoopTxMetrics),
        Arc::new(NoopBudgetGuard),
    );

    dispatcher.tick(&tenant, util::now_ms()).await.unwrap();
    assert!(matches!(
        store.status("tenant-b", "o2"),
        Some(OutboxStatus::Dead)
    ));

    let reference = DeadLetterRef {
        kind: DeadKind::Outbox,
        id: Id::from("o2"),
    };

    dead_arc.replay(&reference).await.unwrap();
    assert!(matches!(
        store.status("tenant-b", "o2"),
        Some(OutboxStatus::Pending)
    ));
}

#[tokio::test]
async fn idempotency_flow() {
    let store = InMemoryIdempoStore::default();
    let tenant = TenantId::from("tenant-c");
    let key = "req-1";
    let hash = "hash";

    let first = store
        .check_and_put(&tenant, key, hash, 10_000)
        .await
        .unwrap();
    assert!(first.is_none());

    store.finish(&tenant, key, "digest-1").await.unwrap();

    let hit = store
        .check_and_put(&tenant, key, hash, 10_000)
        .await
        .unwrap();
    assert_eq!(hit, Some("digest-1".to_string()));
}

struct LocalParticipant {
    fail_second: bool,
}

#[async_trait]
impl SagaParticipant for LocalParticipant {
    async fn execute(&self, uri: &str, _saga: &SagaInstance) -> Result<bool, TxError> {
        if uri == "fail" && self.fail_second {
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
async fn saga_success_and_compensation() {
    let store = InMemorySagaStore::default();
    let orchestrator = SagaOrchestrator {
        store: store.clone(),
        participant: LocalParticipant { fail_second: false },
    };
    let tenant = TenantId::from("tenant-d");

    let def_ok = SagaDefinition {
        name: "ok".into(),
        steps: vec![
            SagaStepDef {
                name: "A".into(),
                action_uri: "doA".into(),
                compensate_uri: Some("undoA".into()),
                idempotent: true,
                timeout_ms: 10_000,
                retry: RetryPolicy::default(),
                concurrency_tag: None,
            },
            SagaStepDef {
                name: "B".into(),
                action_uri: "doB".into(),
                compensate_uri: Some("undoB".into()),
                idempotent: true,
                timeout_ms: 10_000,
                retry: RetryPolicy::default(),
                concurrency_tag: None,
            },
        ],
    };

    let id = orchestrator.start(&tenant, &def_ok, None).await.unwrap();
    orchestrator.tick(&id).await.unwrap();
    orchestrator.tick(&id).await.unwrap();
    let saga = orchestrator.store.load(&id).await.unwrap().unwrap();
    assert_eq!(saga.state, SagaState::Completed);

    let orchestrator_fail = SagaOrchestrator {
        store: store.clone(),
        participant: LocalParticipant { fail_second: true },
    };

    let def_fail = SagaDefinition {
        name: "fail".into(),
        steps: vec![
            SagaStepDef {
                name: "A".into(),
                action_uri: "doA".into(),
                compensate_uri: Some("undoA".into()),
                idempotent: true,
                timeout_ms: 10_000,
                retry: RetryPolicy::default(),
                concurrency_tag: None,
            },
            SagaStepDef {
                name: "B".into(),
                action_uri: "fail".into(),
                compensate_uri: Some("undoB".into()),
                idempotent: true,
                timeout_ms: 10_000,
                retry: RetryPolicy::default(),
                concurrency_tag: None,
            },
        ],
    };

    let id_fail = orchestrator_fail
        .start(&tenant, &def_fail, None)
        .await
        .unwrap();
    orchestrator_fail.tick(&id_fail).await.unwrap();
    orchestrator_fail.tick(&id_fail).await.unwrap();
    orchestrator_fail.tick(&id_fail).await.unwrap();
    let saga_fail = orchestrator_fail
        .store
        .load(&id_fail)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saga_fail.state, SagaState::Cancelled);
}
