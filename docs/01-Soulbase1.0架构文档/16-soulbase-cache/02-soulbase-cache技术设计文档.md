# 文档 SB-16-TD：`soulbase-cache` 技术设计

（Unified Cache & SingleFlight · 两级缓存 · 负缓存 · SWR · 观测）

> 对应规约：SB-16
>  目标：给出 **可落地** 的 Rust 设计与接口：`Cache/SingleFlight/Policy/Codec/Invalidation` trait，**两级实现**（本地 LRU + Redis），**负缓存/TTL 抖动/SWR/请求合并**的算法细节，**键名规范**与 **SB-11/14/05/07/08/09/15** 的对接位。
>  说明：本 TD 不包含 RIS 代码，但所有接口均为“可直接编码”的形态。

------

## 1. Crate 结构与 Feature 规划

```
soulbase-cache/
  src/
    lib.rs
    errors.rs            # CacheError → soulbase-errors 稳定码映射
    key.rs               # CacheKey 生成与规范（tenant/ns/hash）
    policy.rs            # CachePolicy / TTL / Jitter / SWR / Admission / Negative
    codec.rs             # Codec trait：serialize/deserialize (+ 可选压缩)
    trait.rs             # Cache / SingleFlight / Invalidation / Stats 接口
    metrics.rs           # SB-11 观测：hit/miss/merge/evict/bytes/latency
    layer/
      mod.rs             # 两级 orchestrator（local → redis → loader）
      local_lru.rs       # 进程内 LRU（容量/计数/权重）
      redis.rs           # Redis 适配（get/mget/set/del/scan/publish）
      singleflight.rs    # 请求合并（一次加载）
      swr.rs             # Stale-While-Revalidate 背景刷新器
      jitter.rs          # TTL 抖动函数
      negative.rs        # 负缓存编码/策略
    invalidate.rs        # 主动/前缀失效 & 订阅（Redis PubSub/Stream）
    prelude.rs
```

**features**

- `redis`：启用 Redis 适配（`redis`/`deadpool-redis`）
- `compress-snappy` / `compress-deflate`：Codec 压缩
- `observe`：输出指标（SB-11）
- `serde-json`（默认）/`cbor`：序列化格式切换

------

## 2. 键名规范与 Key Builder（`key.rs`）

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey(String);

pub struct KeyParts<'a> {
  pub tenant: &'a str,
  pub namespace: &'a str,     // e.g. "llm:chat:v1" / "tools:http:v1" / "a2a:peer"
  pub payload_hash: &'a str,  // base64url(sha256(canonical(payload)))
}

pub fn build_key(p: KeyParts) -> CacheKey {
  // {tenant}:{namespace}:{hash} —— 不允许冒号之外的分隔符（避免跨系统差异）
  CacheKey(format!("{}:{}:{}", p.tenant, p.namespace, p.payload_hash))
}
```

- **要求**：
  - 必含 `tenant`；必要时将 `subject`/`model_alias/roles_hash` 纳入 `namespace` 或 hash 的原材料；
  - **payload 的哈希**必须用统一 `canonical_json`（来自 `soulbase-crypto`），避免签名/hash 不一致；
  - 大 Key 禁止（>1KB 直接拒绝）。

------

## 3. 策略与配置（`policy.rs`）

```rust
#[derive(Clone, Debug)]
pub struct CachePolicy {
  pub ttl_ms: u64,            // 正向 TTL
  pub neg_ttl_ms: u64,        // 负缓存 TTL（错误/空结果）
  pub jitter_ratio: f32,      // 0.1 → ±10%
  pub swr: Option<SwrPolicy>, // SWR 开关与后台并发度
  pub admission: Admission,   // 纳入条件
  pub max_value_bytes: usize, // 过大值不纳入缓存
}

#[derive(Clone, Debug)]
pub struct SwrPolicy { pub enable: bool, pub refresh_concurrency: usize }

#[derive(Clone, Debug)]
pub struct Admission {
  pub min_loader_ms: Option<u64>,    // 仅当真实加载 > 阈值才纳入（避免“缓存鸡肋”）
  pub only_ok: bool,                 // 非 OK 结果不纳入（负缓存除外）
}

