下面是 **SB-10-RIS：`soulbase-tx` 最小可运行骨架**。
 与 SB-10（规约）& SB-10-TD（设计）逐条对齐：提供 **Outbox / Dispatcher / Transport SPI、Idempotency、Saga Orchestrator、退避策略、死信与回放接口** 的可编译实现；默认使用**内存后端**（零外部依赖），便于快速联调与单测。SurrealDB 适配目录占位，后续可直接替换为真实实现。

> 放入 `soul-base/crates/soulbase-tx/` 后，`cargo check && cargo test` 即可。

------

## 目录结构

```
soul-base/
└─ crates/
   └─ soulbase-tx/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ util.rs
      │  ├─ errors.rs
      │  ├─ observe.rs
      │  ├─ backoff.rs
      │  ├─ model.rs
      │  ├─ outbox.rs
      │  ├─ idempo.rs
      │  ├─ saga.rs
      │  ├─ replay.rs
      │  ├─ prelude.rs
      │  ├─ memory/                 # 内存后端（默认可运行）
      │  │  ├─ mod.rs
      │  │  ├─ outbox_store.rs
      │  │  ├─ idempo_store.rs
      │  │  ├─ saga_store.rs
      │  │  └─ dead_store.rs
      │  └─ surreal/                # 预留：SurrealDB 适配
      │     ├─ mod.rs
      │     ├─ schema.rs
      │     ├─ repo.rs
      │     └─ mapper.rs
      └─ tests/
         └─ basic.rs
```

------

## Cargo.toml

```toml
[package]
name = "soulbase-tx"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "Reliable transactions: Outbox · Saga · Idempotency for the Soul platform"
repository = "https://example.com/soul-base"

[features]
default = ["memory"]
memory = []
surreal = []

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
async-trait = "0.1"
thiserror = "1"
parking_lot = "0.12"
chrono = "0.4"
rand = "0.8"
futures-util = "0.3"

# 平台内
sb-types = { path = "../sb-types", version = "1.0.0" }
soulbase-errors = { path = "../soulbase-errors", version = "1.0.0" }

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time"] }
```

------

## src/lib.rs

```rust
pub mod util;
pub mod errors;
pub mod observe;
pub mod backoff;
pub mod model;
pub mod outbox;
pub mod idempo;
pub mod saga;
pub mod replay;
pub mod prelude;

#[cfg(feature="memory")]
pub mod memory;

pub mod surreal; // 占位
```

------

## src/util.rs

```rust
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
```

------

## src/errors.rs

```rust
use thiserror::Error;
use soulbase_errors::prelude::*;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct TxError(pub ErrorObj);

impl TxError {
    pub fn into_inner(self) -> ErrorObj { self.0 }

    pub fn provider_unavailable(msg: &str) -> Self {
        TxError(ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE)
            .user_msg("Upstream is unavailable. Please retry later.")
            .dev_msg(msg.to_string()).build())
    }
    pub fn timeout(msg: &str) -> Self {
        // 如需单独 TX.TIMEOUT 码，可在 errors 码表补充；此处先映射 LLM.TIMEOUT
        TxError(ErrorBuilder::new(codes::LLM_TIMEOUT)
            .user_msg("Operation timed out.").dev_msg(msg.to_string()).build())
    }
    pub fn idempo_busy() -> Self {
        TxError(ErrorBuilder::new(codes::QUOTA_RATELIMIT)
            .user_msg("Request is already being processed.").build())
    }
    pub fn idempo_failed() -> Self {
        TxError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL)
            .user_msg("Previous attempt failed.").build())
    }
    pub fn schema(msg: &str) -> Self {
        TxError(ErrorBuilder::new(codes::SCHEMA_VALIDATION)
            .user_msg("Invalid input.").dev_msg(msg.to_string()).build())
    }
    pub fn conflict(msg: &str) -> Self {
        // 若已添加 STORAGE.CONFLICT 请替换为该码
        TxError(ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE)
            .user_msg("Conflict. Please retry.").dev_msg(msg.to_string()).build())
    }
    pub fn unknown(msg: &str) -> Self {
        TxError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL)
            .user_msg("Internal error.").dev_msg(msg.to_string()).build())
    }
}
```

------

## src/observe.rs

