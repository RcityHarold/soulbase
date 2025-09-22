# 文档 SB-19-TD：`soulbase-net` 技术设计

（Resilient HTTP Client · 超时/重试/断路器 · 安全白名单 · 观测与治理）

> 对应规约：SB-19
>  目标：给出**可落地**的设计与接口：`NetClient/NetRequest/NetResponse/NetPolicy`、`Interceptor` 链、**重试与断路器算法**、**DNS/TLS/私网过滤**、**与 SB-11/14/06/07/15/16/03 的接线**、错误映射与测试基线。
>  说明：本 TD 只定义结构/算法与接口形状；具体实现放在 RIS。

------

## 1. Crate 结构与 Feature 规划

```
soulbase-net/
  src/
    lib.rs
    errors.rs                 # NetError → SB-02 稳定码映射（PROVIDER.UNAVAILABLE / …TIMEOUT / AUTH.FORBIDDEN / SCHEMA.VALIDATION_FAILED）
    types.rs                  # NetRequest / NetResponse / Body / Json / Stream
    policy.rs                 # NetPolicy / RetryPolicy / Backoff / CircuitBreakerPolicy / Redirect / TLS / DNS / Proxy / Security
    client.rs                 # NetClient trait + Builder
    interceptors/
      mod.rs                  # Interceptor trait（before/after/on_error）
      trace_ua.rs             # Trace & User-Agent 注入
      bearer.rs               # Bearer/M2M token 注入
      a2a_jws.rs              # A2A JWS detached 签名（SB-18 crypto）
      sandbox_guard.rs        # 私网屏蔽/域名白名单（与 SB-06）
      cache_hook.rs           # GET 的 ETag/SWR 钩子（与 SB-16）
      qos_bytes.rs            # 字节计量/预算（与 SB-14）
    runtime/
      dns.rs                  # DNS 解析策略（TTL/Happy-Eyeballs）
      cbreaker.rs             # 断路器状态机（三态：Closed|Open|HalfOpen）
      retry.rs                # 幂等重试 + 指数退避 + Retry-After
      limiter.rs              # host/tenant 限并发 + 限速
      pool.rs                 # 连接池参数与统计
    metrics.rs                # SB-11 指标与 Trace hooks
    config.rs                 # PolicyResolver（从 SB-03 config 快照解析）
    prelude.rs
```

**features**

- `http2`（默认）/`http3`（可选）
- `observe`（SB-11 指标与 trace）
- `qos`（SB-14 字节计量）
- `crypto`（SB-18 JWS 拦截器）
- `sandbox`（SB-06 出网白名单）
- `cache`（SB-16 ETag/SWR 钩子）

------

## 2. 核心类型（`types.rs`）

```rust
#[derive(Clone, Debug)]
pub struct NetRequest {
  pub method: Method,                     // GET/POST/PUT/DELETE/HEAD/OPTIONS/PATCH
  pub url: String,                        // 绝对URL
  pub headers: Vec<(String, String)>,     // 大小写不敏感；内部会规范化
  pub query: Option<Vec<(String, String)>>, // 附加查询；与URL冲突时覆盖
  pub body: Body,                         // Empty | Json(Value) | Bytes(Vec<u8>) | Stream
  pub timeout: TimeoutCfg,                // connect/ttfb/total/read/write
  pub idempotent: bool,                   // 非GET等方法需显式标注才允许重试
  pub policy_key: Option<String>,         // (tenant, host) 解析策略时作为hint
}

#[derive(Clone, Debug)]
pub enum Body { Empty, Json(serde_json::Value), Bytes(bytes::Bytes), Stream(BoxBody) }

#[derive(Clone, Debug)]
pub struct TimeoutCfg { pub connect_ms:u64, pub ttfb_ms:u64, pub total_ms:u64, pub read_ms:u64, pub write_ms:u64 }

#[derive(Clone, Debug)]
pub struct NetResponse {
  pub status: u16,
  pub headers: Vec<(String, String)>,
  pub content_type: Option<String>,
  pub body: RespBody,                     // Bytes | Stream
  pub elapsed: Elapsed,                   // connect/ttfb/total
  pub bytes_in: u64,
}

pub enum RespBody { Bytes(bytes::Bytes), Stream(BoxStream) }

pub struct Elapsed { pub connect_ms:u64, pub ttfb_ms:u64, pub total_ms:u64 }
```

------

## 3. 策略与配置（`policy.rs`）

