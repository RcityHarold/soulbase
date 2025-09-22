# 文档 SB-10-TD：`soulbase-tx` 技术设计（Outbox · Saga · Idempotency）

> 对应规约：SB-10
>  目标：给出 **Outbox/Saga/Idempotency** 的接口与状态机、存储表结构（SurrealDB 映射）、退避策略、死信与回放协议，以及与 `soulbase-storage` 的对接。
>  语言：Rust 接口草案 + SurrealQL 结构；与 **types / errors / storage / interceptors / observe / qos / config** 同频。

------

## 1. Crate 结构与 Feature 规划

```
soulbase-tx/
  src/
    lib.rs
    model.rs            # OutboxMessage / SagaInstance / SagaStep / IdempoRecord / enums
    outbox.rs           # OutboxStore + enqueue_in_tx / Dispatcher / Transport SPI
    saga.rs             # SagaOrchestrator / SagaDefinition / Participant SPI
    idempo.rs           # IdempotencyStore（producer/consumer）
    backoff.rs          # 退避策略（exponential + jitter + policy）
    replay.rs           # 死信 & 回放协议（inspect / replay / quarantine）
    surreal/            # SurrealDB 映射：表/索引/查询模板（基于 soulbase-storage）
      schema.rs
      repo.rs
      mapper.rs
    errors.rs           # TX.* / 映射到 soulbase-errors
    observe.rs          # 指标与标签
    prelude.rs
```

**Features**

- `transport-http` / `transport-bus`（HTTP/消息总线投递器）
- `surreal`（默认开启：落 SurrealDB）
- `migrate`（生成/执行表结构与索引）
- `qos` / `observe`（速率/指标）

------

## 2. 核心数据模型（`model.rs`）

### 2.1 Outbox

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum OutboxStatus { Pending, Leased, Done, Dead }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct OutboxMessage {
  pub id: sb_types::Id,
  pub tenant: sb_types::TenantId,
  pub envelope_id: sb_types::Id,         // 追踪链
  pub topic: String,                            // http:<url> / bus:<topic> / custom:<name>
  pub payload: serde_json::Value,               // 仅必要数据；大对象应为引用
  pub created_at: i64,                          // ms epoch
  pub not_before: i64,                          // 下一次可尝试时间
  pub attempts: u32,
  pub status: OutboxStatus,
  pub last_error: Option<String>,
  pub dispatch_key: Option<String>,             // 并发串行化键（同 key 串行）
  pub lease_until: Option<i64>,                 // 租约截止（反重入）
  pub worker: Option<String>,                   // 当前租约持有者
}
```

### 2.2 Idempotency

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum IdempoStatus { InFlight, Succeeded, Failed }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct IdempoRecord {
  pub key: String,
  pub tenant: sb_types::TenantId,
  pub hash: String,               // 请求摘要（防止错误 key 复用）
  pub status: IdempoStatus,
  pub result_digest: Option<String>,
  pub ttl_ms: u64,
  pub created_at: i64,
  pub updated_at: i64,
}
```