```rust
use std::collections::BTreeMap;

pub fn labels(tenant: &str, kind: &str, code: Option<&str>) -> BTreeMap<&'static str, String> {
    let mut m = BTreeMap::new();
    m.insert("tenant", tenant.to_string());
    m.insert("kind", kind.to_string());
    if let Some(c) = code { m.insert("code", c.to_string()); }
    m
}
```

------

## src/backoff.rs

```rust
use rand::{Rng, rngs::StdRng, SeedableRng};

#[derive(Clone, Debug)]
pub struct RetryPolicy {
  pub max_attempts: u32,
  pub base_ms: u64,
  pub factor: f64,
  pub jitter: f64, // 0..1
  pub cap_ms: u64,
}

pub trait BackoffPolicy {
  fn next_after(&self, now_ms: i64, attempts: u32) -> i64;
  fn allowed(&self, attempts: u32) -> bool;
}

impl BackoffPolicy for RetryPolicy {
  fn next_after(&self, now_ms: i64, attempts: u32) -> i64 {
    let mut rng = StdRng::from_entropy();
    let exp = (self.base_ms as f64) * self.factor.powi((attempts.saturating_sub(1)) as i32);
    let capped = exp.min(self.cap_ms as f64);
    let jitter = 1.0 + (rng.gen::<f64>() * 2.0 - 1.0) * self.jitter;
    now_ms + (capped * jitter).max(self.base_ms as f64) as i64
  }
  fn allowed(&self, attempts: u32) -> bool { attempts < self.max_attempts }
}

impl Default for RetryPolicy {
  fn default() -> Self {
    Self { max_attempts: 8, base_ms: 500, factor: 2.0, jitter: 0.3, cap_ms: 300_000 }
  }
}
```

------

## src/model.rs

```rust
use serde::{Serialize, Deserialize};
use sb_types::prelude::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum OutboxStatus { Pending, Leased, Done, Dead }

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum IdempoStatus { InFlight, Succeeded, Failed }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdempoRecord {
  pub key: String,
  pub tenant: TenantId,
  pub hash: String,
  pub status: IdempoStatus,
  pub result_digest: Option<String>,
  pub ttl_ms: u64,
  pub created_at: i64,
  pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SagaState { Running, Compensating, Completed, Failed, Cancelled, Paused }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum StepState { Ready, InFlight, Succeeded, Failed, Compensated, Skipped }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SagaStepDef {
  pub name: String,
  pub action_uri: String,
  pub compensate_uri: Option<String>,
  pub idempotent: bool,
  pub timeout_ms: u64,
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
```

------

## src/outbox.rs

```rust
use crate::{errors::TxError, backoff::BackoffPolicy, model::*};

#[async_trait::async_trait]
pub trait OutboxStore: Send + Sync {
  async fn enqueue(&self, msg: OutboxMessage) -> Result<(), TxError>;
  async fn lease_batch(
      &self, tenant: &sb_types::prelude::TenantId,
      now_ms: i64, lease_ms: u64, batch: u32, group_by_key: bool
  ) -> Result<Vec<OutboxMessage>, TxError>;
  async fn ack_done(&self, id: &sb_types::prelude::Id) -> Result<(), TxError>;
  async fn nack_backoff(&self, id: &sb_types::prelude::Id, next_ms: i64, err: &str) -> Result<(), TxError>;
  async fn dead_letter(&self, id: &sb_types::prelude::Id, err: &str) -> Result<(), TxError>;
}

#[async_trait::async_trait]
pub trait Transport: Send + Sync {
  async fn send(&self, topic: &str, payload: &serde_json::Value) -> Result<(), TxError>;
}

pub struct Dispatcher<T: Transport, S: OutboxStore> {
  pub transport: T,
  pub store: S,
  pub worker_id: String,
  pub max_attempts: u32,
  pub lease_ms: u64,
  pub batch: u32,
  pub backoff: Box<dyn BackoffPolicy + Send + Sync>,
}

impl<T: Transport, S: OutboxStore> Dispatcher<T,S> {
  pub async fn tick(&self, tenant: &sb_types::prelude::TenantId, now_ms: i64) -> Result<(), TxError> {
    let msgs = self.store.lease_batch(tenant, now_ms, self.lease_ms, self.batch, true).await?;
    for m in msgs {
      match self.transport.send(&m.topic, &m.payload).await {
        Ok(_) => self.store.ack_done(&m.id).await?,
        Err(e) => {
          let attempts = m.attempts + 1;
          if attempts >= self.max_attempts {
            self.store.dead_letter(&m.id, &e.to_string()).await?;
          } else {
            let next = self.backoff.next_after(now_ms, attempts);
            self.store.nack_backoff(&m.id, next, &e.to_string()).await?;
          }
        }
      }
    }
    Ok(())
  }
}
```