pub fn apply_jitter(ttl_ms: u64, ratio: f32, seed: u64) -> u64 {
  // ttl' = ttl * (1 ± r)，r ∈ [0, ratio]；seed 可来自 key hash → 稳定抖动
  let r = (seed as f32 / u64::MAX as f32) * ratio;
  let sign = if (seed & 1) == 0 { 1.0 } else { -1.0 };
  (ttl_ms as f32 * (1.0 + sign * r)).max(1.0) as u64
}
```

- **默认策略**：`ttl=60s`、`neg_ttl=3s`、`jitter=±15%`、`SWR off`、`admission.only_ok=true`。
- **租户/namespace** 可覆盖策略（从 SB-03 config 快照加载）。

------

## 4. 序列化与压缩（`codec.rs`）

```rust
#[async_trait::async_trait]
pub trait Codec: Send + Sync {
  fn id(&self) -> &'static str;                         // "json" | "cbor+snappy"
  fn serialize<T: serde::Serialize>(&self, val:&T) -> Result<Vec<u8>, CacheError>;
  fn deserialize<T: serde::de::DeserializeOwned>(&self, bytes:&[u8]) -> Result<T, CacheError>;
}
```

- 实现：`JsonCodec`（默认）、`CborCodec`；如启 `compress-*`，在 serialize 带压缩，反序列化自动解压。
- **不缓存**含敏感字段的结构（由调用方前置过滤；cache 层不做字段级脱敏，仅体积/类型校验）。

------

## 5. 核心 Trait（`trait.rs`）

```rust
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
  async fn publish_invalidate(&self, tenant:&str, ns:&str, keys:Vec<String>) -> Result<(), CacheError>; // Redis PubSub/Stream
  async fn subscribe(&self, handler: Arc<dyn Fn(&str, &str, &[String]) + Send + Sync>) -> Result<(), CacheError>;
}

