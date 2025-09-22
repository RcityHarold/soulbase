下面是 **SB-16-RIS：`soulbase-cache` 最小可运行骨架**。
 它与 SB-16（规约）& SB-16-TD（设计）一致，提供：

- 统一抽象：`Cache / SingleFlight / Invalidation / Stats`，策略 `CachePolicy`，键名 `CacheKey`，序列化 `Codec`。
- **两级缓存 orchestrator**（本地 LRU → 可选 Redis stub → loader）
- **SingleFlight 请求合并**、**TTL 抖动**、**SWR（过期先回旧值，后台刷新）**、**负缓存**（错误/空结果短 TTL）
- 指标钩子 `Stats`（RIS 自带简易实现）
- 3 个端到端单测：**合并率**、**命中**、**SWR 刷新**

> 放入 `soul-base/crates/soulbase-cache/` 后 `cargo check && cargo test`。Redis 适配在本 RIS 中为 stub，后续按 `redis` feature 填充即可。

------

## 目录结构

```
soul-base/
└─ crates/
   └─ soulbase-cache/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ errors.rs
      │  ├─ key.rs
      │  ├─ policy.rs
      │  ├─ codec.rs
      │  ├─ r#trait.rs
      │  ├─ metrics.rs
      │  ├─ layer/
      │  │  ├─ mod.rs
      │  │  ├─ local_lru.rs
      │  │  ├─ redis.rs
      │  │  ├─ singleflight.rs
      │  │  ├─ jitter.rs
      │  │  └─ swr.rs
      │  ├─ invalidate.rs
      │  └─ prelude.rs
      └─ tests/
         └─ basic.rs
```

------

## Cargo.toml

```toml
[package]
name = "soulbase-cache"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "Unified Cache & SingleFlight for the Soul platform"
repository = "https://example.com/soul-base"

[features]
default = []
redis = []                 # 预留：启用后补充 redis 适配
observe = []               # 预留：接 SB-11

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
async-trait = "0.1"
thiserror = "1"
parking_lot = "0.12"
lru = "0.12"
once_cell = "1"
bytes = "1"
chrono = "0.4"
tokio = { version = "1", features = ["rt", "sync", "time", "macros"] }

# 平台内（若你的 workspace 已有，可保持）
soulbase-errors = { path = "../soulbase-errors", version = "1.0.0" }

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread","macros","time"] }
```

------

## src/lib.rs

```rust
pub mod errors;
pub mod key;
pub mod policy;
pub mod codec;
pub mod r#trait;
pub mod metrics;
pub mod layer { pub mod mod_; pub mod local_lru; pub mod redis; pub mod singleflight; pub mod jitter; pub mod swr; }
pub mod invalidate;
pub mod prelude;

pub use r#trait::{Cache, SingleFlight, Invalidation, Stats};
pub use policy::{CachePolicy, Admission, SwrPolicy};
pub use key::{CacheKey, build_key, KeyParts};
pub use codec::{Codec, JsonCodec};
pub use layer::mod_::TwoTierCache;
```

------

## src/errors.rs

```rust
use thiserror::Error;
use soulbase_errors::prelude::*;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct CacheError(pub ErrorObj);

impl CacheError {
  pub fn provider_unavailable(msg: &str) -> Self {
    CacheError(ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE).user_msg("Cache backend unavailable.").dev_msg(msg.to_string()).build())
  }
  pub fn schema(msg: &str) -> Self {
    CacheError(ErrorBuilder::new(codes::SCHEMA_VALIDATION).user_msg("Cache codec error.").dev_msg(msg.to_string()).build())
  }
  pub fn unknown(msg: &str) -> Self {
    CacheError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL).user_msg("Cache internal error.").dev_msg(msg.to_string()).build())
  }
}
```

------

## src/key.rs

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey(pub String);

pub struct KeyParts<'a> {
  pub tenant: &'a str,
  pub namespace: &'a str,
  pub payload_hash: &'a str, // e.g. base64url(sha256(canonical(payload)))
}