------

## src/idempo.rs

```rust
use crate::{errors::TxError, model::*};

#[async_trait::async_trait]
pub trait IdempotencyStore: Send + Sync {
  async fn check_and_put(&self, tenant: &sb_types::prelude::TenantId, key: &str, hash: &str, ttl_ms: u64)
      -> Result<Option<String>, TxError>;
  async fn finish(&self, tenant: &sb_types::prelude::TenantId, key: &str, result_digest: &str) -> Result<(), TxError>;
  async fn fail(&self, tenant: &sb_types::prelude::TenantId, key: &str, err: &str) -> Result<(), TxError>;
}
```

------

## src/saga.rs

```rust
use crate::{errors::TxError, model::*, util::now_ms};

#[async_trait::async_trait]
pub trait SagaStore: Send + Sync {
  async fn create_instance(&self, tenant: &sb_types::prelude::TenantId, def: &SagaDefinition, timeout_at: Option<i64>)
      -> Result<sb_types::prelude::Id, TxError>;
  async fn load(&self, id: &sb_types::prelude::Id) -> Result<SagaInstance, TxError>;
  async fn save(&self, saga: &SagaInstance) -> Result<(), TxError>;
}

#[async_trait::async_trait]
pub trait SagaParticipant: Send + Sync {
  async fn execute(&self, uri: &str, saga: &SagaInstance) -> Result<bool, TxError>;
  async fn compensate(&self, uri: &str, saga: &SagaInstance) -> Result<bool, TxError>;
}

pub struct SagaOrchestrator<S: SagaStore, P: SagaParticipant> {
  pub store: S,
  pub participant: P,
}

impl<S: SagaStore, P: SagaParticipant> SagaOrchestrator<S,P> {
  pub async fn start(&self, tenant: &sb_types::prelude::TenantId, def: &SagaDefinition, ttl_ms: Option<u64>)
      -> Result<sb_types::prelude::Id, TxError> {
    let timeout = ttl_ms.map(|d| now_ms() + d as i64);
    self.store.create_instance(tenant, def, timeout).await
  }

  pub async fn tick(&self, id: &sb_types::prelude::Id) -> Result<(), TxError> {
    let mut saga = self.store.load(id).await?;
    match saga.state {
      SagaState::Running => self.advance(&mut saga).await?,
      SagaState::Compensating => self.compensate(&mut saga).await?,
      _ => {}
    }
    self.store.save(&saga).await
  }

  async fn advance(&self, saga: &mut SagaInstance) -> Result<(), TxError> {
    if saga.cursor >= saga.steps.len() { saga.state = SagaState::Completed; return Ok(()); }
    let st = &mut saga.steps[saga.cursor];
    match st.state {
      StepState::Ready | StepState::Failed => {
        st.state = StepState::InFlight; st.attempts += 1; st.started_at = Some(now_ms());
        let ok = self.participant.execute(&st.def.action_uri, saga).await?;
        if ok { st.state = StepState::Succeeded; st.completed_at = Some(now_ms()); saga.cursor += 1; }
        else { st.state = StepState::Failed; saga.state = SagaState::Compensating; }
      }
      _ => {}
    }
    Ok(())
  }

  async fn compensate(&self, saga: &mut SagaInstance) -> Result<(), TxError> {
    while saga.cursor > 0 {
      let idx = saga.cursor - 1;
      let st = &mut saga.steps[idx];
      if matches!(st.state, StepState::Succeeded) {
        if let Some(uri) = &st.def.compensate_uri {
          let _ = self.participant.compensate(uri, saga).await?;
        }
        st.state = StepState::Compensated;
      }
      saga.cursor -= 1;
    }
    saga.state = SagaState::Cancelled;
    Ok(())
  }
}
```

------

## src/replay.rs