### 2.3 Saga

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum SagaState { Running, Compensating, Completed, Failed, Cancelled, Paused }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum StepState { Ready, InFlight, Succeeded, Failed, Compensated, Skipped }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SagaStepDef {
  pub name: String,
  pub action_uri: String,         // http://..., bus:topic, fn:local_name
  pub compensate_uri: Option<String>,
  pub idempotent: bool,
  pub timeout_ms: u64,
  pub retry: RetryPolicy,         // 见 backoff.rs
  pub concurrency_tag: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SagaDefinition {
  pub name: String,
  pub steps: Vec<SagaStepDef>,    // 序列或多分支并行（RIS 以序列为主；并行以 tag 控制）
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SagaInstance {
  pub id: sb_types::Id,
  pub tenant: sb_types::TenantId,
  pub state: SagaState,
  pub def_name: String,
  pub steps: Vec<SagaStepState>,
  pub cursor: usize,              // 下一步索引
  pub created_at: i64,
  pub updated_at: i64,
  pub timeout_at: Option<i64>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SagaStepState {
  pub def: SagaStepDef,
  pub state: StepState,
  pub attempts: u32,
  pub last_error: Option<String>,
  pub started_at: Option<i64>,
  pub completed_at: Option<i64>,
}
```

------

## 3. SPI 接口

### 3.1 OutboxStore / Dispatcher / Transport（`outbox.rs`）

```rust
#[async_trait::async_trait]
pub trait OutboxStore: Send + Sync {
  // 同事务入库：通过 storage::Tx 扩展方法实现（见 §7 Surreal 映射）
  async fn enqueue(&self, msg: OutboxMessage) -> Result<(), TxError>;

  // 工作者租约：按 not_before & dispatch_key 抽取批次，设置 lease_until & worker
  async fn lease_batch(
      &self,
      tenant: &sb_types::TenantId,
      now_ms: i64,
      lease_ms: u64,
      batch: u32,
      group_by_key: bool
  ) -> Result<Vec<OutboxMessage>, TxError>;

  async fn ack_done(&self, id: &sb_types::Id) -> Result<(), TxError>;
  async fn nack_backoff(&self, id: &sb_types::Id, next_ms: i64, err: &str) -> Result<(), TxError>;
  async fn dead_letter(&self, id: &sb_types::Id, err: &str) -> Result<(), TxError>;
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
  pub async fn tick(&self, tenant: &sb_types::TenantId, now_ms: i64) -> Result<(), TxError> {
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

### 3.2 IdempotencyStore（`idempo.rs`）

```rust
#[async_trait::async_trait]
pub trait IdempotencyStore: Send + Sync {
  /// 若 key 不存在则写入 InFlight（含 hash/ttl），返回 None
  /// 若存在且 hash 相同：
  ///   - Succeeded: 返回 Some(result_digest)
  ///   - InFlight:  返回 Err(TxError::idempo_busy())
  ///   - Failed:    返回 Err(TxError::idempo_failed())
  async fn check_and_put(&self, tenant: &sb_types::TenantId, key: &str, hash: &str, ttl_ms: u64)
      -> Result<Option<String>, TxError>;

  /// 写入 Succeeded 并记录结果摘要（公共视图摘要；原文禁止落库存证）
  async fn finish(&self, tenant: &sb_types::TenantId, key: &str, result_digest: &str) -> Result<(), TxError>;

  /// 写入 Failed
  async fn fail(&self, tenant: &sb_types::TenantId, key: &str, err: &str) -> Result<(), TxError>;
}
```

### 3.3 SagaOrchestrator / Participant（`saga.rs`）

```rust
#[async_trait::async_trait]
pub trait SagaStore: Send + Sync {
  async fn create_instance(&self, tenant: &sb_types::TenantId, def: &SagaDefinition, timeout_at: Option<i64>)
      -> Result<sb_types::Id, TxError>;
  async fn load(&self, id: &sb_types::Id) -> Result<SagaInstance, TxError>;
  async fn save(&self, saga: &SagaInstance) -> Result<(), TxError>;
}

#[async_trait::async_trait]
pub trait SagaParticipant: Send + Sync {
  /// 执行 step；返回 Ok(false)=失败可重试，Ok(true)=成功；Err=致命失败
  async fn execute(&self, uri: &str, saga: &SagaInstance) -> Result<bool, TxError>;
  async fn compensate(&self, uri: &str, saga: &SagaInstance) -> Result<bool, TxError>;
}

pub struct SagaOrchestrator<S: SagaStore, P: SagaParticipant> {
  pub store: S,
  pub participant: P,
}

impl<S: SagaStore, P: SagaParticipant> SagaOrchestrator<S,P> {
  pub async fn start(&self, tenant: &sb_types::TenantId, def: &SagaDefinition, ttl_ms: Option<u64>)
      -> Result<sb_types::Id, TxError> {
    let timeout = ttl_ms.map(|d| now_ms() + d as i64);
    self.store.create_instance(tenant, def, timeout).await
  }

  pub async fn tick(&self, id: &sb_types::Id) -> Result<(), TxError> {
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
        else if st.def.retry.allowed(st.attempts) { st.state = StepState::Failed; } else { saga.state = SagaState::Compensating; }
      }
      _ => {}
    }
    Ok(())
  }

  async fn compensate(&self, saga: &mut SagaInstance) -> Result<(), TxError> {
    while saga.cursor > 0 {
      let idx = saga.cursor - 1;
      let st = &mut saga.steps[idx];
      if st.state == StepState::Succeeded {
        if let Some(uri) = &st.def.compensate_uri {
          let ok = self.participant.compensate(uri, saga).await?;
          if !ok && !st.def.retry.allowed(st.attempts) { saga.state = SagaState::Failed; return Ok(()); }
        }
        st.state = StepState::Compensated;
      }
      saga.cursor -= 1;
    }
    saga.state = SagaState::Cancelled; // 或 Completed（若补偿成功并业务定义为取消）
    Ok(())
  }
}
```

------

## 4. 退避策略（`backoff.rs`）

```rust
#[derive(Clone, Debug)]
pub struct RetryPolicy {
  pub max_attempts: u32,
  pub base_ms: u64,         // 起始间隔
  pub factor: f64,          // 乘子
  pub jitter: f64,          // 0~1 随机抖动幅度
  pub cap_ms: u64,          // 上限
}
pub trait BackoffPolicy {
  fn next_after(&self, now_ms: i64, attempts: u32) -> i64;
}
impl BackoffPolicy for RetryPolicy {
  fn next_after(&self, now_ms: i64, attempts: u32) -> i64 {
    use rand::{Rng, rngs::StdRng, SeedableRng};
    let mut rng = StdRng::from_entropy();
    let exp = (self.base_ms as f64) * self.factor.powi((attempts as i32).saturating_sub(1));
    let capped = exp.min(self.cap_ms as f64);
    let jitter = 1.0 + (rng.gen::<f64>() * 2.0 - 1.0) * self.jitter;
    now_ms + (capped * jitter).max(self.base_ms as f64) as i64
  }
}
impl RetryPolicy {
  pub fn allowed(&self, attempts: u32) -> bool { attempts < self.max_attempts }
}
```

------

## 5. 死信与回放（`replay.rs`）

```rust
#[derive(Clone, Debug)]
pub struct DeadLetterRef { pub kind: DeadKind, pub id: sb_types::Id }
#[derive(Clone, Debug)]
pub enum DeadKind { Outbox, Saga }

#[async_trait::async_trait]
pub trait DeadStore: Send + Sync {
  async fn list(&self, tenant: &sb_types::TenantId, kind: DeadKind, limit: u32)
      -> Result<Vec<DeadLetterRef>, TxError>;
  async fn inspect(&self, r: &DeadLetterRef) -> Result<serde_json::Value, TxError>; // 摘要
  async fn replay(&self, r: &DeadLetterRef) -> Result<(), TxError>;                 // 重置状态重试
  async fn quarantine(&self, r: &DeadLetterRef, note: &str) -> Result<(), TxError>; // 隔离，避免自动重试
}
```

**回放原则**

- **Outbox**：`Dead → Pending(not_before=now, attempts=0, last_error=null)`；
- **Saga**：可选择**从失败步重新执行**或**直接进入补偿**；由实现根据记录的 `state/steps` 状态恢复。
- 回放事件 `TxReplayRequested` 写入审计；指标计数 `tx_replay_total{kind}`。

------

## 6. 错误映射（`errors.rs`）

```rust
#[derive(thiserror::Error, Debug)]
pub struct TxError(pub soulbase_errors::prelude::ErrorObj);

impl TxError {
  pub fn provider_unavailable(msg: &str) -> Self { ... }     // PROVIDER.UNAVAILABLE
  pub fn timeout(msg: &str) -> Self { ... }                  // TX.TIMEOUT（需在码表新增）
  pub fn idempo_busy() -> Self { ... }                       // TX.IDEMPOTENT_BUSY
  pub fn idempo_failed() -> Self { ... }                     // TX.IDEMPOTENT_LAST_FAILED
  pub fn schema(msg: &str) -> Self { ... }                   // SCHEMA.VALIDATION_FAILED
  pub fn conflict(msg: &str) -> Self { ... }                 // STORAGE.CONFLICT
  pub fn unknown(msg: &str) -> Self { ... }                  // UNKNOWN.INTERNAL
}
```

------

## 7. SurrealDB 落地映射（`surreal/schema.rs`, `surreal/repo.rs`）

### 7.1 表结构（SurrealQL）

```sql
-- Outbox
DEFINE TABLE outbox SCHEMAFULL;
DEFINE FIELD id          ON outbox TYPE string;          -- ulid
DEFINE FIELD tenant      ON outbox TYPE string;
DEFINE FIELD envelope_id ON outbox TYPE string;
DEFINE FIELD topic       ON outbox TYPE string;
DEFINE FIELD payload     ON outbox TYPE object;
DEFINE FIELD created_at  ON outbox TYPE datetime;
DEFINE FIELD not_before  ON outbox TYPE datetime;
DEFINE FIELD attempts    ON outbox TYPE int;
DEFINE FIELD status      ON outbox TYPE string;          -- pending|leased|done|dead
DEFINE FIELD last_error  ON outbox TYPE string;
DEFINE FIELD dispatch_key ON outbox TYPE string;
DEFINE FIELD lease_until ON outbox TYPE datetime;
DEFINE FIELD worker      ON outbox TYPE string;

DEFINE INDEX idx_outbox_ready ON TABLE outbox COLUMNS tenant, status, not_before;
DEFINE INDEX idx_outbox_key   ON TABLE outbox COLUMNS tenant, dispatch_key;
DEFINE INDEX pk_outbox_id     ON TABLE outbox COLUMNS id UNIQUE;

-- Idempotency
DEFINE TABLE idempo SCHEMAFULL;
DEFINE FIELD key        ON idempo TYPE string;
DEFINE FIELD tenant     ON idempo TYPE string;
DEFINE FIELD hash       ON idempo TYPE string;
DEFINE FIELD status     ON idempo TYPE string;
DEFINE FIELD result_digest ON idempo TYPE string;
DEFINE FIELD ttl_ms     ON idempo TYPE int;
DEFINE FIELD created_at ON idempo TYPE datetime;
DEFINE FIELD updated_at ON idempo TYPE datetime;

DEFINE INDEX uniq_idempo ON TABLE idempo COLUMNS tenant, key UNIQUE;

-- Saga
DEFINE TABLE saga SCHEMAFULL;
DEFINE FIELD id        ON saga TYPE string;
DEFINE FIELD tenant    ON saga TYPE string;
DEFINE FIELD state     ON saga TYPE string;
DEFINE FIELD def_name  ON saga TYPE string;
DEFINE FIELD steps     ON saga TYPE array;      -- JSON 数组（各 StepState）
DEFINE FIELD cursor    ON saga TYPE int;
DEFINE FIELD created_at ON saga TYPE datetime;
DEFINE FIELD updated_at ON saga TYPE datetime;
DEFINE FIELD timeout_at ON saga TYPE datetime;

DEFINE INDEX pk_saga_id ON TABLE saga COLUMNS id UNIQUE;

-- Dead letters（也可做视图或状态字段聚合）
DEFINE TABLE dead_letters SCHEMAFULL;
DEFINE FIELD kind       ON dead_letters TYPE string;  -- outbox|saga
DEFINE FIELD ref_id     ON dead_letters TYPE string;  -- 对应 outbox.id / saga.id
DEFINE FIELD tenant     ON dead_letters TYPE string;
DEFINE FIELD error_code ON dead_letters TYPE string;
DEFINE FIELD message    ON dead_letters TYPE string;
DEFINE FIELD created_at ON dead_letters TYPE datetime;

DEFINE INDEX idx_dead ON TABLE dead_letters COLUMNS tenant, kind, created_at;
```

### 7.2 查询模板（使用 `soulbase-storage` 的命名参数绑定）

- **入队（事务内）**

```sql
-- 在业务 Tx 中
CREATE type::thing("outbox", $id) CONTENT {
  id: $id, tenant: $tenant, envelope_id: $envelope_id, topic: $topic,
  payload: $payload, created_at: time::now(), not_before: time::now(),
  attempts: 0, status: "pending", dispatch_key: $dispatch_key
};
```

- **租约批量（按 dispatch_key 串行）**

```sql
-- 选择可用消息
SELECT * FROM outbox
WHERE tenant = $tenant
  AND status = "pending"
  AND not_before <= time::now()
ORDER BY not_before ASC
LIMIT $batch;

-- 尝试租约（逐条，或用事务对命中的行 set lease）
UPDATE outbox SET status="leased", lease_until = time::now() + $lease_ms, worker=$worker
WHERE id = $id AND status="pending";
```

- **ACK/NACK/DEAD**

```sql
UPDATE outbox SET status="done", last_error=NULL, worker=NULL WHERE id=$id;

UPDATE outbox SET status="pending", attempts = attempts + 1, not_before = time::now() + $delta_ms,
  last_error = $err, worker=NULL, lease_until=NULL
WHERE id=$id;

-- DEAD
UPDATE outbox SET status="dead", last_error=$err WHERE id=$id;
CREATE dead_letters CONTENT {
  kind:"outbox", ref_id:$id, tenant:$tenant, error_code: $code, message: $err, created_at: time::now()
};
```

- **幂等**

```sql
-- check_and_put
LET $rec = SELECT * FROM idempo WHERE tenant=$tenant AND key=$key;
IF $rec[0] == NONE THEN
  CREATE idempo CONTENT { key:$key, tenant:$tenant, hash:$hash, status:"InFlight", ttl_ms:$ttl, created_at: time::now(), updated_at: time::now() };
ELSE
  RETURN $rec[0];
END;

-- finish/fail
UPDATE idempo SET status="Succeeded", result_digest=$digest, updated_at=time::now()
WHERE tenant=$tenant AND key=$key;

UPDATE idempo SET status="Failed", updated_at=time::now()
WHERE tenant=$tenant AND key=$key;
```

- **Saga 保存/推进**

```sql
CREATE saga CONTENT {
  id:$id, tenant:$tenant, state:"Running", def_name:$def_name, steps:$steps,
  cursor:0, created_at:time::now(), updated_at:time::now(), timeout_at:$timeout_at
};

UPDATE saga SET steps=$steps, cursor=$cursor, state=$state, updated_at=time::now()
WHERE id=$id AND tenant=$tenant;
```

------

## 8. 编排算法（摘要）

### 8.1 Dispatcher 主循环

```
loop tick(tenant):
  now = now_ms()
  msgs = store.lease_batch(tenant, now, lease_ms, batch, group_by_key=true)
  for m in msgs:
     result = transport.send(m.topic, m.payload)
     if ok: store.ack_done(m.id)
     else:
        attempts = m.attempts + 1
        if attempts >= max_attempts: store.dead_letter(m.id, error)
        else: store.nack_backoff(m.id, backoff.next_after(now, attempts), error)
```

- **并发串行化**：`group_by_key=true` 确保同 `dispatch_key` 同时最多取 1 条。
- **租约**：`lease_until` 防止 worker 崩溃导致长租；过期后其他 worker 可接管。

### 8.2 Saga 推进

```
tick(saga_id):
  saga = store.load(id)
  if saga.state == Running:
     step = saga.steps[cursor]
     if step.state in {Ready, Failed and retry_allowed}:
         ok = participant.execute(step.def.action_uri, saga)
         if ok -> mark Succeeded, cursor+=1
         else if retry_allowed -> mark Failed (等待下一次 tick)
         else -> saga.state = Compensating
     else if step.state == Succeeded and cursor+1 < steps -> cursor+=1
     else -> state=Completed
  else if saga.state == Compensating:
     for i from cursor-1 downto 0:
       if steps[i].def.compensate_uri:
          ok = participant.compensate(...)
          if !ok and !retry_allowed -> state=Failed; break
       steps[i].state = Compensated
     if all compensated -> state=Cancelled
  save(saga)
```

- **并发步骤**：以 `concurrency_tag` 实现（同 tag 的步骤并行；RIS 保留顺序，后续扩展）。

------

## 9. 指标（`observe.rs`）

- **Outbox**
  - `tx_outbox_enqueued_total{tenant,topic}`
  - `tx_outbox_dispatched_total{tenant,topic}`
  - `tx_outbox_retry_total{tenant,code}`
  - `tx_outbox_dead_total{tenant,code}`
  - `tx_outbox_latency_ms{p50,p95}{tenant}`（入库→首次投递）
- **Saga**
  - `tx_saga_started_total{tenant,def}` / `tx_saga_completed_total` / `tx_saga_failed_total` / `tx_saga_compensate_total`
  - `tx_saga_step_latency_ms{def,step}`
- **Idempo**
  - `tx_idempo_hits_total{tenant}` / `tx_idempo_conflicts_total`

**标签**：`tenant`, `topic|def|step`, `code`, `retryable`, `severity`（来自 `soulbase-errors`）。

------

## 10. 配置（与 `soulbase-config` 对接）

```yaml
tx:
  outbox:
    lease_ms: 15000
    batch: 50
    max_attempts: 12
    backoff: { base_ms: 500, factor: 2.0, jitter: 0.3, cap_ms: 300000 }
  saga:
    default_retry: { max_attempts: 6, base_ms: 1000, factor: 2.0, jitter: 0.2, cap_ms: 60000 }
    heartbeat_ms: 5000
  idempo:
    ttl_ms: 86400000         # 24h
    result_digest_max_bytes: 4096
```

热更生效：下一次 `tick`/新实例读取即用。

------

## 11. 开发与落地指南

- **出站统一走 Outbox**：凡“写库 + 下游副作用/通知”，**禁止**直接发送，改为事务内 `enqueue_in_tx`。
- **消费者幂等**：消费端使用 `IdempotencyStore` 保护写路径，key 建议 = `tenant + ":" + envelope_id` 或业务唯一键。
- **错误公共视图**：对外仅返回稳定码 + 用户短消息；诊断细节仅入审计。
- **证据链**：所有 Outbox/Saga 关键动作生成 `Envelope<TxEvent>`（Begin/Retry/Dead/Replay），由 `observe` 采集。
- **SurrealDB**：使用 `soulbase-storage` 的参数化接口；强制 `$tenant` 存在与 ID 前缀规则。

------

## 12. 开放问题

- **跨域账本**：与 `soulbase-a2a` 的对账周期、签名与最小披露字段对齐。
- **多队列后端**：Kafka/NATS/AMQP 的事务性生产/幂等语义适配器。
- **长事务拆分**：将超长 Saga 拆分为子 Saga 并行编排的基础支持。
- **人审环节**：对不可自动补偿的失败，引入“挂起/审批/继续”协议。

------

> 以上 TD 将在 **SB-10-RIS** 中落为可运行骨架：Surreal 表迁移脚本、OutboxStore（租约/ACK/NACK/DEAD）、Dispatcher 主循环、IdempotencyStore/Producer&Consumer、SagaOrchestrator 及最小本地 Participant、退避策略与单测（入库→投递→重试→死信、Saga 正/逆序补偿、幂等命中）。
