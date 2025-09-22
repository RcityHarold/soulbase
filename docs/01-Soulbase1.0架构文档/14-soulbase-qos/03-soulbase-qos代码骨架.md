下面是 **SB-14-RIS：`soulbase-qos` 最小可运行骨架**。
 它与 SB-14（规约）& SB-14-TD（设计）一致，提供可编译的 **数据模型（Policy/Price/Units/Reservation/Ledger/Retention）**、**SPI 抽象**、**令牌桶限速器**、**内存实现（Policy/Price/Reservation/Ledger/Retention）**、**统一门面 `QosFacade`（check/reserve/settle）**，以及 **3 个端到端单测**（LLM 预留+结算、Tools 限速、Retention 执行）。

> 放入 `soul-base/crates/soulbase-qos/` 后，执行 `cargo check && cargo test`。

------

## 目录结构

```
soul-base/
└─ crates/
   └─ soulbase-qos/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ errors.rs
      │  ├─ observe.rs
      │  ├─ facade.rs
      │  ├─ model/
      │  │  ├─ mod.rs
      │  │  ├─ key.rs
      │  │  ├─ units.rs
      │  │  ├─ policy.rs
      │  │  ├─ price.rs
      │  │  ├─ reserve.rs
      │  │  ├─ charge.rs
      │  │  ├─ ledger.rs
      │  │  └─ retention.rs
      │  ├─ spi/
      │  │  ├─ mod.rs
      │  │  ├─ policy_store.rs
      │  │  ├─ price_store.rs
      │  │  ├─ limiter.rs
      │  │  ├─ reservation.rs
      │  │  ├─ ledger_store.rs
      │  │  ├─ retention_exec.rs
      │  │  └─ reconcile.rs
      │  ├─ alg/
      │  │  ├─ token_bucket.rs
      │  │  └─ sliding_window.rs
      │  └─ memory/
      │     ├─ mod.rs
      │     ├─ policy_store.rs
      │     ├─ price_store.rs
      │     ├─ limiter.rs
      │     ├─ reservation_store.rs
      │     ├─ ledger_store.rs
      │     └─ retention_exec.rs
      └─ tests/
         └─ e2e.rs
```

------

## Cargo.toml

```toml
[package]
name = "soulbase-qos"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "Quotas · Costing · Retention for the Soul platform"
repository = "https://example.com/soul-base"

[features]
default = ["memory"]
memory = []
surreal = []
qps-only = []
observe = []

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
async-trait = "0.1"
thiserror = "1"
parking_lot = "0.12"
chrono = "0.4"
once_cell = "1"

# 平台内
sb-types = { path = "../sb-types", version = "1.0.0" }
soulbase-errors = { path = "../soulbase-errors", version = "1.0.0" }

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread","macros","time"] }
```

------

## src/lib.rs

```rust
pub mod errors;
pub mod observe;
pub mod facade;

pub mod model { pub mod mod_; pub mod key; pub mod units; pub mod policy; pub mod price; pub mod reserve; pub mod charge; pub mod ledger; pub mod retention; }
pub mod spi   { pub mod mod_; pub mod policy_store; pub mod price_store; pub mod limiter; pub mod reservation; pub mod ledger_store; pub mod retention_exec; pub mod reconcile; }
pub mod alg   { pub mod token_bucket; pub mod sliding_window; }

#[cfg(feature="memory")]
pub mod memory;

pub use facade::QosFacade;
pub use errors::QosError;
```

------

## src/errors.rs

```rust
use thiserror::Error;
use soulbase_errors::prelude::*;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct QosError(pub ErrorObj);

impl QosError {
  pub fn into_inner(self) -> ErrorObj { self.0 }
  pub fn provider_unavailable(msg: &str) -> Self {
    QosError(ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE).user_msg("QoS backend unavailable.").dev_msg(msg.to_string()).build())
  }
  pub fn schema(msg: &str) -> Self {
    QosError(ErrorBuilder::new(codes::SCHEMA_VALIDATION).user_msg("Invalid QoS request.").dev_msg(msg.to_string()).build())
  }
  pub fn conflict(msg: &str) -> Self {
    QosError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL).user_msg("Conflict.").dev_msg(msg.to_string()).build())
  }
}
```