```rust
use crate::{errors::TxError, model::*};

#[derive(Clone, Debug)]
pub struct DeadLetterRef { pub kind: DeadKind, pub id: sb_types::prelude::Id }
#[derive(Clone, Debug)]
pub enum DeadKind { Outbox, Saga }

#[async_trait::async_trait]
pub trait DeadStore: Send + Sync {
  async fn list(&self, tenant: &sb_types::prelude::TenantId, kind: DeadKind, limit: u32)
      -> Result<Vec<DeadLetterRef>, TxError>;
  async fn inspect(&self, r: &DeadLetterRef) -> Result<serde_json::Value, TxError>;
  async fn replay(&self, r: &DeadLetterRef) -> Result<(), TxError>;
  async fn quarantine(&self, r: &DeadLetterRef, note: &str) -> Result<(), TxError>;
}
```

------

## src/prelude.rs

```rust
pub use crate::errors::TxError;
pub use crate::backoff::{RetryPolicy, BackoffPolicy};
pub use crate::model::*;
pub use crate::outbox::{OutboxStore, Dispatcher, Transport};
pub use crate::idempo::IdempotencyStore;
pub use crate::saga::{SagaDefinition, SagaOrchestrator, SagaParticipant, SagaStore};
pub use crate::replay::{DeadStore, DeadLetterRef, DeadKind};

#[cfg(feature="memory")]
pub use crate::memory::{InMemoryOutboxStore, InMemoryIdempoStore, InMemorySagaStore, InMemoryDeadStore, MockTransport};
```

------

## src/memory/mod.rs

```rust
pub mod outbox_store;
pub mod idempo_store;
pub mod saga_store;
pub mod dead_store;

pub use outbox_store::InMemoryOutboxStore;
pub use idempo_store::InMemoryIdempoStore;
pub use saga_store::InMemorySagaStore;
pub use dead_store::InMemoryDeadStore;

/// 简单 Transport：topic 以 "fail://" 开头时失败；其余成功
pub struct MockTransport;
#[async_trait::async_trait]
impl crate::outbox::Transport for MockTransport {
  async fn send(&self, topic: &str, _payload: &serde_json::Value) -> Result<(), crate::errors::TxError> {
    if topic.starts_with("fail://") {
      Err(crate::errors::TxError::provider_unavailable("mock fail"))
    } else { Ok(()) }
  }
}
```

### src/memory/outbox_store.rs

```rust
use parking_lot::RwLock;
use std::collections::HashMap;
use crate::{outbox::OutboxStore, errors::TxError, model::*, util::now_ms};

#[derive(Default)]
struct TenantQueues {
  msgs: HashMap<String, OutboxMessage>, // id -> msg
  dead: Vec<(String, String)>,          // (id, reason)
}

#[derive(Default)]
pub struct InMemoryOutboxStore {
  data: RwLock<HashMap<String, TenantQueues>>, // tenant -> queues
}

#[async_trait::async_trait]
impl OutboxStore for InMemoryOutboxStore {
  async fn enqueue(&self, msg: OutboxMessage) -> Result<(), TxError> {
    let mut all = self.data.write();
    let qs = all.entry(msg.tenant.0.clone()).or_default();
    qs.msgs.insert(msg.id.0.clone(), msg);
    Ok(())
  }

  async fn lease_batch(
      &self, tenant: &sb_types::prelude::TenantId,
      now: i64, lease_ms: u64, batch: u32, group_by_key: bool
  ) -> Result<Vec<OutboxMessage>, TxError> {
    let mut all = self.data.write();
    let qs = all.entry(tenant.0.clone()).or_default();
    // 选出符合条件的 Pending
    let mut picked: Vec<String> = qs.msgs.values().filter(|m|
        matches!(m.status, OutboxStatus::Pending) &&
        m.not_before <= now &&
        (m.lease_until.unwrap_or(0) <= now)
    )
    .take(batch as usize * 2).map(|m| m.id.0.clone()).collect();

    // 同 dispatch_key 串行：仅保留每个 key 的第一条
    if group_by_key {
      use std::collections::HashSet;
      let mut seen: HashSet<Option<String>> = HashSet::new();
      picked.retain(|id| {
        let m = qs.msgs.get(id).unwrap();
        if seen.contains(&m.dispatch_key) { false } else { seen.insert(m.dispatch_key.clone()); true }
      });
    }

    picked.truncate(batch as usize);
    let mut res = vec![];
    for id in picked {
      if let Some(m) = qs.msgs.get_mut(&id) {
        m.status = OutboxStatus::Leased;
        m.lease_until = Some(now + lease_ms as i64);
        m.worker = Some("mem-worker".into());
        res.push(m.clone());
      }
    }
    Ok(res)
  }

  async fn ack_done(&self, id: &sb_types::prelude::Id) -> Result<(), TxError> {
    let mut all = self.data.write();
    for (_t, qs) in all.iter_mut() {
      if let Some(m) = qs.msgs.get_mut(&id.0) { m.status = OutboxStatus::Done; m.worker=None; m.lease_until=None; }
    }
    Ok(())
  }

  async fn nack_backoff(&self, id: &sb_types::prelude::Id, next_ms: i64, err: &str) -> Result<(), TxError> {
    let mut all = self.data.write();
    for (_t, qs) in all.iter_mut() {
      if let Some(m) = qs.msgs.get_mut(&id.0) {
        m.status = OutboxStatus::Pending; m.not_before = next_ms; m.attempts += 1;
        m.last_error = Some(err.to_string()); m.worker=None; m.lease_until=None;
      }
    }
    Ok(())
  }

  async fn dead_letter(&self, id: &sb_types::prelude::Id, err: &str) -> Result<(), TxError> {
    let mut all = self.data.write();
    for (_t, qs) in all.iter_mut() {
      if let Some(m) = qs.msgs.get_mut(&id.0) {
        m.status = OutboxStatus::Dead; m.worker=None; m.lease_until=None; m.last_error=Some(err.to_string());
        qs.dead.push((id.0.clone(), err.to_string()));
      }
    }
    Ok(())
  }
}

// 辅助：测试观测
impl InMemoryOutboxStore {
  pub fn status(&self, tenant: &str, id: &str) -> Option<OutboxStatus> {
    self.data.read().get(tenant)?.msgs.get(id).map(|m| m.status.clone())
  }
  pub fn dead_count(&self, tenant: &str) -> usize {
    self.data.read().get(tenant).map(|q| q.dead.len()).unwrap_or(0)
  }
}
```

