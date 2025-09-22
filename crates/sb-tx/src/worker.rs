use std::sync::Arc;
use std::time::Duration;

use sb_types::prelude::TenantId;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::config::WorkerConfig;
use crate::outbox::Dispatcher;
use crate::replay::DeadStore;
use crate::util::now_ms;

pub struct DispatcherWorkerHandle {
    stop: Option<oneshot::Sender<()>>,
    handle: JoinHandle<()>,
}

impl DispatcherWorkerHandle {
    pub async fn shutdown(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let _ = self.handle.await;
    }
}

pub fn spawn_dispatcher_worker<T, S>(
    dispatcher: Dispatcher<T, S>,
    tenant: TenantId,
    interval: Duration,
) -> DispatcherWorkerHandle
where
    T: crate::outbox::OutboxTransport + Send + Sync + 'static,
    S: crate::outbox::OutboxStore + Send + Sync + 'static,
{
    let dispatcher = Arc::new(dispatcher);
    let (tx, mut rx) = oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            tokio::select! {
                _ = &mut rx => {
                    break;
                }
                _ = ticker.tick() => {
                    let now = now_ms();
                    if let Err(err) = dispatcher.tick(&tenant, now).await {
                        eprintln!("sb-tx dispatcher worker tick error: {}", err);
                    }
                }
            }
        }
    });
    DispatcherWorkerHandle {
        stop: Some(tx),
        handle,
    }
}

pub fn spawn_dead_letter_maintenance(
    dead_store: Arc<dyn DeadStore>,
    tenant: TenantId,
    retention_ms: u64,
    interval: Duration,
) -> DispatcherWorkerHandle {
    let (tx, mut rx) = oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        if retention_ms == 0 {
            // nothing to do; just park until stop requested
            let _ = rx.await;
            return;
        }
        let retention = retention_ms as i64;
        let mut ticker = tokio::time::interval(interval);
        loop {
            tokio::select! {
                _ = &mut rx => break,
                _ = ticker.tick() => {
                    let now = now_ms();
                    let before = now - retention;
                    if let Err(err) = dead_store.purge_older_than(&tenant, before).await {
                        eprintln!("sb-tx maintenance purge error: {}", err);
                    }
                }
            }
        }
    });

    DispatcherWorkerHandle {
        stop: Some(tx),
        handle,
    }
}

pub struct TxRuntimeHandles {
    dispatcher: Option<DispatcherWorkerHandle>,
    maintenance: Option<DispatcherWorkerHandle>,
}

impl TxRuntimeHandles {
    pub async fn shutdown(self) {
        if let Some(dispatcher) = self.dispatcher {
            dispatcher.shutdown().await;
        }
        if let Some(maintenance) = self.maintenance {
            maintenance.shutdown().await;
        }
    }
}