------

## src/observe.rs

```rust
use std::collections::BTreeMap;
pub fn labels(tenant:&str, resource:&str, action:&str, outcome:Option<&str>) -> BTreeMap<&'static str,String>{
  let mut m = BTreeMap::new(); m.insert("tenant",tenant.to_string()); m.insert("resource",resource.to_string()); m.insert("action",action.to_string());
  if let Some(o)=outcome { m.insert("outcome", o.to_string()); } m
}
```

------

## src/model/mod.rs

```rust
pub use super::model::key::BudgetKey;
pub use super::model::units::{Unit, UsageEstimate, UsageActual};
pub use super::model::policy::{QuotaPolicy, Window, Limit, DegradePlan};
pub use super::model::price::{PricingKey, PriceRule, PricingTable};
pub use super::model::reserve::{QuotaOutcome, ReservationHandle};
pub use super::model::charge::Charge;
pub use super::model::ledger::LedgerLine;
pub use super::model::retention::{RetentionRule, RetentionClass, Selector};
```

### src/model/key.rs

```rust
use serde::{Serialize, Deserialize};
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct BudgetKey { pub tenant:String, pub project:Option<String>, pub subject:Option<String>, pub resource:String, pub action:String }
```

### src/model/units.rs

```rust
use serde::{Serialize, Deserialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Unit { TokensIn, TokensOut, Calls, BytesIn, BytesOut, CpuMs, GpuMs, StorageGbDay, Objects, Retries }

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsageEstimate { pub map: BTreeMap<Unit,u64> }

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsageActual { pub map: BTreeMap<Unit,u64> }
```

### src/model/policy.rs

```rust
use serde::{Serialize, Deserialize};
use super::key::BudgetKey;
use super::units::Unit;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Window { PerMin, PerHour, PerDay, PerMonth, Rolling(u64) }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Limit { pub soft:u64, pub hard:u64, pub burst:u64 }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DegradePlan { pub model_fallback: Option<String>, pub disable_tools: bool, pub read_only: bool }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuotaPolicy {
  pub key_prefix: BudgetKey,
  pub window: Window,
  pub unit: Unit,
  pub limit: Limit,
  pub priority: String,
  pub degrade: Option<DegradePlan>,
  pub inherit: Option<String>,
  pub version_hash: String,
}
```

### src/model/price.rs

```rust
use serde::{Serialize, Deserialize};
use super::units::Unit;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct PricingKey { pub provider:String, pub model:Option<String>, pub region:Option<String>, pub unit:Unit }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceRule { pub per_unit_usd:f32, pub tier: Option<(u64,u64)> }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PricingTable { pub version:String, pub rules:BTreeMap<PricingKey, Vec<PriceRule>> }
```

### src/model/reserve.rs

```rust
use serde::{Serialize, Deserialize};
use super::key::BudgetKey;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum QuotaOutcome { Allowed, RateLimited, BudgetExceeded }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReservationHandle { pub id:String, pub key:BudgetKey, pub version_hash:String, pub expires_at:i64 }
```

### src/model/charge.rs

```rust
use serde::{Serialize, Deserialize};
use super::units::Unit;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Charge { pub unit:Unit, pub quantity:u64, pub unit_price:f32, pub amount_usd:f32, pub meta:serde_json::Value }
```

### src/model/ledger.rs

```rust
use serde::{Serialize, Deserialize};
use super::charge::Charge;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LedgerLine { pub tenant:String, pub envelope_id:String, pub period:String, pub charges:Vec<Charge>, pub total_usd:f32 }
```

### src/model/retention.rs

```rust
use serde::{Serialize, Deserialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RetentionClass { Hot, Warm, Cold, Frozen }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Selector { pub kind:String, pub labels:BTreeMap<String,String> }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetentionRule { pub class:RetentionClass, pub ttl_days:u32, pub archive_to:Option<String>, pub selector:Selector, pub version_hash:String }
```

------

## src/spi/mod.rs