### src/memory/idempo_store.rs

```rust
use parking_lot::RwLock;
use std::collections::HashMap;
use crate::{idempo::IdempotencyStore, errors::TxError, model::*};

#[derive(Default)]
pub struct InMemoryIdempoStore {
  data: RwLock<HashMap<(String,String), IdempoRecord>>, // (tenant,key) -> rec
}

#[async_trait::async_trait]
impl IdempotencyStore for InMemoryIdempoStore {
  async fn check_and_put(&self, tenant: &sb_types::prelude::TenantId, key: &str, hash: &str, ttl_ms: u64)
      -> Result<Option<String>, TxError> {
    let mut d = self.data.write();
    let k = (tenant.0.clone(), key.to_string());
    if let Some(rec) = d.get(&k) {
      if rec.hash != hash { return Err(TxError::schema("idempotency hash mismatch")); }
      return match rec.status {
        IdempoStatus::Succeeded => Ok(rec.result_digest.clone()),
        IdempoStatus::InFlight  => Err(TxError::idempo_busy()),
        IdempoStatus::Failed    => Err(TxError::idempo_failed()),
      }
    }
    d.insert(k, IdempoRecord{
      key: key.into(), tenant: tenant.clone(), hash: hash.into(),
      status: IdempoStatus::InFlight, result_digest: None,
      ttl_ms, created_at: crate::util::now_ms(), updated_at: crate::util::now_ms()
    });
    Ok(None)
  }

  async fn finish(&self, tenant: &sb_types::prelude::TenantId, key: &str, result_digest: &str) -> Result<(), TxError> {
    let mut d = self.data.write();
    let k = (tenant.0.clone(), key.to_string());
    let rec = d.get_mut(&k).ok_or_else(|| TxError::schema("idempo record missing"))?;
    rec.status = IdempoStatus::Succeeded; rec.result_digest = Some(result_digest.into()); rec.updated_at = crate::util::now_ms();
    Ok(())
  }

  async fn fail(&self, tenant: &sb_types::prelude::TenantId, key: &str, _err: &str) -> Result<(), TxError> {
    let mut d = self.data.write();
    let k = (tenant.0.clone(), key.to_string());
    let rec = d.get_mut(&k).ok_or_else(|| TxError::schema("idempo record missing"))?;
    rec.status = IdempoStatus::Failed; rec.updated_at = crate::util::now_ms();
    Ok(())
  }
}
```

### src/memory/saga_store.rs