```rust
#[derive(Clone, Debug)]
pub struct NetPolicy {
  pub retry: RetryPolicy,
  pub cbreaker: CircuitBreakerPolicy,
  pub redirect: RedirectPolicy,
  pub tls: TlsPolicy,
  pub dns: DnsPolicy,
  pub proxy: Option<ProxyPolicy>,
  pub security: SecurityPolicy,
  pub limits: LimitsPolicy,
  pub cache: CacheHookPolicy,
}

#[derive(Clone, Debug)]
pub struct RetryPolicy {
  pub enabled: bool,
  pub max_attempts: u32,              // 含首发；幂等才启用
  pub backoff: BackoffCfg,
  pub retry_on: RetryOn,              // 可配置：5xx/429/网络类/超时（按阶段）
  pub respect_retry_after: bool,      // 429/503 Retry-After
}
#[derive(Clone, Debug)]
pub struct BackoffCfg { pub base_ms:u64, pub factor:f64, pub jitter:f64, pub cap_ms:u64 }
#[derive(Clone, Debug)]
pub struct RetryOn { pub http_5xx:bool, pub http_429:bool, pub dns_err:bool, pub connect_err:bool, pub read_timeout:bool, pub ttfb_timeout:bool }

#[derive(Clone, Debug)]
pub struct CircuitBreakerPolicy {
  pub enabled: bool,
  pub window_ms: u64,                 // 滑窗
  pub failure_ratio: f32,             // e.g. ≥0.5
  pub min_samples: u32,               // 小样本避免误开
  pub consecutive_failures: u32,      // 或按连续失败开路
  pub cooldown_ms: u64,
  pub half_open_probes: u32,          // 探测数
  pub key_by: CbKey,                  // host|host+tenant
}
pub enum CbKey { Host, HostTenant }

#[derive(Clone, Debug)]
pub struct RedirectPolicy { pub enabled: bool, pub max: u8, pub allow_cross_origin: bool, pub keep_method_on_303: bool }

#[derive(Clone, Debug)]
pub struct TlsPolicy {
  pub min_tls: TlsVersion,            // TLS1_2+
  pub verify_cert: bool,              // 必开
  pub pinset_sha256: Option<Vec<String>>, // 证书公钥pin（base64）
  pub mtls: Option<MtlsCfg>,          // 可选 mTLS
}
pub enum TlsVersion { Tls12, Tls13 }
pub struct MtlsCfg { pub client_cert_pem:String, pub client_key_pem:String }

#[derive(Clone, Debug)]
pub struct DnsPolicy {
  pub timeout_ms: u64,
  pub cache_ttl_ms: u64,
  pub happy_eyeballs: bool,
  pub prefer_ipv6: bool,
}

#[derive(Clone, Debug)]
pub struct ProxyPolicy {
  pub http_proxy: Option<String>, pub https_proxy: Option<String>, pub no_proxy: Vec<String>, // CIDR/host 列表
}

#[derive(Clone, Debug)]
pub struct SecurityPolicy {
  pub allow_hosts: Option<Vec<String>>,  // host 白名单
  pub deny_private: bool,                // 默认 true
  pub max_redirects: u8,
  pub max_resp_bytes: u64,               // 防大响应炸弹
  pub max_req_bytes: u64,                // 上传限制
  pub strip_headers: Vec<String>,        // 发送前剥离的敏感头
}

#[derive(Clone, Debug)]
pub struct LimitsPolicy {
  pub per_host_concurrency: u32,
  pub per_tenant_concurrency: u32,
  pub per_host_rate_rps: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct CacheHookPolicy { pub enable_etag: bool, pub swr: bool }
```

**默认**：TLS1.2+、`deny_private=true`、`retry.max_attempts=3`（幂等）、断路器关、重定向≤5、`max_resp_bytes=32MB`、`per_host_concurrency=64`。

------

## 4. 客户端与拦截器（`client.rs`, `interceptors/*`）

### 4.1 NetClient

```rust
#[async_trait::async_trait]
pub trait NetClient: Send + Sync {
  async fn send(&self, req: NetRequest) -> Result<NetResponse, NetError>;

  // 便捷方法
  async fn get_json<T: DeserializeOwned>(&self, url:&str, policy_key:Option<&str>) -> Result<T, NetError>;
  async fn post_json<R: Serialize, T: DeserializeOwned>(&self, url:&str, body:&R, policy_key:Option<&str>) -> Result<T, NetError>;
  async fn get_bytes(&self, url:&str, policy_key:Option<&str>) -> Result<bytes::Bytes, NetError>;
}
```

- **Builder**：`ClientBuilder::new().with_policy_resolver(..).with_interceptors(vec![..]).build()`；
- **取消/超时**：调用侧可传 `timeout.total_ms` 或外层 `tokio::time::timeout`；内部对 connect/ttfb/read/write 分别设置。