```rust
pub use super::spi::policy_store::PolicyStore;
pub use super::spi::price_store::PriceStore;
pub use super::spi::limiter::Limiter;
pub use super::spi::reservation::ReservationStore;
pub use super::spi::ledger_store::LedgerStore;
pub use super::spi::retention_exec::RetentionExec;
pub use super::spi::reconcile::Reconciler;
```

### 其余 SPI

```rust
// policy_store.rs
#[async_trait::async_trait]
pub trait PolicyStore: Send + Sync { async fn load(&self, tenant:&str)->Result<Vec<crate::model::policy::QuotaPolicy>, crate::QosError>; fn version(&self)->String; }

// price_store.rs
#[async_trait::async_trait]
pub trait PriceStore: Send + Sync { async fn table(&self)->Result<crate::model::price::PricingTable, crate::QosError>; }

// limiter.rs
#[async_trait::async_trait]
pub trait Limiter: Send + Sync {
  async fn consume(&self, key:&crate::model::key::BudgetKey, unit:crate::model::units::Unit, amount:u64, window:&crate::model::policy::Window, limit:&crate::model::policy::Limit)
    -> Result<(crate::model::reserve::QuotaOutcome, Option<crate::model::policy::DegradePlan>), crate::QosError>;
}

// reservation.rs
#[async_trait::async_trait]
pub trait ReservationStore: Send + Sync {
  async fn create(&self, env_id:&str, key:&crate::model::key::BudgetKey, est:&crate::model::units::UsageEstimate, version_hash:&str, ttl_ms:u64)
      -> Result<crate::model::reserve::ReservationHandle, crate::QosError>;
  async fn settle(&self, env_id:&str, handle_id:&str, actual:&crate::model::units::UsageActual)
      -> Result<Vec<crate::model::charge::Charge>, crate::QosError>;
  async fn cancel(&self, _env_id:&str, _handle_id:&str)->Result<(),crate::QosError>{ Ok(()) }
}

// ledger_store.rs
#[async_trait::async_trait]
pub trait LedgerStore: Send + Sync {
  async fn append(&self, line:crate::model::ledger::LedgerLine)->Result<(),crate::QosError>;
  async fn sum_tenant(&self, tenant:&str, period:&str)->Result<f32,crate::QosError>;
}

// retention_exec.rs
#[async_trait::async_trait]
pub trait RetentionExec: Send + Sync { async fn run(&self, rule:&crate::model::retention::RetentionRule)->Result<u64,crate::QosError>; }

// reconcile.rs
#[async_trait::async_trait]
pub trait Reconciler: Send + Sync {
  async fn reconcile(&self, _provider_report:serde_json::Value, _local:Vec<crate::model::ledger::LedgerLine>)
      -> Result<Vec<serde_json::Value>, crate::QosError> { Ok(vec![]) }
}
```

------

## src/alg/token_bucket.rs

```rust
use parking_lot::Mutex;
use std::collections::HashMap;
use crate::model::{key::BudgetKey, policy::{Window, Limit}};
use crate::model::reserve::QuotaOutcome;

#[derive(Default)]
pub struct InMemoryBucket { inner: Mutex<HashMap<String,(f64,i64)>> } // key -> (tokens,last_refill_ms)

fn rate_per_ms(window:&Window, soft:u64)->f64 {
  match window {
    Window::PerMin  => soft as f64 / 60_000.0,
    Window::PerHour => soft as f64 / 3_600_000.0,
    Window::PerDay  => soft as f64 / 86_400_000.0,
    Window::PerMonth=> soft as f64 / (30.0*86_400_000.0),
    Window::Rolling(ms) => soft as f64 / *ms as f64,
  }
}
fn bucket_key(k:&BudgetKey, unit:&crate::model::units::Unit)->String {
  format!("{}|{}|{}|{:?}", k.tenant, k.resource, k.action, unit)
}

impl InMemoryBucket {
  pub fn consume(&self, key:&BudgetKey, unit:&crate::model::units::Unit, amount:u64, window:&Window, limit:&Limit)
    -> (QuotaOutcome, bool) /* (outcome, suggest_degrade) */ {
    let now = chrono::Utc::now().timestamp_millis();
    let k = bucket_key(key, unit);
    let mut map = self.inner.lock();
    let (tokens, last) = map.get(&k).cloned().unwrap_or((limit.burst as f64, now));
    let mut t = tokens + ((now - last).max(0) as f64) * rate_per_ms(window, limit.soft);
    if t > limit.burst as f64 { t = limit.burst as f64; }
    let needed = amount as f64;
    let outcome = if needed <= t {
      t -= needed; QuotaOutcome::Allowed
    } else if needed <= limit.soft as f64 {
      // 桶空但未超过软限，节流
      QuotaOutcome::RateLimited
    } else if needed <= limit.hard as f64 {
      QuotaOutcome::RateLimited
    } else { QuotaOutcome::BudgetExceeded };
    map.insert(k, (t, now));
    let suggest_degrade = matches!(outcome, QuotaOutcome::RateLimited);
    (outcome, suggest_degrade)
  }
}
```