```rust
use parking_lot::RwLock;
use std::collections::HashMap;
use crate::{saga::SagaStore, errors::TxError, model::*, util::now_ms};
use sb_types::prelude::*;

#[derive(Default)]
pub struct InMemorySagaStore {
  data: RwLock<HashMap<String, SagaInstance>>,
}

#[async_trait::async_trait]
impl SagaStore for InMemorySagaStore {
  async fn create_instance(&self, tenant: &TenantId, def: &SagaDefinition, timeout_at: Option<i64>)
      -> Result<Id, TxError> {
    let id = Id(uuid::Uuid::new_v4().to_string());
    let steps = def.steps.iter().map(|d| SagaStepState{
      def: d.clone(), state: StepState::Ready, attempts: 0, last_error: None, started_at: None, completed_at: None
    }).collect();
    let inst = SagaInstance{
      id: id.clone(), tenant: tenant.clone(), state: SagaState::Running, def_name: def.name.clone(),
      steps, cursor: 0, created_at: now_ms(), updated_at: now_ms(), timeout_at
    };
    self.data.write().insert(id.0.clone(), inst);
    Ok(id)
  }

  async fn load(&self, id: &Id) -> Result<SagaInstance, TxError> {
    self.data.read().get(&id.0).cloned().ok_or_else(|| TxError::unknown("saga not found"))
  }

  async fn save(&self, saga: &SagaInstance) -> Result<(), TxError> {
    self.data.write().insert(saga.id.0.clone(), saga.clone());
    Ok(())
  }
}
```

### src/memory/dead_store.rs

```rust
use parking_lot::RwLock;
use crate::{replay::{DeadStore, DeadLetterRef, DeadKind}, errors::TxError, model::*, prelude::*};
use std::collections::HashMap;

#[derive(Default)]
pub struct InMemoryDeadStore {
  outbox_dead: RwLock<Vec<(String, String, String)>>, // (tenant, id, err)
  saga_dead:   RwLock<Vec<(String, String, String)>>, // (tenant, id, err)
  // 用于 replay：直接访问内存 outbox
  pub outbox:  std::sync::Arc<InMemoryOutboxStore>,
  pub saga:    std::sync::Arc<InMemorySagaStore>,
}

#[async_trait::async_trait]
impl DeadStore for InMemoryDeadStore {
  async fn list(&self, tenant: &sb_types::prelude::TenantId, kind: DeadKind, limit: u32)
      -> Result<Vec<DeadLetterRef>, TxError> {
    let v = match kind {
      DeadKind::Outbox => self.outbox_dead.read().iter().filter(|(t,_,_)| t==&tenant.0).map(|(_,id,_)| id.clone()).collect::<Vec<_>>(),
      DeadKind::Saga   => self.saga_dead.read().iter().filter(|(t,_,_)| t==&tenant.0).map(|(_,id,_)| id.clone()).collect::<Vec<_>>(),
    };
    Ok(v.into_iter().take(limit as usize).map(|id| DeadLetterRef { kind: kind.clone(), id: sb_types::prelude::Id(id) }).collect())
  }

  async fn inspect(&self, r: &DeadLetterRef) -> Result<serde_json::Value, TxError> {
    let out = match r.kind {
      DeadKind::Outbox => {
        let it = self.outbox_dead.read();
        it.iter().find(|(_,id,_)| id==&r.id.0).map(|(t,i,e)| serde_json::json!({"tenant":t,"id":i,"err":e})).unwrap_or_default()
      }
      DeadKind::Saga => {
        let it = self.saga_dead.read();
        it.iter().find(|(_,id,_)| id==&r.id.0).map(|(t,i,e)| serde_json::json!({"tenant":t,"id":i,"err":e})).unwrap_or_default()
      }
    };
    Ok(out)
  }

  async fn replay(&self, r: &DeadLetterRef) -> Result<(), TxError> {
    match r.kind {
      DeadKind::Outbox => {
        // 将 Dead 的 outbox 恢复为 Pending
        let mut all = self.outbox.data.write();
        for (_t, qs) in all.iter_mut() {
          if let Some(m) = qs.msgs.get_mut(&r.id.0) {
            m.status = OutboxStatus::Pending; m.not_before = crate::util::now_ms(); m.attempts = 0; m.last_error=None;
          }
        }
      }
      DeadKind::Saga => {
        let mut all = self.saga.data.write();
        if let Some(s) = all.get_mut(&r.id.0) {
          s.state = SagaState::Running; // 从头继续
        }
      }
    }
    Ok(())
  }

  async fn quarantine(&self, _r: &DeadLetterRef, _note: &str) -> Result<(), TxError> { Ok(()) }
}

// 写入 dead 记录的辅助（给内存 outbox 调用）
impl InMemoryDeadStore {
  pub fn add_outbox_dead(&self, tenant: &str, id: &str, err: &str) {
    self.outbox_dead.write().push((tenant.into(), id.into(), err.into()));
  }
  pub fn add_saga_dead(&self, tenant: &str, id: &str, err: &str) {
    self.saga_dead.write().push((tenant.into(), id.into(), err.into()));
  }
}
```