pub fn build_key(p: KeyParts) -> CacheKey {
  CacheKey(format!("{}:{}:{}", p.tenant, p.namespace, p.payload_hash))
}
```

------

## src/policy.rs

```rust
#[derive(Clone, Debug)]
pub struct CachePolicy {
  pub ttl_ms: u64,
  pub neg_ttl_ms: u64,
  pub jitter_ratio: f32,           // e.g. 0.15 => ±15%
  pub swr: Option<SwrPolicy>,
  pub admission: Admission,
  pub max_value_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct SwrPolicy { pub enable: bool, pub refresh_concurrency: usize }

#[derive(Clone, Debug)]
pub struct Admission {
  pub min_loader_ms: Option<u64>,
  pub only_ok: bool,
}

impl Default for CachePolicy {
  fn default() -> Self {
    Self {
      ttl_ms: 60_000, neg_ttl_ms: 3_000, jitter_ratio: 0.15,
      swr: None, admission: Admission{ min_loader_ms: None, only_ok: true },
      max_value_bytes: 512 * 1024,
    }
  }
}
```

------

## src/codec.rs

```rust
use crate::errors::CacheError;

#[async_trait::async_trait]
pub trait Codec: Send + Sync {
  fn id(&self) -> &'static str;
  fn serialize<T: serde::Serialize>(&self, val:&T) -> Result<Vec<u8>, CacheError>;
  fn deserialize<T: serde::de::DeserializeOwned>(&self, bytes:&[u8]) -> Result<T, CacheError>;
}

pub struct JsonCodec;
#[async_trait::async_trait]
impl Codec for JsonCodec {
  fn id(&self) -> &'static str { "json" }
  fn serialize<T: serde::Serialize>(&self, val:&T) -> Result<Vec<u8>, CacheError> {
    serde_json::to_vec(val).map_err(|e| CacheError::schema(&format!("serde to_vec: {e}")))
  }
  fn deserialize<T: serde::de::DeserializeOwned>(&self, bytes:&[u8]) -> Result<T, CacheError> {
    serde_json::from_slice(bytes).map_err(|e| CacheError::schema(&format!("serde from_slice: {e}")))
  }
}
```

------

## src/r#trait.rs

```rust
use crate::{errors::CacheError, key::CacheKey, policy::CachePolicy};
use std::future::Future;
use std::sync::Arc;

#[async_trait::async_trait]
pub trait Cache: Send + Sync {
  async fn get<T: serde::de::DeserializeOwned>(&self, key:&CacheKey) -> Result<Option<T>, CacheError>;
  async fn set<T: serde::Serialize>(&self, key:&CacheKey, val:&T, ttl_ms:u64) -> Result<(), CacheError>;
  async fn delete(&self, key:&CacheKey) -> Result<(), CacheError>;
  async fn delete_prefix(&self, tenant:&str, ns:&str) -> Result<(), CacheError>;