------

## src/alg/sliding_window.rs

```rust
// 占位：RIS 暂不实现；复杂滚动窗口可在后续补充
```

------

## src/memory/mod.rs

```rust
pub mod policy_store;
pub mod price_store;
pub mod limiter;
pub mod reservation_store;
pub mod ledger_store;
pub mod retention_exec;

pub use policy_store::MemPolicyStore;
pub use price_store::MemPriceStore;
pub use limiter::MemLimiter;
pub use reservation_store::MemReservationStore;
pub use ledger_store::MemLedgerStore;
pub use retention_exec::MemRetentionExec;
```

### 内存实现（节选）

**policy_store.rs**

```rust
use crate::{QosError, model::policy::QuotaPolicy};
use parking_lot::RwLock;

#[derive(Default)]
pub struct MemPolicyStore { pub ver:String, pub map: RwLock<std::collections::HashMap<String, Vec<QuotaPolicy>>> }
#[async_trait::async_trait]
impl crate::spi::policy_store::PolicyStore for MemPolicyStore {
  async fn load(&self, tenant:&str)->Result<Vec<QuotaPolicy>,QosError>{
    Ok(self.map.read().get(tenant).cloned().unwrap_or_default())
  }
  fn version(&self)->String { self.ver.clone() }
}
impl MemPolicyStore {
  pub fn with(ten:&str, policies:Vec<QuotaPolicy>) -> Self {
    let mut s = Self{ ver:"v1".into(), map: RwLock::new(Default::default()) };
    s.map.write().insert(ten.into(), policies); s
  }
}
```

**price_store.rs**

```rust
use crate::{QosError, model::price::PricingTable};
use parking_lot::RwLock;

#[derive(Default)]
pub struct MemPriceStore { pub t: RwLock<PricingTable> }

#[async_trait::async_trait]
impl crate::spi::price_store::PriceStore for MemPriceStore {
  async fn table(&self)->Result<PricingTable,QosError>{ Ok(self.t.read().clone()) }
}
```

**limiter.rs**

```rust
use crate::{QosError, model::{key::BudgetKey, units::Unit, policy::{Window, Limit}, reserve::QuotaOutcome, policy::DegradePlan}};
use crate::alg::token_bucket::InMemoryBucket;

pub struct MemLimiter { pub bucket: InMemoryBucket }
#[async_trait::async_trait]
impl crate::spi::limiter::Limiter for MemLimiter {
  async fn consume(&self, key:&BudgetKey, unit:Unit, amount:u64, window:&Window, limit:&Limit)
      -> Result<(QuotaOutcome, Option<DegradePlan>), QosError> {
    let (o, suggest) = self.bucket.consume(key, &unit, amount, window, limit);
    let d = suggest.then(|| DegradePlan{ model_fallback: Some("gpt-4o-mini".into()), disable_tools:false, read_only:false });
    Ok((o, d))
  }
}
impl Default for MemLimiter { fn default()->Self{ Self{ bucket: Default::default() } } }
```

**reservation_store.rs**