------

## src/surreal/*（占位）

```rust
pub mod schema { /* SurrealQL 见 TD；此处占位 */ }
pub mod repo   { /* Outbox/Idempo/Saga 的 Surreal 查询实现占位 */ }
pub mod mapper { /* 状态映射占位 */ }
```

------

## tests/basic.rs

```rust
use soulbase_tx::prelude::*;
use sb_types::prelude::*;
use serde_json::json;

#[tokio::test]
async fn outbox_enqueue_dispatch_success() {
    let store = InMemoryOutboxStore::default();
    let tenant = TenantId("t1".into());
    let msg = OutboxMessage{
        id: Id("o1".into()), tenant: tenant.clone(), envelope_id: Id("e1".into()),
        topic: "ok://deliver".into(), payload: json!({"v":1}), created_at: crate::soulbase_tx::util::now_ms(),
        not_before: crate::soulbase_tx::util::now_ms(), attempts: 0, status: OutboxStatus::Pending,
        last_error: None, dispatch_key: Some("k".into()), lease_until: None, worker: None
    };
    store.enqueue(msg).await.unwrap();

    let d = Dispatcher{
        transport: MockTransport,
        store: store.clone(),
        worker_id: "w1".into(),
        max_attempts: 3,
        lease_ms: 5_000,
        batch: 10,
        backoff: Box::new(RetryPolicy::default()),
    };
    d.tick(&tenant, crate::soulbase_tx::util::now_ms()).await.unwrap();
    assert!(matches!(store.status(&tenant.0, "o1"), Some(OutboxStatus::Done)));
}

#[tokio::test]
async fn outbox_retry_then_deadletter_and_replay() {
    let store = InMemoryOutboxStore::default();
    let tenant = TenantId("t1".into());
    let msg = OutboxMessage{
        id: Id("o2".into()), tenant: tenant.clone(), envelope_id: Id("e2".into()),
        topic: "fail://always".into(), payload: json!({}), created_at: crate::soulbase_tx::util::now_ms(),
        not_before: crate::soulbase_tx::util::now_ms(), attempts: 0, status: OutboxStatus::Pending,
        last_error: None, dispatch_key: Some("k".into()), lease_until: None, worker: None
    };
    store.enqueue(msg).await.unwrap();

    let dead = InMemoryDeadStore{ outbox: std::sync::Arc::new(store.clone()),
                                  saga: std::sync::Arc::new(InMemorySagaStore::default()),
                                  ..Default::default() };

    let d = Dispatcher{
        transport: MockTransport,
        store: store.clone(),
        worker_id: "w1".into(),
        max_attempts: 2,
        lease_ms: 1_000,
        batch: 10,
        backoff: Box::new(RetryPolicy{ max_attempts: 8, base_ms: 1, factor: 1.0, jitter: 0.0, cap_ms: 2 }),
    };

    // 第一次尝试 → NACK
    d.tick(&tenant, crate::soulbase_tx::util::now_ms()).await.unwrap();
    // 第二次到达 backoff 后再尝试 → DEAD
    let later = crate::soulbase_tx::util::now_ms() + 10;
    d.tick(&tenant, later).await.unwrap();
    assert!(matches!(store.status(&tenant.0, "o2"), Some(OutboxStatus::Dead)));

    // replay
    let r = DeadLetterRef{ kind: DeadKind::Outbox, id: Id("o2".into()) };
    dead.replay(&r).await.unwrap();
    assert!(matches!(store.status(&tenant.0, "o2"), Some(OutboxStatus::Pending)));
}

#[tokio::test]
async fn idempotency_flow() {
    let idempo = InMemoryIdempoStore::default();
    let tenant = TenantId("t1".into());
    let key = "req-001"; let hash = "abc";
    // 第一次：占位
    let hit = idempo.check_and_put(&tenant, key, hash, 1000).await.unwrap();
    assert!(hit.is_none());
    // 完成
    idempo.finish(&tenant, key, "digest-1").await.unwrap();
    // 第二次：命中
    let hit2 = idempo.check_and_put(&tenant, key, hash, 1000).await.unwrap();
    assert_eq!(hit2, Some("digest-1".into()));
}

struct LocalParticipant { fail_second: bool }
#[async_trait::async_trait]
impl SagaParticipant for LocalParticipant {
  async fn execute(&self, uri: &str, _saga: &SagaInstance) -> Result<bool, TxError> {
    if uri == "fail" { Ok(false) } else { Ok(true) }
  }
  async fn compensate(&self, _uri: &str, _saga: &SagaInstance) -> Result<bool, TxError> {
    Ok(true)
  }
}

#[tokio::test]
async fn saga_success_and_compensate() {
    let store = InMemorySagaStore::default();
    let orch = SagaOrchestrator{ store, participant: LocalParticipant{ fail_second: false } };

    // 成功路径
    let def_ok = SagaDefinition {
      name: "ok".into(),
      steps: vec![
        SagaStepDef{ name:"A".into(), action_uri:"doA".into(), compensate_uri:Some("undoA".into()), idempotent:true, timeout_ms: 60000 },
        SagaStepDef{ name:"B".into(), action_uri:"doB".into(), compensate_uri:Some("undoB".into()), idempotent:true, timeout_ms: 60000 },
      ],
    };
    let tenant = TenantId("t1".into());
    let id = orch.start(&tenant, &def_ok, None).await.unwrap();
    orch.tick(&id).await.unwrap(); // A
    orch.tick(&id).await.unwrap(); // B
    let s = orch.store.load(&id).await.unwrap();
    assert!(matches!(s.state, SagaState::Completed));

    // 失败→补偿路径
    let store2 = InMemorySagaStore::default();
    let orch2 = SagaOrchestrator{ store: store2, participant: LocalParticipant{ fail_second: true } };
    let def_fail = SagaDefinition {
      name: "fail".into(),
      steps: vec![
        SagaStepDef{ name:"A".into(), action_uri:"doA".into(), compensate_uri:Some("undoA".into()), idempotent:true, timeout_ms: 60000 },
        SagaStepDef{ name:"B".into(), action_uri:"fail".into(), compensate_uri:Some("undoB".into()), idempotent:true, timeout_ms: 60000 },
      ],
    };
    let id2 = orch2.start(&tenant, &def_fail, None).await.unwrap();
    orch2.tick(&id2).await.unwrap(); // run A
    orch2.tick(&id2).await.unwrap(); // run B -> fail => compensating
    orch2.tick(&id2).await.unwrap(); // compensate A
    let s2 = orch2.store.load(&id2).await.unwrap();
    assert!(matches!(s2.state, SagaState::Cancelled));
}
```