### 4.2 Interceptor

```rust
#[async_trait::async_trait]
pub trait Interceptor: Send + Sync {
  async fn before(&self, req: &mut NetRequest) -> Result<(), NetError> { Ok(()) }
  async fn after(&self, req: &NetRequest, res: &mut NetResponse) -> Result<(), NetError> { Ok(()) }
  async fn on_error(&self, req: &NetRequest, err: &NetError) { /* 观测或补偿 */ }
}
```

**内置拦截器**

- `TraceUa`：追加 `X-Request-Id/Traceparent` 和统一 `User-Agent`;
- `Bearer`：从上游闭包获取 token；
- `A2A-JWS`（feature `crypto`）：对 `canonical(body)` 做 detached JWS，放到 `Authorization: A2A …`；
- `SandboxGuard`（feature `sandbox`）：检查 host 白名单与私网；
- `QosBytes`（feature `qos`）：统计 `bytes_in/out`；
- `CacheHook`（feature `cache`）：GET 响应该 ETag/SWR 策略与 SB-16 互动。

------

## 5. 重试与断路器（`runtime/retry.rs`, `runtime/cbreaker.rs`）

### 5.1 幂等重试逻辑

**决策维度**

- 仅当 `req.idempotent || method in {GET, HEAD, OPTIONS}`；POST 需显式标注幂等（且提供 Idempotency-Key）。
- **可重试错误**：DNS 失败、连接失败、`connect/ttfb/read` 超时、`HTTP 5xx/429`。
- `Retry-After`：若存在且 ≤ `cap_ms`，优先等待该时间。

**算法**

```
attempt = 1..=max_attempts
delay = base_ms * factor^(attempt-1)
jitter: delay *= (1 ± r), r ∈ [0, jitter]
delay = min(delay, cap_ms)
```

- 指数退避 + 抖动（full jitter或equal jitter均可，默认 full）。
- 在**断路器 Open** 时直接失败；HalfOpen 状态仅允许 `half_open_probes` 次探测。

### 5.2 断路器状态机

- **Closed**：滑窗统计失败率 ≥ `failure_ratio` 且样本 ≥ `min_samples` 或连续失败 ≥ `consecutive_failures` → **Open**；
- **Open**：拒绝请求，返回 `PROVIDER.UNAVAILABLE`；计时 `cooldown_ms`；
- **HalfOpen**：允许 N 次探测（`half_open_probes`），若探测成功率高于阈值 → **Closed**，否则回 **Open**。
- **维度**：按 `host` 或 `host+tenant`（`CbKey`）。

------

## 6. DNS / TLS / 安全过滤（`runtime/dns.rs`, `policy.tls`/`security`）

### 6.1 DNS

- 自定义超时 `dns.timeout_ms`；缓存 TTL `dns.cache_ttl_ms`；Happy-Eyeballs：并行 v6/v4，先返回的优先；
- 支持 `no_proxy` 与域名解析后的 **私网/环回**检查：
  - 私网 CIDR：`10/8, 172.16/12, 192.168/16, 169.254/16, 127/8`；IPv6 链路本地 `fe80::/10`；
  - 若 `security.deny_private=true` 且未通过 Sandbox 授权，**拒绝**。

### 6.2 TLS

- 默认 TLS1.2+；证书校验开启；可配置 pinset（SPKI 指纹 SHA-256）；
- 支持 mTLS（加载 client cert/key）；
- **错误映射**：验证失败→`AUTH.FORBIDDEN`；握手失败/版本不满足→`PROVIDER.UNAVAILABLE`。

### 6.3 重定向/限制

- 最大 `redirect.max`；跨域是否保留方法/Body；
- 响应体大小限制：若超过 `security.max_resp_bytes` → 中止并返回 `PROVIDER.UNAVAILABLE`（公共视图）。

------

## 7. 连接池与背压（`runtime/pool.rs`, `runtime/limiter.rs`）

- 连接池：keep-alive 超时、最大空闲连接、每 host MaxConns；
- 背压：
  - 每 host `Semaphore` 限并发（`limits.per_host_concurrency`）；
  - 可选每 tenant 并发；
  - 速率器（可选）：滑窗令牌桶限制每 host RPS；
- 超限 → 立刻 `RateLimited`（内部错误码）；对外公共视图仍为 `PROVIDER.UNAVAILABLE` 或自定义 `QUOTA.RATE_LIMITED`（若启 QoS）。

------

## 8. 观测与错误映射（`metrics.rs`, `errors.rs`）