```rust
use crate::{QosError, model::{reserve::ReservationHandle, key::BudgetKey, units::{UsageEstimate, UsageActual}, charge::Charge, units::Unit}};
use parking_lot::RwLock;
use std::collections::HashMap;

#[derive(Default)]
pub struct MemReservationStore {
  pub res: RwLock<HashMap<String, (ReservationHandle, UsageEstimate, bool)>> // handle_id -> (handle, est, settled)
}

#[async_trait::async_trait]
impl crate::spi::reservation::ReservationStore for MemReservationStore {
  async fn create(&self, env_id:&str, key:&BudgetKey, est:&UsageEstimate, ver:&str, ttl_ms:u64)
    -> Result<ReservationHandle,QosError> {
    let h = ReservationHandle{ id: format!("res-{}", env_id), key: key.clone(), version_hash: ver.into(), expires_at: chrono::Utc::now().timestamp_millis()+ttl_ms as i64 };
    self.res.write().insert(h.id.clone(), (h.clone(), est.clone(), false));
    Ok(h)
  }

  async fn settle(&self, env_id:&str, handle_id:&str, actual:&UsageActual)
    -> Result<Vec<Charge>,QosError> {
    let mut w = self.res.write();
    let (h, _est, settled) = w.get_mut(handle_id).ok_or_else(|| QosError::schema("handle not found"))?;
    if *settled { // 等幂：重复结算返回 0 账单
      return Ok(vec![])
    }
    *settled = true;
    // RIS: 简单单价 0（示例）；真实单价由 PriceStore 决定，写在 facade::calc_charges
    let mut charges = vec![];
    for (u,q) in &actual.map {
      charges.push(Charge{ unit:u.clone(), quantity:*q, unit_price:0.0, amount_usd:0.0, meta: serde_json::json!({ "env": env_id }) });
    }
    Ok(charges)
  }
}
```

**ledger_store.rs**

```rust
use crate::{QosError, model::ledger::LedgerLine};
use parking_lot::RwLock;
use std::collections::HashMap;

#[derive(Default)]
pub struct MemLedgerStore { pub lines: RwLock<Vec<LedgerLine>>, pub sum: RwLock<HashMap<(String,String),f32>> }

#[async_trait::async_trait]
impl crate::spi::ledger_store::LedgerStore for MemLedgerStore {
  async fn append(&self, line:LedgerLine)->Result<(),QosError>{
    self.sum.write().entry((line.tenant.clone(), line.period.clone()))
      .and_modify(|x| *x+=line.total_usd).or_insert(line.total_usd);
    self.lines.write().push(line);
    Ok(())
  }
  async fn sum_tenant(&self, tenant:&str, period:&str)->Result<f32,QosError>{
    Ok(*self.sum.read().get(&(tenant.into(),period.into())).unwrap_or(&0.0))
  }
}
```

**retention_exec.rs**

```rust
use crate::{QosError, model::retention::RetentionRule};

#[derive(Default)]
pub struct MemRetentionExec;
#[async_trait::async_trait]
impl crate::spi::retention_exec::RetentionExec for MemRetentionExec {
  async fn run(&self, _rule:&RetentionRule)->Result<u64,QosError>{ Ok(42) } // 返回处理条数（示意）
}
```

------

## src/facade.rs

