#![cfg(feature = "surreal")]

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use tokio::sync::Semaphore;

use crate::errors::{StorageError, StorageResult};
use crate::observe::{NoopStorageMetrics, StorageMetrics};
use crate::spi::health::HealthInfo;
use crate::spi::query::NamedArgs;
use crate::spi::{Datastore, QueryResult, Session, Tx};
use crate::surreal::binder::{bind_tenant, filtered_bindings};
use crate::surreal::config::{SurrealConfig, SurrealCredentials, SurrealProtocol};
use crate::surreal::errors::map_surreal_error;
use crate::surreal::observe::SurrealMetricsProxy;
use sb_types::prelude::TenantId;
use serde_json::Value;

use surrealdb::engine::remote::http::Client as HttpClient;
use surrealdb::engine::remote::http::Http;
use surrealdb::engine::remote::ws::Client as WsClient;
use surrealdb::engine::remote::ws::Ws;
use surrealdb::opt::auth::Root;
use surrealdb::{Response, Surreal};

pub struct SurrealDatastore {
    pool: Arc<SurrealPool>,
    metrics: Arc<dyn StorageMetrics>,
    observer: SurrealMetricsProxy,
    config: SurrealConfig,
}

impl Clone for SurrealDatastore {
    fn clone(&self) -> Self {
        Self {
            pool: Arc::clone(&self.pool),
            metrics: Arc::clone(&self.metrics),
            observer: self.observer.clone(),
            config: self.config.clone(),
        }
    }
}

impl SurrealDatastore {
    pub async fn connect(config: SurrealConfig) -> StorageResult<Self> {
        let pool = SurrealPool::connect(&config).await?;
        let metrics: Arc<dyn StorageMetrics> = Arc::new(NoopStorageMetrics);
        let observer = SurrealMetricsProxy::new(metrics.clone());
        Ok(Self {
            pool: Arc::new(pool),
            metrics,
            observer,
            config,
        })
    }

    pub fn with_metrics(mut self, metrics: Arc<dyn StorageMetrics>) -> Self {
        self.observer = SurrealMetricsProxy::new(metrics.clone());
        self.metrics = metrics;
        self
    }

    pub fn metrics(&self) -> Arc<dyn StorageMetrics> {
        Arc::clone(&self.metrics)
    }

    pub fn pool(&self) -> Arc<SurrealPool> {
        Arc::clone(&self.pool)
    }

    pub fn config(&self) -> &SurrealConfig {
        &self.config
    }
}

#[async_trait]
impl Datastore for SurrealDatastore {
    async fn session(&self) -> StorageResult<Box<dyn Session>> {
        Ok(Box::new(SurrealSession {
            pool: self.pool(),
            metrics: self.observer.clone(),
            strict: self.config.strict,
        }))
    }

    async fn health(&self) -> StorageResult<HealthInfo> {
        self.pool.health().await
    }

    fn metrics(&self) -> &dyn StorageMetrics {
        self.metrics.as_ref()
    }
}

pub struct SurrealPool {
    client: SurrealClient,
    semaphore: Semaphore,
    config: SurrealConfig,
}

impl SurrealPool {
    async fn connect(config: &SurrealConfig) -> StorageResult<Self> {
        match config.protocol {
            SurrealProtocol::Ws => {
                let client = Self::connect_ws(config).await?;
                Ok(Self {
                    client: SurrealClient::Ws(Arc::new(client)),
                    semaphore: Semaphore::new(config.max_connections),
                    config: config.clone(),
                })
            }
            SurrealProtocol::Http => {
                let client = Self::connect_http(config).await?;
                Ok(Self {
                    client: SurrealClient::Http(Arc::new(client)),
                    semaphore: Semaphore::new(config.max_connections),
                    config: config.clone(),
                })
            }
        }
    }

    async fn connect_ws(config: &SurrealConfig) -> StorageResult<Surreal<WsClient>> {
        let db = Surreal::new::<Ws>(config.endpoint.as_str())
            .await
            .map_err(map_surreal_error)?;
        Self::authenticate(&db, config).await?;
        db.use_ns(&config.namespace)
            .use_db(&config.database)
            .await
            .map_err(map_surreal_error)?;
        Ok(db)
    }

    async fn connect_http(config: &SurrealConfig) -> StorageResult<Surreal<HttpClient>> {
        let db = Surreal::new::<Http>(config.endpoint.as_str())
            .await
            .map_err(map_surreal_error)?;
        Self::authenticate(&db, config).await?;
        db.use_ns(&config.namespace)
            .use_db(&config.database)
            .await
            .map_err(map_surreal_error)?;
        Ok(db)
    }

    async fn authenticate<E>(db: &Surreal<E>, config: &SurrealConfig) -> StorageResult<()>
    where
        E: surrealdb::Connection,
    {
        if let Some(SurrealCredentials { username, password }) = &config.credentials {
            db.signin(Root { username, password })
                .await
                .map_err(map_surreal_error)?;
        }
        Ok(())
    }

    pub async fn health(&self) -> StorageResult<HealthInfo> {
        let params = NamedArgs::default();
        match self.run_raw("INFO FOR DB", &params).await {
            Ok(_) => Ok(HealthInfo::healthy()),
            Err(err) => Ok(HealthInfo::unhealthy(err.to_string())),
        }
    }