pub fn spawn_runtime<T, S>(
    dispatcher: Option<Dispatcher<T, S>>,
    dead_store: Option<Arc<dyn DeadStore>>,
    tenant: TenantId,
    cfg: &WorkerConfig,
    default_retention_ms: u64,
) -> TxRuntimeHandles
where
    T: crate::outbox::OutboxTransport + Send + Sync + 'static,
    S: crate::outbox::OutboxStore + Send + Sync + 'static,
{
    let dispatcher_handle = if cfg.enable_dispatcher {
        dispatcher.map(|d| {
            let interval = Duration::from_millis(cfg.dispatcher_interval_ms.max(10));
            spawn_dispatcher_worker(d, tenant.clone(), interval)
        })
    } else {
        None
    };

    let retention_ms = cfg.dead_letter_retention_ms.unwrap_or(default_retention_ms);

    let maintenance_handle = if retention_ms > 0 {
        dead_store.map(|store| {
            let interval = Duration::from_millis(cfg.dead_letter_interval_ms.max(1_000));
            spawn_dead_letter_maintenance(store, tenant.clone(), retention_ms, interval)
        })
    } else {
        None
    };

    TxRuntimeHandles {
        dispatcher: dispatcher_handle,
        maintenance: maintenance_handle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backoff::RetryPolicy;
    use crate::memory::InMemoryTxStore;
    use crate::model::{NewOutboxMessage, OutboxStatus};
    use crate::observe::NoopTxMetrics;
    use crate::outbox::{Dispatcher, OutboxStore, OutboxTransport};
    use crate::prelude::NoopBudgetGuard;
    use crate::replay::DeadStore;
    use crate::util::now_ms;
    use sb_types::prelude::{Id, TenantId};
    use serde_json::json;
    use std::sync::Arc;

    struct AlwaysOk;

    #[async_trait::async_trait]
    impl OutboxTransport for AlwaysOk {
        async fn send(
            &self,
            _message: &crate::model::OutboxMessage,
        ) -> Result<(), crate::errors::TxError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn worker_dispatches_messages() {
        let store = InMemoryTxStore::default();
        let tenant = TenantId::from("worker-tenant");

        store
            .enqueue(NewOutboxMessage {
                id: Id::from("msg-worker"),
                tenant: tenant.clone(),
                envelope_id: Id::from("env-worker"),
                topic: "http://example.com".into(),
                payload: json!({"ok": true}),
                not_before: Some(now_ms()),
                dispatch_key: None,
            })
            .await
            .unwrap();

        let dispatcher = Dispatcher::new(
            AlwaysOk,
            store.clone(),
            "worker",
            3,
            1_000,
            8,
            Arc::new(RetryPolicy::default()),
            true,
            None,
            Arc::new(NoopTxMetrics),
            Arc::new(NoopBudgetGuard),
        );

        let handle = spawn_dispatcher_worker(dispatcher, tenant.clone(), Duration::from_millis(20));
        tokio::time::sleep(Duration::from_millis(80)).await;
        handle.shutdown().await;

        assert!(matches!(
            store.status("worker-tenant", "msg-worker"),
            Some(OutboxStatus::Done)
        ));
    }

    #[tokio::test]
    async fn maintenance_purges_dead_letters() {
        let store = InMemoryTxStore::default();
        let tenant = TenantId::from("tenant-maint");

        store
            .enqueue(NewOutboxMessage {
                id: Id::from("dead-1"),
                tenant: tenant.clone(),
                envelope_id: Id::from("env-dead"),
                topic: "http://example.com".into(),
                payload: json!({"dead": true}),
                not_before: Some(now_ms()),
                dispatch_key: None,
            })
            .await
            .unwrap();

        store
            .dead_letter(&tenant, &Id::from("dead-1"), Some("error".to_string()))
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(25)).await;

        let dead_store: Arc<dyn DeadStore> = store.dead.clone();
        let handle = spawn_dead_letter_maintenance(
            dead_store.clone(),
            tenant.clone(),
            10,
            Duration::from_millis(10),
        );

        tokio::time::sleep(Duration::from_millis(40)).await;
        handle.shutdown().await;

        let remaining = dead_store.list(&tenant, None, 10).await.unwrap();
        assert!(remaining.is_empty());
    }

    #[tokio::test]
    async fn runtime_handles_both_workers() {
        let store = InMemoryTxStore::default();
        let tenant = TenantId::from("tenant-runtime");

        store
            .enqueue(NewOutboxMessage {
                id: Id::from("runtime-msg"),
                tenant: tenant.clone(),
                envelope_id: Id::from("env-runtime"),
                topic: "http://example.com".into(),
                payload: json!({"runtime": true}),
                not_before: Some(now_ms()),
                dispatch_key: None,
            })
            .await
            .unwrap();

        let dispatcher = Dispatcher::new(
            AlwaysOk,
            store.clone(),
            "runtime",
            3,
            1_000,
            8,
            Arc::new(RetryPolicy::default()),
            true,
            None,
            Arc::new(NoopTxMetrics),
            Arc::new(NoopBudgetGuard),
        );

        let dead_store: Arc<dyn DeadStore> = store.dead.clone();
        let cfg = WorkerConfig {
            enable_dispatcher: true,
            dispatcher_interval_ms: 20,
            dead_letter_interval_ms: 30,
            dead_letter_retention_ms: Some(10),
        };

        let runtime = spawn_runtime(
            Some(dispatcher),
            Some(dead_store.clone()),
            tenant.clone(),
            &cfg,
            store.config.dead_letter.retention_ms,
        );

        tokio::time::sleep(Duration::from_millis(80)).await;
        runtime.shutdown().await;

        assert!(matches!(
            store.status("tenant-runtime", "runtime-msg"),
            Some(OutboxStatus::Done)
        ));
    }
}