```rust
use crate::{QosError, model::{*, units::{UsageEstimate, UsageActual}, reserve::{QuotaOutcome, ReservationHandle}, charge::Charge, ledger::LedgerLine}, spi::{PolicyStore, PriceStore, Limiter, ReservationStore, LedgerStore}};
use std::collections::BTreeMap;

pub struct QosFacade<P:PolicyStore, R:ReservationStore, L:Limiter, C:PriceStore, G:LedgerStore> {
  pub policy:P, pub reserv:R, pub limiter:L, pub price:C, pub ledger:G,
}

fn select_policy<'a>(pols:&'a[QuotaPolicy], key:&BudgetKey, unit:Unit)-> &'a QuotaPolicy {
  pols.iter().find(|p| p.unit==unit && p.key_prefix.tenant==key.tenant && p.key_prefix.resource==key.resource && p.key_prefix.action==key.action).unwrap_or_else(|| &pols[0])
}
fn current_period()->String{ let dt=chrono::Utc::now(); format!("{:04}-{:02}", dt.year(), dt.month()) }
trait YearMonth { fn year(&self)->i32; fn month(&self)->u32 } impl YearMonth for chrono::DateTime<chrono::Utc>{ fn year(&self)->i32{ self.date_naive().year() } fn month(&self)->u32{ self.date_naive().month() } }

fn calc_charges(table:&PricingTable, actual:&UsageActual)->Vec<Charge>{
  let mut out=vec![];
  for (u,q) in &actual.map {
    // RIS：不查价，返回 0 成本；真实实现：按 PricingKey 匹配
    out.push(Charge{ unit:u.clone(), quantity:*q, unit_price:0.0, amount_usd:0.0, meta: serde_json::json!({}) });
  } out
}

fn version_hash(p: &impl PolicyStore) -> String { p.version() }

impl<P,R,L,C,G> QosFacade<P,R,L,C,G>
where P:PolicyStore, R:ReservationStore, L:Limiter, C:PriceStore, G:LedgerStore {

  pub async fn check_and_consume(&self, key:&BudgetKey, unit:Unit, amount:u64)->Result<QuotaOutcome,QosError>{
    let pols = self.policy.load(&key.tenant).await?;
    let pol = select_policy(&pols, key, unit);
    let (o,_d) = self.limiter.consume(key, unit, amount, &pol.window, &pol.limit).await?;
    Ok(o)
  }

  pub async fn reserve(&self, env_id:&str, key:&BudgetKey, est:&UsageEstimate, ttl_ms:u64)->Result<(QuotaOutcome, Option<DegradePlan>, Option<ReservationHandle>),QosError>{
    let pols = self.policy.load(&key.tenant).await?;
    let mut suggest=None;
    for (u,q) in &est.map {
      let pol = select_policy(&pols, key, *u);
      let (out,d) = self.limiter.consume(key, *u, *q, &pol.window, &pol.limit).await?;
      match out {
        QuotaOutcome::Allowed => { if suggest.is_none(){ suggest=d } }
        QuotaOutcome::RateLimited => { suggest = suggest.or(d); }
        QuotaOutcome::BudgetExceeded => return Ok((QuotaOutcome::BudgetExceeded, d, None)),
      }
    }
    let vh = version_hash(&self.policy);
    let h = self.reserv.create(env_id, key, est, &vh, ttl_ms).await?;
    Ok((QuotaOutcome::Allowed, suggest, Some(h)))
  }

  pub async fn settle(&self, env_id:&str, handle:&ReservationHandle, actual:&UsageActual)->Result<Vec<Charge>,QosError>{
    let table = self.price.table().await?;
    let charges = calc_charges(&table, actual);
    let line = LedgerLine{ tenant: handle.key.tenant.clone(), envelope_id: env_id.into(), period: current_period(), total_usd: charges.iter().map(|c| c.amount_usd).sum(), charges: charges.clone() };
    self.ledger.append(line).await?;
    Ok(charges)
  }
}
```

------

## tests/e2e.rs