    pub async fn run_raw(&self, statement: &str, params: &NamedArgs) -> StorageResult<Response> {
        let bind = filtered_bindings(params);
        let permit = self.semaphore.acquire().await.expect("semaphore poisoned");
        let result = match &self.client {
            SurrealClient::Ws(client) => client.query(statement).bind(bind).await,
            SurrealClient::Http(client) => client.query(statement).bind(bind).await,
        };
        drop(permit);
        result.map_err(map_surreal_error)
    }
}

enum SurrealClient {
    Ws(Arc<Surreal<WsClient>>),
    Http(Arc<Surreal<HttpClient>>),
}

pub struct SurrealSession {
    pool: Arc<SurrealPool>,
    metrics: SurrealMetricsProxy,
    strict: bool,
}

#[async_trait]
impl Session for SurrealSession {
    async fn begin(&mut self) -> StorageResult<Box<dyn Tx>> {
        SurrealTx::begin(self.pool.clone(), self.metrics.clone(), self.strict).await
    }

    async fn query(&mut self, statement: &str, params: &NamedArgs) -> StorageResult<QueryResult> {
        let (tenant, table, kind) = extract_meta(params, self.strict)?;
        let args = bind_tenant(params.clone(), &tenant)?;

        let start = Instant::now();
        let _response = self.pool.run_raw(statement, &args).await?;
        let latency = start.elapsed();
        self.metrics
            .record_request(&tenant, table, kind, None, 0, 0, latency);
        Ok(QueryResult::new(0, 0))
    }

    async fn query_json(
        &mut self,
        statement: &str,
        params: &NamedArgs,
    ) -> StorageResult<Option<Value>> {
        let (tenant, table, kind) = extract_meta(params, self.strict)?;
        let args = bind_tenant(params.clone(), &tenant)?;

        let start = Instant::now();
        let mut response = self.pool.run_raw(statement, &args).await?;
        let latency = start.elapsed();
        let value = response
            .take::<Option<Value>>(0)
            .map_err(map_surreal_error)?;
        let rows = match &value {
            Some(Value::Array(arr)) => arr.len() as u64,
            Some(_) => 1,
            None => 0,
        };
        self.metrics
            .record_request(&tenant, table, kind, None, rows, 0, latency);
        Ok(value)
    }
}

pub struct SurrealTx {
    pool: Arc<SurrealPool>,
    metrics: SurrealMetricsProxy,
    active: bool,
    strict: bool,
    last_tenant: Option<TenantId>,
}

impl SurrealTx {
    pub async fn begin(
        pool: Arc<SurrealPool>,
        metrics: SurrealMetricsProxy,
        strict: bool,
    ) -> StorageResult<Box<dyn Tx>> {
        let mut tx = Self {
            pool,
            metrics,
            active: false,
            strict,
            last_tenant: None,
        };
        tx.start().await?;
        Ok(Box::new(tx))
    }

    async fn start(&mut self) -> StorageResult<()> {
        let params = NamedArgs::default();
        self.pool.run_raw("BEGIN TRANSACTION", &params).await?;
        self.active = true;
        Ok(())
    }
}

#[async_trait]
impl Tx for SurrealTx {
    async fn execute(&mut self, statement: &str, params: &NamedArgs) -> StorageResult<QueryResult> {
        let (tenant, table, kind) = extract_meta(params, self.strict)?;
        let args = bind_tenant(params.clone(), &tenant)?;
        self.last_tenant = Some(tenant.clone());

        let start = Instant::now();
        let _response = self.pool.run_raw(statement, &args).await?;
        let latency = start.elapsed();
        self.metrics
            .record_request(&tenant, table, kind, None, 0, 0, latency);
        Ok(QueryResult::new(0, 0))
    }

    async fn commit(mut self: Box<Self>) -> StorageResult<()> {
        if self.active {
            let params = NamedArgs::default();
            self.pool.run_raw("COMMIT TRANSACTION", &params).await?;
            self.active = false;
        }
        Ok(())
    }

    async fn rollback(mut self: Box<Self>) -> StorageResult<()> {
        if self.active {
            let params = NamedArgs::default();
            self.pool.run_raw("CANCEL TRANSACTION", &params).await?;
            self.active = false;
            if let Some(tenant) = &self.last_tenant {
                self.metrics.record_tx_rollback(tenant);
            }
        }
        Ok(())
    }
}

fn extract_meta<'a>(
    params: &'a NamedArgs,
    strict: bool,
) -> StorageResult<(TenantId, &'a str, &'a str)> {
    let tenant = match params.get("tenant") {
        Some(value) => value
            .as_str()
            .map(TenantId::from)
            .ok_or_else(|| StorageError::schema("tenant parameter must be a string"))?,
        None if strict => {
            return Err(StorageError::schema(
                "missing tenant parameter for strict Surreal session",
            ))
        }
        None => TenantId::from("__system__"),
    };

    let table = params
        .get("table")
        .and_then(|v| v.as_str())
        .unwrap_or("__raw__");
    let kind = params
        .get("__kind")
        .and_then(|v| v.as_str())
        .unwrap_or("read");

    Ok((tenant, table, kind))
}