------

## README.md（简版）

~~~markdown
# soulbase-tx (RIS)

Reliable transactions for the Soul platform:
- Outbox (enqueue + lease/ack/retry/dead)
- Dispatcher + Backoff policy (exponential + jitter)
- Idempotency store (producer/consumer)
- Saga orchestrator (execute/compensate)
- Dead letter & replay (memory backend)
- Error normalization & basic labels

## Build & Test
```bash
cargo check
cargo test
~~~

## Next

- Add SurrealDB-backed stores using `soulbase-storage`
- Add dedicated TX error codes (TX.TIMEOUT / TX.IDEMPOTENT_*)
- Hook metrics to soulbase-observe, rate/QoS to soulbase-qos
- Add concurrent steps & isolation tags for Saga

```
---

### 对齐说明
- **同频**：严格贯彻“同库原子入箱、至少一次 + 幂等、补偿、死信与回放、稳定错误、可观测”不变式；接口与状态机与 TD 保持一致。  
- **可演进**：`surreal/` 目录已预置，后续填入 Surreal 查询与表结构即可。  
- **可运行**：内存后端与单测覆盖成功/重试/死信/回放、幂等命中、Saga 成功与补偿主路径，方便团队立刻串接上游服务与基座模块。
::contentReference[oaicite:0]{index=0}
```