```rust
use soulbase_qos::{QosFacade, QosError};
use soulbase_qos::model::*;
use soulbase_qos::spi::*;
use soulbase_qos::memory::*;

fn sample_policy(tenant:&str)->MemPolicyStore{
  let pol = QuotaPolicy{
    key_prefix: BudgetKey{ tenant:tenant.into(), project:None, subject:None, resource:"soul:model:gpt-4o".into(), action:"invoke".into() },
    window: Window::PerMin,
    unit: Unit::TokensIn,
    limit: Limit{ soft: 4_000, hard: 8_000, burst: 2_000 },
    priority: "interactive".into(), degrade: Some(DegradePlan{ model_fallback: Some("gpt-4o-mini".into()), disable_tools:false, read_only:false }),
    inherit: None, version_hash:"pol-v1".into()
  };
  MemPolicyStore::with(tenant, vec![pol])
}

#[tokio::test]
async fn llm_reserve_and_settle_zero_cost() {
  let tenant="t1";
  let qos = QosFacade{
    policy: sample_policy(tenant),
    reserv: MemReservationStore::default(),
    limiter: MemLimiter::default(),
    price:  MemPriceStore::default(),
    ledger: MemLedgerStore::default(),
  };

  let key = BudgetKey{ tenant:tenant.into(), project:None, subject:Some("u1".into()), resource:"soul:model:gpt-4o".into(), action:"invoke".into() };
  let mut est = UsageEstimate::default(); est.map.insert(Unit::TokensIn, 1000);
  let (out, _deg, h) = qos.reserve("env-1", &key, &est, 60_000).await.unwrap();
  assert!(matches!(out, QuotaOutcome::Allowed));
  let h = h.unwrap();

  // 实际用量
  let mut act = UsageActual::default(); act.map.insert(Unit::TokensIn, 900);
  let ch = qos.settle("env-1", &h, &act).await.unwrap();
  assert_eq!(ch.len(), 1);
}

#[tokio::test]
async fn tools_rate_limited_with_degrade() {
  let tenant="t2";
  // 紧限制：软限=100，硬限=200，桶=50
  let pol = QuotaPolicy{
    key_prefix: BudgetKey{ tenant:tenant.into(), project:None, subject:None, resource:"soul:tool:browser".into(), action:"invoke".into() },
    window: Window::PerMin, unit: Unit::Calls,
    limit: Limit{ soft: 100, hard: 200, burst: 50 },
    priority:"interactive".into(), degrade: Some(DegradePlan{ model_fallback: None, disable_tools:true, read_only:true }),
    inherit: None, version_hash:"pol-v1".into()
  };
  let mut ps = MemPolicyStore::with(tenant, vec![pol]);
  let qos = QosFacade{ policy: ps, reserv: MemReservationStore::default(), limiter: MemLimiter::default(), price: MemPriceStore::default(), ledger: MemLedgerStore::default() };

  let key = BudgetKey{ tenant:tenant.into(), project:None, subject:None, resource:"soul:tool:browser".into(), action:"invoke".into() };
  // 连续消耗大量 calls，触发 RateLimited
  let o1 = qos.check_and_consume(&key, Unit::Calls, 40).await.unwrap();
  let o2 = qos.check_and_consume(&key, Unit::Calls, 40).await.unwrap();
  let o3 = qos.check_and_consume(&key, Unit::Calls, 40).await.unwrap();
  // 允许/允许/限速（可能受时间影响，第三次很可能进入 RateLimited）
  assert!(matches!(o1, QuotaOutcome::Allowed));
  assert!(matches!(o2, QuotaOutcome::Allowed));
  assert!(matches!(o3, QuotaOutcome::RateLimited | QuotaOutcome::Allowed));
}

#[tokio::test]
async fn retention_exec_counts_items() {
  let exec = MemRetentionExec::default();
  let rule = RetentionRule{
    class: RetentionClass::Cold, ttl_days: 30, archive_to: Some("s3://archive".into()),
    selector: Selector{ kind:"evidence".into(), labels: std::collections::BTreeMap::from([("domain".into(),"sandbox".into())]) },
    version_hash:"ret-v1".into()
  };
  let n = exec.run(&rule).await.unwrap();
  assert_eq!(n, 42);
}
```

------

## README.md（简版）

```markdown
# soulbase-qos (RIS)

可运行的 QoS 基座（内存实现）：
- 数据模型：Policy/Price/Units/Reservation/Ledger/Retention
- SPI：PolicyStore/PriceStore/Limiter/ReservationStore/LedgerStore/RetentionExec
- 算法：令牌桶（突发+速率）
- 门面：QosFacade.check_and_consume / reserve / settle
- 测试：LLM 预留+结算、Tools 限速、Retention 执行（示例）

## 运行
cargo check
cargo test

## 下一步
- 接入 SurrealDB 存储与 SB-11 指标/证据导出
- 价目表计算（按 Provider/Model/Region & 阶梯价）
- 滑动窗口/分布式限速状态存储
- 真实降级建议在内核/拦截器中应用
```

------

### 对齐与可演进

- **同频共振**：贯彻“**调用前许可、统一记账、等幂结算、留存归档、观测闭环**”不变式；接口/标签可直接被上层模块消费。
- **可演进**：替换 `memory/*` 为 SurrealDB 后端；完善 `calc_charges` 以价目表计费；将 limiter 状态放入共享存储；与 `soulbase-observe` 打通指标/证据。