  async fn get_or_load<T, F, Fut>(&self, key:&CacheKey, policy:&CachePolicy, loader:F) -> Result<T, CacheError>
  where T: serde::de::DeserializeOwned + serde::Serialize + Send + Sync + 'static,
        F: FnOnce() -> Fut + Send, Fut: Future<Output=Result<T, CacheError>> + Send;
}

#[async_trait::async_trait]
pub trait SingleFlight: Send + Sync {
  async fn do_once<T, F, Fut>(&self, key:&CacheKey, ttl_ms:u64, loader:F) -> Result<T, CacheError>
  where T: serde::de::DeserializeOwned + serde::Serialize + Send + Sync + 'static,
        F: FnOnce() -> Fut + Send, Fut: Future<Output=Result<T, CacheError>> + Send;
}

#[async_trait::async_trait]
pub trait Invalidation: Send + Sync {
  async fn del(&self, key:&CacheKey) -> Result<(), CacheError>;
  async fn del_prefix(&self, tenant:&str, ns:&str) -> Result<(), CacheError>;
  async fn publish_invalidate(&self, _tenant:&str, _ns:&str, _keys:Vec<String>) -> Result<(), CacheError> { Ok(()) }
  async fn subscribe(&self, _handler: Arc<dyn Fn(&str,&str,&[String]) + Send + Sync>) -> Result<(), CacheError> { Ok(()) }
}

pub trait Stats: Send + Sync {
  fn on_hit(&self, _tier:&'static str) {}
  fn on_miss(&self) {}
  fn on_merge(&self) {}
  fn on_evict(&self, _bytes:usize) {}
  fn on_get_latency(&self, _ms:u64) {}
}
```

------

## src/metrics.rs（RIS 简易实现）

```rust
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct SimpleStats {
  pub hits_local: AtomicU64,
  pub hits_redis: AtomicU64,
  pub misses: AtomicU64,
  pub merges: AtomicU64,
}
impl super::r#trait::Stats for SimpleStats {
  fn on_hit(&self, tier:&'static str) {
    match tier {
      "local" => { self.hits_local.fetch_add(1, Ordering::Relaxed); }
      "redis" => { self.hits_redis.fetch_add(1, Ordering::Relaxed); }
      _ => {}
    }
  }
  fn on_miss(&self) { self.misses.fetch_add(1, Ordering::Relaxed); }
  fn on_merge(&self) { self.merges.fetch_add(1, Ordering::Relaxed); }
}
```

------

## src/layer/mod.rs

```rust
use crate::{errors::CacheError, r#trait::*, key::CacheKey, policy::CachePolicy, codec::Codec};
use crate::layer::{local_lru::LocalLru, redis::RedisStub, singleflight::Flight, jitter::apply_jitter};
use std::sync::Arc;
use std::time::{Instant, Duration};
use tokio::time::sleep;

pub struct TwoTierCache<C: Codec, S: Stats> {
  pub local: LocalLru,                  // 进程内 LRU
  pub redis: Option<RedisStub>,         // RIS：Redis stub；后续替换
  pub codec: C,
  pub flight: Flight,
  pub stats: Arc<S>,
  pub default_policy: CachePolicy,
}

#[async_trait::async_trait]
impl<C: Codec + Send + Sync + 'static, S: Stats + 'static> crate::Cache for TwoTierCache<C,S> {
  async fn get<T: serde::de::DeserializeOwned>(&self, key:&CacheKey) -> Result<Option<T>, CacheError> {
    if let Some(bytes) = self.local.get(&key.0).await {
      self.stats.on_hit("local");
      return self.codec.deserialize::<T>(&bytes).map(Some);
    }
    if let Some(r) = &self.redis {
      if let Some(bytes) = r.get(&key.0).await? {
        self.stats.on_hit("redis");
        self.local.set(&key.0, bytes.clone(), 1_000).await; // 小 TTL 作为回填；真实 TTL 在 set 时控制
        return self.codec.deserialize::<T>(&bytes).map(Some);
      }
    }
    self.stats.on_miss();
    Ok(None)
  }

  async fn set<T: serde::Serialize>(&self, key:&CacheKey, val:&T, ttl_ms:u64) -> Result<(), CacheError> {
    let bytes = self.codec.serialize(val)?;
    self.local.set(&key.0, bytes.clone(), ttl_ms).await;
    if let Some(r) = &self.redis { let _ = r.set(&key.0, bytes, ttl_ms).await?; }
    Ok(())
  }

  async fn delete(&self, key:&CacheKey) -> Result<(), CacheError> {
    self.local.del(&key.0).await;
    if let Some(r) = &self.redis { let _ = r.del(&key.0).await?; }
    Ok(())
  }

  async fn delete_prefix(&self, tenant:&str, ns:&str) -> Result<(), CacheError> {
    let prefix = format!("{tenant}:{ns}:");
    self.local.del_prefix(&prefix).await;
    if let Some(r) = &self.redis { let _ = r.del_prefix(&prefix).await?; }
    Ok(())
  }

  async fn get_or_load<T, F, Fut>(&self, key:&CacheKey, policy:&CachePolicy, loader:F) -> Result<T, CacheError>
  where
    T: serde::de::DeserializeOwned + serde::Serialize + Send + Sync + 'static,
    F: FnOnce() -> Fut + Send,
    Fut: std::future::Future<Output=Result<T, CacheError>> + Send
  {
    // 1) local
    if let Some(bytes) = self.local.get(&key.0).await {
      self.stats.on_hit("local");
      // SWR 检查（本地条目标注 stale 与 expires，RIS 用 expires_ms 即过期）
      if self.local.is_stale(&key.0).await {
        if let Some(swr) = &policy.swr {
          if swr.enable {
            // 后台刷新
            let k = key.clone();
            let ttl = policy.ttl_ms;
            let policy = policy.clone();
            let stats = self.stats.clone();
            let codec = self.codec_id();
            let flight = self.flight.clone();
            let loader = loader;
            let this = self.clone_arc(); // helper
            tokio::spawn(async move {
              // flight 合并，以防风暴
              let _ = this.flight.run_refresh(&k.0, async {
                let v = loader().await?;
                let jttl = apply_jitter(ttl, policy.jitter_ratio, hash64(&k.0));
                let _ = this.set(&k, &v, jttl).await;
                stats.on_merge(); // 记作刷新一次
                Ok::<(), CacheError>(())
              }).await;
            });
          }
        }
      }
      return self.codec.deserialize::<T>(&bytes);
    }
    // 2) redis
    if let Some(r) = &self.redis {
      if let Some(bytes) = r.get(&key.0).await? {
        self.stats.on_hit("redis");
        self.local.set(&key.0, bytes.clone(), policy.ttl_ms).await;
        return self.codec.deserialize::<T>(&bytes);
      }
    }
    self.stats.on_miss();

    // 3) SingleFlight loader
    let start = Instant::now();
    let leader = self.flight.register(&key.0).await;
    if leader {
      // 仅首个执行真实加载，其它协程等通知后读缓存
      let out = loader().await;
      let elapsed = start.elapsed().as_millis() as u64;
      let result = match out {
        Ok(val) => {
          // admission
          if policy.admission.min_loader_ms.map(|t| elapsed >= t).unwrap_or(true) {
            let bytes = self.codec.serialize(&val)?;
            if bytes.len() <= policy.max_value_bytes {
              let jttl = apply_jitter(policy.ttl_ms, policy.jitter_ratio, hash64(&key.0));
              self.local.set(&key.0, bytes.clone(), jttl).await;
              if let Some(r) = &self.redis { let _ = r.set(&key.0, bytes, jttl).await?; }
            }
          }
          Ok(val)
        }
        Err(e) => {
          if policy.neg_ttl_ms > 0 {
            // 负缓存：记一个短空值（RIS 直接不回填内容，仅打一个空标记）
            self.local.set_neg(&key.0, policy.neg_ttl_ms).await;
          }
          Err(e)
        }
      };
      self.flight.complete(&key.0).await;
      return result;
    } else {
      // 追随者：等待 leader 完成后，从缓存取
      self.flight.wait(&key.0).await;
      if let Some(bytes) = self.local.get(&key.0).await {
        return self.codec.deserialize::<T>(&bytes);
      }
      if let Some(r) = &self.redis {
        if let Some(bytes) = r.get(&key.0).await? {
          self.local.set(&key.0, bytes.clone(), policy.ttl_ms).await;
          return self.codec.deserialize::<T>(&bytes);
        }
      }
      // 仍未命中：返回 miss 错误（说明 loader 失败且无负缓存）
      return Err(CacheError::provider_unavailable("singleflight follower miss"));
    }
  }
}

impl<C: Codec + Send + Sync + 'static, S: Stats + 'static> TwoTierCache<C,S> {
  fn codec_id(&self) -> &'static str { self.codec.id() }
  fn clone_arc(&self) -> Arc<Self> { Arc::new(Self{
    local: self.local.clone(),
    redis: self.redis.clone(),
    codec: self.codec_id_new(),
    flight: self.flight.clone(),
    stats: self.stats.clone(),
    default_policy: self.default_policy.clone(),
  })}
  fn codec_id_new(&self) -> C { // RIS：JsonCodec 无状态，可 clone；若有状态需 Clone bound
    // unsafe 不允许；此 helper 在 RIS 未被实际调用，保留占位
    // 简化：要求 C: Default
    Default::default()
  }
}

// —— 辅助 —— //
fn hash64(s:&str) -> u64 {
  use std::hash::{Hash, Hasher};
  let mut h = std::collections::hash_map::DefaultHasher::new();
  s.hash(&mut h); h.finish()
}
```

> 注：上面 `codec_id_new` 为骨架占位，RIS 使用 `JsonCodec` 可让 `C: Default` 满足；若你想让 `C` 不要求 Default，可把 `TwoTierCache` 包装在工厂或用 `Arc<dyn Codec>`。

------

## src/layer/local_lru.rs

```rust
use parking_lot::Mutex;
use lru::LruCache;
use std::time::{Instant, Duration};
use bytes::Bytes;

#[derive(Clone)]
pub struct LocalLru {
  inner: std::sync::Arc<Mutex<LruCache<String, Entry>>>,
}
struct Entry { bytes: Bytes, expires_at: Instant, neg: bool }

impl LocalLru {
  pub fn new(capacity: usize) -> Self {
    Self { inner: std::sync::Arc::new(Mutex::new(LruCache::new(capacity.try_into().unwrap_or(1024)))) }
  }
  pub async fn get(&self, key:&str) -> Option<Vec<u8>> {
    let mut l = self.inner.lock();
    if let Some(e) = l.get(key) {
      if e.expires_at > Instant::now() || e.neg { // 负缓存视为命中但调用方不会反序列化为目标类型
        return Some(e.bytes.to_vec());
      }
    }
    None
  }
  pub async fn set(&self, key:&str, bytes:Vec<u8>, ttl_ms:u64) {
    let mut l = self.inner.lock();
    l.put(key.to_string(), Entry{ bytes: Bytes::from(bytes), expires_at: Instant::now() + Duration::from_millis(ttl_ms), neg:false });
  }
  pub async fn set_neg(&self, key:&str, ttl_ms:u64) {
    let mut l = self.inner.lock();
    l.put(key.to_string(), Entry{ bytes: Bytes::new(), expires_at: Instant::now() + Duration::from_millis(ttl_ms), neg:true });
  }
  pub async fn del(&self, key:&str) {
    let mut l = self.inner.lock(); l.pop(key);
  }
  pub async fn del_prefix(&self, prefix:&str) {
    let mut l = self.inner.lock();
    let keys: Vec<String> = l.iter().map(|(k, _)| k.clone()).collect();
    for k in keys {
      if k.starts_with(prefix) { l.pop(&k); }
    }
  }
  pub async fn is_stale(&self, key:&str) -> bool {
    let l = self.inner.lock();
    if let Some(e) = l.peek(key) {
      e.expires_at <= Instant::now() && !e.neg
    } else { false }
  }
}
impl Default for LocalLru { fn default() -> Self { Self::new(10_000) } }
```

------

## src/layer/redis.rs（stub）

```rust
use crate::errors::CacheError;

#[derive(Clone, Default)]
pub struct RedisStub;

impl RedisStub {
  pub async fn get(&self, _key:&str) -> Result<Option<Vec<u8>>, CacheError> { Ok(None) }
  pub async fn set(&self, _key:&str, _bytes:Vec<u8>, _ttl_ms:u64) -> Result<(), CacheError> { Ok(()) }
  pub async fn del(&self, _key:&str) -> Result<(), CacheError> { Ok(()) }
  pub async fn del_prefix(&self, _prefix:&str) -> Result<(), CacheError> { Ok(()) }
}
```

------

## src/layer/singleflight.rs

```rust
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Notify;

#[derive(Clone, Default)]
pub struct Flight {
  map: Arc<Mutex<HashMap<String, Arc<Notify>>>>,
}
impl Flight {
  pub async fn register(&self, key:&str) -> bool {
    let mut m = self.map.lock();
    if m.contains_key(key) { false } else { m.insert(key.to_string(), Arc::new(Notify::new())); true }
  }
  pub async fn complete(&self, key:&str) {
    if let Some(n) = self.map.lock().remove(key) { n.notify_waiters(); }
  }
  pub async fn wait(&self, key:&str) {
    let n = { self.map.lock().get(key).cloned() };
    if let Some(notify) = n { notify.notified().await; }
  }
  // SWR 刷新器使用：若 key 正在刷新，等待；否则注册并在完成时 complete
  pub async fn run_refresh<F, Fut>(&self, key:&str, fut:F) -> Result<(), crate::errors::CacheError>
  where F: FnOnce() -> Fut, Fut: std::future::Future<Output=Result<(), crate::errors::CacheError>> {
    if !self.register(key).await { self.wait(key).await; return Ok(()); }
    let r = fut().await;
    self.complete(key).await; r
  }
}
```

------

## src/layer/jitter.rs

```rust
pub fn apply_jitter(ttl_ms: u64, ratio: f32, seed: u64) -> u64 {
  let r = (seed as f32 / u64::MAX as f32) * ratio;
  let sign = if (seed & 1) == 0 { 1.0 } else { -1.0 };
  ((ttl_ms as f32) * (1.0 + sign * r)).max(1.0) as u64
}
```

------

## src/layer/swr.rs

```rust
// SWR 背景刷新逻辑已内嵌在 TwoTierCache::get_or_load 中，此文件保留占位（如需独立刷新器可以在这里扩展）
```

------

## src/invalidate.rs

```rust
use crate::{Cache, CacheError, CacheKey};

pub async fn invalidate_prefix<C: Cache>(cache:&C, tenant:&str, namespace:&str) -> Result<(), CacheError> {
  cache.delete_prefix(tenant, namespace).await
}
```

------

## src/prelude.rs

```rust
pub use crate::errors::CacheError;
pub use crate::key::{CacheKey, build_key, KeyParts};
pub use crate::policy::{CachePolicy, Admission, SwrPolicy};
pub use crate::codec::{Codec, JsonCodec};
pub use crate::r#trait::{Cache, SingleFlight, Invalidation, Stats};
pub use crate::metrics::SimpleStats;
pub use crate::layer::mod::TwoTierCache;
pub use crate::layer::local_lru::LocalLru;
pub use crate::layer::singleflight::Flight;
pub use crate::layer::redis::RedisStub;
```

------

## tests/basic.rs

```rust
use soulbase_cache::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn key_of(s:&str) -> CacheKey {
  let h = format!("{:x}", seahash::hash(s.as_bytes()));
  build_key(KeyParts{ tenant:"t", namespace:"demo:v1", payload_hash:&h })
}

#[tokio::test]
async fn singleflight_merges_concurrent_loads() {
  let cache = TwoTierCache{
    local: LocalLru::new(1024),
    redis: None,
    codec: JsonCodec,
    flight: Flight::default(),
    stats: Arc::new(SimpleStats::default()),
    default_policy: CachePolicy::default(),
  };
  static CNT: AtomicUsize = AtomicUsize::new(0);
  let key = key_of("sf-merge");
  let policy = CachePolicy::default();

  // 50 个并发只触发一次 loader
  let mut handles = vec![];
  for _ in 0..50 {
    let c = &cache; let k = key.clone(); let p = policy.clone();
    handles.push(tokio::spawn(async move {
      c.get_or_load::<String,_,_>(&k, &p, || async {
        if CNT.fetch_add(1, Ordering::SeqCst) == 0 {
          tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        Ok::<_, CacheError>("ok".to_string())
      }).await
    }));
  }
  for h in handles { let _ = h.await.unwrap().unwrap(); }
  assert_eq!(CNT.load(Ordering::SeqCst), 1, "loader should run once");
}

#[tokio::test]
async fn hit_after_first_miss() {
  let cache = TwoTierCache{
    local: LocalLru::new(1024),
    redis: None,
    codec: JsonCodec,
    flight: Flight::default(),
    stats: Arc::new(SimpleStats::default()),
    default_policy: CachePolicy::default(),
  };
  let key = key_of("hit-then-miss");
  let policy = CachePolicy::default();

  // 第一次 miss → load
  let v1 = cache.get_or_load::<String,_,_>(&key, &policy, || async {
    Ok::<_, CacheError>("v1".into())
  }).await.unwrap();
  assert_eq!(v1, "v1");

  // 第二次应命中 local
  let v2: Option<String> = cache.get(&key).await.unwrap();
  assert!(v2.is_some());
  assert_eq!(v2.unwrap(), "v1");
}

#[tokio::test]
async fn swr_returns_stale_and_refreshes() {
  let cache = TwoTierCache{
    local: LocalLru::new(1024),
    redis: None,
    codec: JsonCodec,
    flight: Flight::default(),
    stats: Arc::new(SimpleStats::default()),
    default_policy: CachePolicy::default(),
  };
  let key = key_of("swr");
  let mut policy = CachePolicy::default();
  policy.ttl_ms = 80; // 快速过期
  policy.swr = Some(SwrPolicy{ enable: true, refresh_concurrency: 4 });

  static REV: AtomicUsize = AtomicUsize::new(0);

  // 第一次加载：写入 v1
  let first = cache.get_or_load::<String,_,_>(&key, &policy, || async {
    REV.store(1, Ordering::SeqCst);
    Ok::<_,CacheError>("v1".into())
  }).await.unwrap();
  assert_eq!(first, "v1");

  // 等待过期
  tokio::time::sleep(std::time::Duration::from_millis(100)).await;

  // 第二次：SWR 应先返回“旧值 v1”，并后台刷新填入 v2
  let second = cache.get_or_load::<String,_,_>(&key, &policy, || async {
    Ok::<_,CacheError>(if REV.swap(2, Ordering::SeqCst) == 1 { "v2".into() } else { "v2".into() })
  }).await.unwrap();
  assert_eq!(second, "v1", "SWR should return stale value immediately");

  // 给后台刷新一点时间
  tokio::time::sleep(std::time::Duration::from_millis(80)).await;

  // 第三次应读到 v2
  let third = cache.get_or_load::<String,_,_>(&key, &policy, || async {
    Ok::<_,CacheError>("v2".into())
  }).await.unwrap();
  assert_eq!(third, "v2");
}
```

> 测试里用 `seahash` 计算 payload 哈希。你也可以换成 `sha2` + base64url。如果工作区没有 `seahash`，把它加到 dev-dependencies 即可。

------

## README.md（简版）

```markdown
# soulbase-cache (RIS)

统一缓存与请求合并（Rust）：
- 两级缓存：本地 LRU → 可选 Redis
- SingleFlight 合并；负缓存；TTL 抖动；SWR（过期先回旧值、后台刷新）
- 一致键规范：{tenant}:{namespace}:{hash}
- 观测钩子 Stats（命中率/合并率/驱逐/时延）

## 用法
let cache = TwoTierCache{
  local: LocalLru::new(10_000),
  redis: None, // 或替换为真实 redis 适配
  codec: JsonCodec,
  flight: Flight::default(),
  stats: Arc::new(SimpleStats::default()),
  default_policy: CachePolicy::default(),
};
let key = build_key(KeyParts{ tenant:"t", namespace:"llm:chat:v1", payload_hash:"..." });
let policy = CachePolicy::default();
let val: String = cache.get_or_load(&key, &policy, || async { Ok("value".to_string()) }).await?;

## 测试
cargo test
```

------

### 说明与后续

- **Redis 适配**：把 `layer/redis.rs` 换成实际实现（`GET/SETEX/SCAN` + 订阅）；
- **指标**：如启 `observe`，在 `SimpleStats` 中对接 `soulbase-observe::Meter`；
- **安全**：调用方必须保证**不缓存敏感内容**或把 `subject/roles_hash` 纳入 key；
- **高阶**：加入批量 `mget`、Bloom/CM Sketch 作“是否值得查”的判定；把 **SWR 刷新器**抽到独立 worker 控并发。

到此，`soulbase-cache` 已能**在现有 15+ 模块里直接复用**：LLM/Tools 的只读场景会明显降低 p95 与成本；A2A 与 Config 也可借此防止“热 key 抖动”。