pub trait Stats: Send + Sync {
  fn on_hit(&self, tier:&'static str);
  fn on_miss(&self);
  fn on_merge(&self);
  fn on_evict(&self, bytes:usize);
  fn on_get_latency(&self, ms:u64);
}
```

------

## 6. 两级 orchestrator（`layer/mod.rs`）

**逻辑顺序**：`local → redis → loader`；并发请求在 **SingleFlight** 合并，loader 后回填两级缓存。

```rust
pub struct TwoTierCache<L: Local, R: Redis, C: Codec, SF: SingleFlight, S: Stats> {
  pub local: L, pub redis: Option<R>, pub codec: C, pub flight: SF, pub stats: S,
  pub default_policy: CachePolicy,
}

#[async_trait::async_trait]
impl<L,R,C,SF,S> Cache for TwoTierCache<L,R,C,SF,S>
where L: Local, R: Redis, C: Codec, SF: SingleFlight, S: Stats
{
  async fn get_or_load<T, F, Fut>(&self, key:&CacheKey, policy:&CachePolicy, loader:F) -> Result<T, CacheError> { ... }
  // 细节：
  // 1) 查 local：命中 → 如果未过期：返回；若过期且 SWR→返回旧值 + 异步刷新；否则继续
  // 2) 查 redis：命中 → 反序列化；写入 local（jitter）→ 返回
  // 3) SingleFlight:
  //    - flight.do_once(key, ttl, async || -> loader().await)
  //    - 成功：admission 检查→ set local(+jitter), set redis(+jitter)；失败：写负缓存（短 ttl）
  // 4) 记录 stats（hit/miss/merge/get_latency）
}
```

### 6.1 本地 LRU（`layer/local_lru.rs`）

- **实现**：`lru` crate（或 `mini-lru`）+ `parking_lot::Mutex`；
- **权重**：按 value 字节数（序列化后长度）；
- **驱逐**：记录 evict 计数与字节；
- **条目结构**：`{bytes, expires_at, headers?}`；SWR 需要额外 `stale_at`。

### 6.2 Redis 适配（`layer/redis.rs`）

- **命令**：`GET/SETEX/MGET/EVAL`；
- **编码**：`{codec_id}:{compressed?}:{payload}` 一段 bytes；
- **批量失效**：`SCAN` + 模式 `tenant:ns:*`（谨慎使用；推荐维护**索引集合**或使用 Redis stream 订阅失效消息）。
- **错误降级**：连接失败/超时 → 仅用 local；记录 `cache_redis_error_total`。

### 6.3 SingleFlight（`layer/singleflight.rs`）

- **实现**：key → `tokio::sync::Mutex<Option<JoinHandle<Result<Bytes, CacheError>>>>`；
- 首个请求创建 handle，其余 await；完成后移除 entry；
- **异常**：loader panic/错误 → 所有 await 侧返回相同错误；负缓存写入。

### 6.4 SWR 刷新器（`layer/swr.rs`）

- **策略**：过期但可用 → 立即返回旧值；spawn 刷新任务（并发度由 `refresh_concurrency` 控制，防风暴）。
- 刷新成功回填；失败记录 `cache_swr_refresh_failed_total`。

### 6.5 负缓存（`layer/negative.rs`）

- **编码**：`NEG:{code}` 作为 value 前缀（或头），反序列化时识别为“空/错误”类；
- **TTL**：`neg_ttl_ms`；
- **适用**：`404/空集合`/`错误码`（可配置仅空集合，不缓存错误）。

------

## 7. 观测与指标（`metrics.rs`）

- 计数器：`cache_hit_total{tier="local|redis"}`、`cache_miss_total`、`cache_merge_total`、`cache_evict_total`
- 直方图：`cache_get_ms_bucket`（5/10/20/50/100/200/500ms）
- 量纲：`cache_value_bytes{dir="in|out"}`
- 标签最小集：`tenant, namespace`（从 KeyParts 解析；**不得**记录 payload）

------

## 8. 错误与稳定码（`errors.rs`）

```rust
#[derive(thiserror::Error, Debug)]
pub enum CacheError {
  #[error("{0}")] Obj(soulbase_errors::prelude::ErrorObj)
}
impl CacheError {
  pub fn provider_unavailable(e:&str)->Self { ... }     // Redis 断链/超时
  pub fn schema(e:&str)->Self { ... }                   // 反序列化/编码错误
  pub fn conflict(e:&str)->Self { ... }                 // SingleFlight 内部状态异常（理论上不应出现）
}
```

> 所有对外错误上抛到调用方**仍需**映射 SB-02 公共视图。

------

## 9. 与周边模块的集成点

- **SB-07 LLM**：
  - key = `{tenant}:llm:chat:v1:{hash(messages+model_alias+response_format)}`；
  - 结构化输出缓存：value 为 JSON；负缓存：provider 错误短 ttl；
  - 建议**仅缓存 OK**，把 `allow_sensitive=false` 的输出纳入缓存（避免泄露）。
- **SB-08/06 Tools/Sandbox (`net.http`)**：
  - 只读 GET 请求：key = `{tenant}:tools:http:v1:{hash(method+url+headers?)}`；
  - 默认 **SWR=on**；ETag/Last-Modified 可进 `headers?` 字段辅助刷新。
- **SB-09 Storage**：
  - 热点读：缓存以查询条件 hash 做 key；**强一致写**路径必须主动失效（`del_prefix(tenant, ns)`）。
- **SB-15 A2A**：
  - 对端 `PeerMetadata`、`JWKS`；key = `{tenant}:a2a:peer:{peer_id_hash}`；
  - TTL 短 + 订阅轮换事件触发主动失效。
- **SB-03 Config**：
  - `Snapshot` 类对象可暂存 1–5 分钟；policy 必须版本感知（`config_checksum` 参与 hash）。
- **SB-11 Observe**：
  - 所有 API 调用 `Stats`；错误落公共视图；**严禁**把 value 打进日志。

------

## 10. 并发 & 背压策略

- `get_or_load` 内部对每个 key **最多 1 个 loader**；其它 await；
- 背景刷新与 loader 需要**全局并发上限**（`Semaphore`），防止高峰时任务风暴；
- Redis 侧 `MGET` 聚合（有批量场景可扩展）；**超时**必须显式（如 2–10ms），失败即降级 local。

------

## 11. 安全与最小披露

- **禁止缓存**：含凭证/个人敏感字段的结构；若必须缓存，key 需纳入 `subject/roles_hash`；
- Value 存储统一经 `Codec` 序列化；不要直接存文本；
- 大对象（>NKB）默认压缩；超过 `max_value_bytes` 拒绝缓存。

------

## 12. 测试与验收（契约/基准/混沌）

- 契约：
  - 键规范/租户校验/负缓存写入/TTL 抖动/删除前缀；
  - SingleFlight：合并率 ≥ 90%（在 32 并发相同 key 下）。
- 基准：
  - local 命中 p95 ≤ 0.2ms；redis 命中 p95 ≤ 2ms；
  - SWR：过期返回旧值 + 刷新成功率指标。
- 混沌：
  - Redis 断链/超时/慢查询 → 业务仍能返回（local 命中或 miss 退 loader）。

------

## 13. RIS 预告（实现要点）

- **本地 LRU**：`lru::LruCache<K, Entry>` + `parking_lot::Mutex`；权重=字节数；
- **Redis**：`deadpool-redis` 连接池；`GET/SETEX`；`del_prefix` 用 `SCAN`（RIS 可简化）；
- **SingleFlight**：`tokio::sync::Mutex<HashMap<CacheKey, JoinHandle<_>>>`；
- **SWR**：`tokio::task::spawn` + `Semaphore` 限并发；
- **Codec**：`serde_json` + 可选 `snap` 压缩；
- **Metrics**：若启 `observe`，用 `soulbase-observe` 的 `Meter`/`Logger` 打点。

------

### 小结

本 TD 给出 `soulbase-cache` 的**边界清晰、可立即实现**的接口与算法：

- 统一键名、两级缓存、请求合并、负缓存与 SWR；
- 与周边模块的**具体 key 策略与集成位**；
- 充分的观测与安全约束。

如果你确认无误，我将按“三件套”输出 **SB-16-RIS（最小可运行骨架）**：本地 LRU + SingleFlight + 可选 Redis 适配（stub），并附 2–3 个端到端单测（合并率、命中率、SWR 刷新）。