### 8.1 指标

- `net_requests_total{tenant,host,method,scheme,outcome}`
- `net_latency_ms_bucket{phase=connect|ttfb|total}`
- `net_bytes{dir=in|out,tenant,host}`
- `net_retry_total{reason}`、`net_circuit_open_total{host}`、`net_redirect_total{host}`

### 8.2 错误→稳定码

| 场景                                   | 映射（对外公共视图）             |
| -------------------------------------- | -------------------------------- |
| DNS 失败、连接拒绝、握手失败、读写错误 | `PROVIDER.UNAVAILABLE`           |
| connect/ttfb/read 总体超时             | `…TIMEOUT`（内部仍归口公共视图） |
| TLS 验证失败 / pin 失败 / mTLS 失败    | `AUTH.FORBIDDEN`                 |
| 请求体/头超限，URL 非法                | `SCHEMA.VALIDATION_FAILED`       |
| 重试上限/断路器 Open                   | `PROVIDER.UNAVAILABLE`           |

> *注*：拦截器可进一步将部分错误映射为领域码（如 LLM.TIMEOUT）。

------

## 9. 与周边模块的接线

- **SB-03 配置**：`PolicyResolver` 从快照解析 per-tenant/per-host 策略（热更对新请求生效）。
- **SB-06 Sandbox**：出网前 `SandboxGuard` 检查**域名白名单/私网屏蔽/最大体积**；对 `net.http` 工具调用强制启用。
- **SB-07 LLM**：LLM Provider 客户端使用 `NetClient`；POST 幂等需带 `Idempotency-Key`；超时配置由模型级策略注入。
- **SB-15 A2A**：使用 `A2A-JWS` 拦截器对 body canonical 后做 detached 签名；对端用 SB-18 验签。
- **SB-16 Cache**：GET 响应如含 `ETag/Last-Modified`，`CacheHook` 记录 TTL，后续条件请求；SWR 由 SB-16 管理。
- **SB-14 QoS**：`QosBytes` 拦截器统计 `bytes_in/out`；可与 `reserve/settle` 联动。
- **SB-11 Observe**：请求/应答生成 trace span；指标按标签统一记录。

------

## 10. 测试与验收（契约/基准/混沌）

- **契约（SB-13）**
  - 超时：connect/ttfb/total 的触发与错误映射；
  - 重试：幂等 GET/HEAD 对 5xx/429/DNS/连接错误按策略重试；非幂等 POST 不重试；`Retry-After` 遵守；
  - 断路器：达到阈值后 Open；Half-Open 探测成功后关闭；
  - 安全：白名单与私网屏蔽 100% 命中；重定向上限与跨域策略；
  - 限制：最大响应体/请求体超限立即中止。
- **基准（SB-12）**
  - 并发 256：本地回环模拟服务，p95/吞吐；
  - 断路器开启前后**错误/重试降低 ≥ 50%**。
- **混沌**
  - 随机 DNS 污染/丢包/慢启动/5xx/429；
  - TLS 证书过期/自签名/中间证书缺失；
  - 代理异常与 no_proxy 白名单生效。

------

## 11. RIS 实现要点（预告）

- 基于 `reqwest` + `tokio`：
  - Builder 设置连接池/HTTP2/代理/TLS；
  - `SandboxGuard` 在 `before` 中做私网与白名单校验（解析 host→DNS→IP ranges 检查）；
  - `retry.rs` 包裹 `send_once()`，根据 `RetryPolicy` 与 `Retry-After` 控制重试与延迟；
  - `cbreaker.rs` 使用滑窗计数器（失败+样本），`AtomicU64/Mutex` 或 `dashmap` 存 per-host 状态；
  - `limiter.rs` 用 `Semaphore` 做 per-host 并发；
  - 指标：`metrics.rs` 通过 `observe` feature 打点；
  - 错误：统一 `NetError` → SB-02 公共视图；
  - 单测：本地 `hyper` mock server 注入延迟/5xx/429/重定向/大响应等场景。

------

### 小结

本技术设计将 `soulbase-net` 打造成**可治理的统一出网层**：

- 固化“**超时/重试/断路器/白名单/观测**”五要素；
- 以拦截器连接 `Auth/Sandbox/Cache/QoS/A2A`；
- 与 `SB-03` 的策略快照热更配合，确保新配置只影响新请求、不中断在途流量。

若确认无误，我将输出 **SB-19-RIS（最小可运行骨架）**：`reqwest` 客户端 + 幂等重试 + 断路器 + 私网过滤 + Trace/指标钩子 + 2–3 个端到端单测（重试/断路器/白名单）。
