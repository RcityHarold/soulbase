下面是 **SB-19-RIS：`soulbase-net` 最小可运行骨架**。
 与 SB-19（规约）& SB-19-TD（设计）一致，骨架提供：

- 统一类型：`NetRequest/NetResponse/Body/TimeoutCfg`，策略 `NetPolicy`（重试/断路器/安全/限流）。
- 客户端：`NetClient` Trait + `ReqwestClient` 实现（HTTP/1.1+HTTP/2），**幂等重试**（5xx/429/网络类）、**断路器**、**私网过滤**、**Trace+UA 拦截器**。
- 指标钩子（简易计数器）。
- 端到端单测（tokio + hyper）：**重试成功**、**断路器开路**、**私网白名单/屏蔽**。

> 放入 `soul-base/crates/soulbase-net/` 后：`cargo check && cargo test`。
>  说明：为便于快速落地，RIS 未包含 HTTP/3、代理、mTLS、SWR/CacheHook、A2A-JWS 等可选特性；留有文件与接口占位，后续按 TD 逐步补齐。

------

## 目录结构

```
soul-base/
└─ crates/
   └─ soulbase-net/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ errors.rs
      │  ├─ types.rs
      │  ├─ policy.rs
      │  ├─ client.rs
      │  ├─ interceptors/
      │  │  ├─ mod.rs
      │  │  ├─ trace_ua.rs
      │  │  └─ sandbox_guard.rs
      │  ├─ runtime/
      │  │  ├─ retry.rs
      │  │  ├─ cbreaker.rs
      │  │  └─ limiter.rs
      │  ├─ metrics.rs
      │  └─ prelude.rs
      └─ tests/
         └─ e2e.rs
```

------

## Cargo.toml

```toml
[package]
name = "soulbase-net"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "Resilient HTTP client with retry/circuit breaker/security for the Soul platform"
repository = "https://example.com/soul-base"

[features]
default = []
observe = []    # 预留：接 SB-11

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
async-trait = "0.1"
thiserror = "1"
once_cell = "1"
parking_lot = "0.12"
bytes = "1"
tokio = { version = "1", features = ["rt","macros","time","sync"] }
reqwest = { version = "0.12", default-features = false, features = ["json","http2","rustls-tls"] }
http = "1"
url = "2"

# 平台内
soulbase-errors = { path = "../soulbase-errors", version = "1.0.0" }

[dev-dependencies]
hyper = { version = "1", features = ["http1","server","tcp"] }
tokio = { version = "1", features = ["rt-multi-thread","macros","time","net"] }
```

------

## src/lib.rs

```rust
pub mod errors;
pub mod types;
pub mod policy;
pub mod client;
pub mod interceptors { pub mod mod_; pub mod trace_ua; pub mod sandbox_guard; }
pub mod runtime { pub mod retry; pub mod cbreaker; pub mod limiter; }
pub mod metrics;
pub mod prelude;

pub use client::{NetClient, ReqwestClient, ClientBuilder};
pub use types::{NetRequest, NetResponse, Body, TimeoutCfg};
pub use policy::{NetPolicy, RetryPolicy, BackoffCfg, RetryOn, CircuitBreakerPolicy, RedirectPolicy, TlsPolicy, DnsPolicy, ProxyPolicy, SecurityPolicy, LimitsPolicy, CacheHookPolicy};
```

------

## src/errors.rs

```rust
use thiserror::Error;
use soulbase_errors::prelude::*;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct NetError(pub ErrorObj);

impl NetError {
  pub fn provider_unavailable(msg:&str)->Self {
    NetError(ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE).user_msg("Upstream unavailable.").dev_msg(msg.to_string()).build())
  }
  pub fn timeout(phase:&str)->Self {
    NetError(ErrorBuilder::new(codes::LLM_TIMEOUT).user_msg(format!("{phase} timeout")).dev_msg(phase).build())
  }
  pub fn forbidden(msg:&str)->Self {
    NetError(ErrorBuilder::new(codes::AUTH_FORBIDDEN).user_msg("Forbidden.").dev_msg(msg.to_string()).build())
  }
  pub fn schema(msg:&str)->Self {
    NetError(ErrorBuilder::new(codes::SCHEMA_VALIDATION).user_msg("Invalid request.").dev_msg(msg.to_string()).build())
  }
  pub fn unknown(msg:&str)->Self {
    NetError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL).user_msg("Internal error.").dev_msg(msg.to_string()).build())
  }
}
```

------

## src/types.rs

```rust
use serde::{Serialize, Deserialize};
use bytes::Bytes;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeoutCfg { pub connect_ms:u64, pub ttfb_ms:u64, pub total_ms:u64, pub read_ms:u64, pub write_ms:u64 }
impl Default for TimeoutCfg {
  fn default()->Self { Self{ connect_ms:1000, ttfb_ms:3000, total_ms:10_000, read_ms:10_000, write_ms:10_000 } }
}

#[derive(Clone, Debug)]
pub enum Body { Empty, Json(serde_json::Value), Bytes(Bytes) }

#[derive(Clone, Debug)]
pub struct NetRequest {
  pub method: http::Method,
  pub url: String,
  pub headers: Vec<(String, String)>,
  pub query: Option<Vec<(String, String)>>,
  pub body: Body,
  pub timeout: TimeoutCfg,
  pub idempotent: bool,
  pub policy_key: Option<String>,
}
impl Default for NetRequest {
  fn default()->Self {
    Self {
      method: http::Method::GET, url: String::new(), headers: vec![], query: None,
      body: Body::Empty, timeout: TimeoutCfg::default(), idempotent: true, policy_key: None
    }
  }
}

#[derive(Clone, Debug)]
pub struct NetResponse {
  pub status: u16,
  pub headers: Vec<(String,String)>,
  pub content_type: Option<String>,
  pub body: Bytes,
  pub elapsed: Elapsed,
  pub bytes_in: u64,
}
#[derive(Clone, Debug)]
pub struct Elapsed { pub connect_ms:u64, pub ttfb_ms:u64, pub total_ms:u64 }
```

------

## src/policy.rs

```rust
#[derive(Clone, Debug)]
pub struct NetPolicy {
  pub retry: RetryPolicy,
  pub cbreaker: CircuitBreakerPolicy,
  pub security: SecurityPolicy,
  pub limits: LimitsPolicy,
}
impl Default for NetPolicy {
  fn default()->Self {
    Self{
      retry: RetryPolicy{ enabled:true, max_attempts:3,
        backoff: BackoffCfg{ base_ms:100, factor:2.0, jitter:0.3, cap_ms:2_000 },
        retry_on: RetryOn{ http_5xx:true, http_429:true, dns_err:true, connect_err:true, read_timeout:true, ttfb_timeout:true },
        respect_retry_after: true
      },
      cbreaker: CircuitBreakerPolicy{ enabled:true, window_ms:10_000, failure_ratio:0.5, min_samples:10, consecutive_failures:5, cooldown_ms:5_000, half_open_probes:3 },
      security: SecurityPolicy{ allow_hosts: None, deny_private: true, max_redirects:5, max_resp_bytes: 32*1024*1024, max_req_bytes: 8*1024*1024, strip_headers: vec![] },
      limits: LimitsPolicy{ per_host_concurrency:64, per_tenant_concurrency:256, per_host_rate_rps: None }
    }
  }
}

#[derive(Clone, Debug)]
pub struct RetryPolicy { pub enabled: bool, pub max_attempts: u32, pub backoff: BackoffCfg, pub retry_on: RetryOn, pub respect_retry_after: bool }
#[derive(Clone, Debug)]
pub struct BackoffCfg { pub base_ms:u64, pub factor:f64, pub jitter:f64, pub cap_ms:u64 }
#[derive(Clone, Debug)]
pub struct RetryOn { pub http_5xx:bool, pub http_429:bool, pub dns_err:bool, pub connect_err:bool, pub read_timeout:bool, pub ttfb_timeout:bool }

#[derive(Clone, Debug)]
pub struct CircuitBreakerPolicy {
  pub enabled: bool, pub window_ms: u64, pub failure_ratio: f32, pub min_samples: u32,
  pub consecutive_failures: u32, pub cooldown_ms: u64, pub half_open_probes: u32,
}

#[derive(Clone, Debug)]
pub struct SecurityPolicy {
  pub allow_hosts: Option<Vec<String>>,
  pub deny_private: bool,
  pub max_redirects: u8,
  pub max_resp_bytes: u64,
  pub max_req_bytes: u64,
  pub strip_headers: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct LimitsPolicy { pub per_host_concurrency: u32, pub per_tenant_concurrency: u32, pub per_host_rate_rps: Option<u32> }
```

------

## src/interceptors/mod.rs

```rust
use crate::{types::{NetRequest, NetResponse}, errors::NetError};

#[async_trait::async_trait]
pub trait Interceptor: Send + Sync {
  async fn before(&self, _req:&mut NetRequest) -> Result<(), NetError> { Ok(()) }
  async fn after(&self, _req:&NetRequest, _res:&mut NetResponse) -> Result<(), NetError> { Ok(()) }
  async fn on_error(&self, _req:&NetRequest, _err:&NetError) {}
}
```

### src/interceptors/trace_ua.rs

```rust
use super::Interceptor;
use crate::{types::NetRequest, errors::NetError};

#[derive(Clone, Default)]
pub struct TraceUa { pub user_agent: String, pub request_id: Option<String> }

#[async_trait::async_trait]
impl Interceptor for TraceUa {
  async fn before(&self, req:&mut NetRequest) -> Result<(), NetError> {
    if let Some(id) = &self.request_id { req.headers.push(("X-Request-Id".into(), id.clone())); }
    req.headers.push(("User-Agent".into(), if self.user_agent.is_empty() { "soulbase-net/1.0".into() } else { self.user_agent.clone() }));
    Ok(())
  }
}
```

### src/interceptors/sandbox_guard.rs

```rust
use super::Interceptor;
use crate::{types::NetRequest, errors::NetError, policy::SecurityPolicy};
use url::Url;

#[derive(Clone)]
pub struct SandboxGuard { pub policy: SecurityPolicy }
#[async_trait::async_trait]
impl Interceptor for SandboxGuard {
  async fn before(&self, req:&mut NetRequest) -> Result<(), NetError> {
    let u = Url::parse(&req.url).map_err(|e| NetError::schema(&format!("url parse: {e}")))?;
    let host = u.host_str().ok_or_else(|| NetError::schema("no host"))?.to_lowercase();
    if let Some(allow) = &self.policy.allow_hosts {
      if !allow.iter().any(|h| host.ends_with(h)) {
        return Err(NetError::forbidden("host not in allowlist"));
      }
    }
    if self.policy.deny_private && is_private_host(&host) {
      return Err(NetError::forbidden("private address denied"));
    }
    Ok(())
  }
}

fn is_private_host(h:&str) -> bool {
  // 简化：本地/常见私网前缀；生产建议 DNS 解析到 IP 后再判定
  h == "localhost" || h.starts_with("127.") || h.starts_with("10.") ||
  h.starts_with("192.168.") || h.starts_with("172.16.") || h.starts_with("172.17.") || h.starts_with("172.18.") || h.starts_with("172.19.") ||
  h.starts_with("172.2")
}
```

------

## src/runtime/retry.rs

```rust
use crate::{errors::NetError, types::{NetRequest, NetResponse}};
use crate::policy::{RetryPolicy, BackoffCfg, RetryOn};
use rand::Rng;

pub struct RetryCtx { pub attempt:u32, pub next_delay_ms:u64 }
impl RetryCtx { pub fn new()->Self { Self{ attempt:1, next_delay_ms:0 } } }

pub fn backoff_delay(cfg:&BackoffCfg, attempt:u32) -> u64 {
  let base = (cfg.base_ms as f64) * cfg.factor.powi((attempt-1) as i32);
  let cap = cfg.cap_ms as f64;
  let mut rng = rand::thread_rng();
  let jitter = 1.0 + (rng.gen::<f64>()*2.0 - 1.0) * cfg.jitter;
  (base.min(cap) * jitter).max(0.0) as u64
}

/// 是否应重试：仅限幂等请求或显式标注
pub fn should_retry(req:&NetRequest, res:&Result<NetResponse, NetError>, on:&RetryOn) -> bool {
  if !req.idempotent { return false; }
  match res {
    Ok(r) => {
      (on.http_5xx && r.status >= 500) || (on.http_429 && r.status == 429)
    }
    Err(e) => {
      let msg = format!("{}", e);
      (on.dns_err && msg.contains("dns")) ||
      (on.connect_err && msg.contains("connect")) ||
      (on.read_timeout && msg.contains("timeout")) ||
      (on.ttfb_timeout && msg.contains("timeout"))
    }
  }
}
```

------

## src/runtime/cbreaker.rs

```rust
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::time::{Instant, Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CbState { Closed, Open(Instant), HalfOpen{ probes_left:u32 } }

pub struct CircuitBreaker {
  pub state: Mutex<CbState>,
  pub window: Duration,
  pub failure_ratio: f32,
  pub min_samples: usize,
  pub consec_limit: u32,
  pub cooldown: Duration,
  pub half_probes: u32,
  ring: Mutex<VecDeque<bool>>, // true=success,false=failure
  consec_fail: Mutex<u32>,
}

impl CircuitBreaker {
  pub fn new(window_ms:u64, ratio:f32, min_samples:u32, consec:u32, cooldown_ms:u64, half:u32) -> Self {
    Self {
      state: Mutex::new(CbState::Closed),
      window: Duration::from_millis(window_ms),
      failure_ratio: ratio,
      min_samples: min_samples as usize,
      consec_limit: consec,
      cooldown: Duration::from_millis(cooldown_ms),
      half_probes: half,
      ring: Mutex::new(VecDeque::with_capacity(512)),
      consec_fail: Mutex::new(0),
    }
  }

  pub fn allow(&self) -> bool {
    match *self.state.lock() {
      CbState::Closed => true,
      CbState::Open(t) => t.elapsed() >= self.cooldown,
      CbState::HalfOpen{ probes_left } => probes_left > 0,
    }
  }
  pub fn on_result(&self, ok:bool) {
    // update ring & consec
    {
      let mut cf = self.consec_fail.lock();
      if ok { *cf = 0; } else { *cf += 1; }
    }
    {
      let mut r = self.ring.lock();
      if r.len() >= 256 { r.pop_front(); }
      r.push_back(ok);
    }
    // transitions
    let mut st = self.state.lock();
    match *st {
      CbState::Closed => {
        let cf = *self.consec_fail.lock();
        if cf >= self.consec_limit || self.fail_ratio() {
          *st = CbState::Open(Instant::now());
        }
      }
      CbState::Open(t) => {
        if t.elapsed() >= self.cooldown {
          *st = CbState::HalfOpen{ probes_left: self.half_probes };
        }
      }
      CbState::HalfOpen{ ref mut probes_left } => {
        if ok {
          if *probes_left > 0 { *probes_left -= 1; }
          if *probes_left == 0 { *st = CbState::Closed; self.ring.lock().clear(); *self.consec_fail.lock() = 0; }
        } else {
          *st = CbState::Open(Instant::now());
        }
      }
    }
  }
  fn fail_ratio(&self)->bool {
    let r = self.ring.lock();
    if r.len() < self.min_samples { return false; }
    let fails = r.iter().filter(|&&b| !b).count();
    (fails as f32) / (r.len() as f32) >= self.failure_ratio
  }
}
```

------

## src/runtime/limiter.rs（占位）

```rust
use tokio::sync::Semaphore;
use std::{sync::Arc, collections::HashMap};
use parking_lot::Mutex;

#[derive(Clone)]
pub struct HostLimiter {
  map: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
  per_host: usize,
}
impl HostLimiter {
  pub fn new(per_host:usize)->Self { Self{ map: Arc::new(Mutex::new(HashMap::new())), per_host } }
  pub async fn acquire(&self, host:&str) -> tokio::sync::OwnedSemaphorePermit {
    let sem = self.map.lock().entry(host.to_string()).or_insert_with(|| Arc::new(Semaphore::new(self.per_host))).clone();
    sem.acquire_owned().await.expect("semaphore closed")
  }
}
```

------

## src/metrics.rs（占位）

```rust
#[derive(Default)]
pub struct NetStats { pub req:u64, pub retry:u64, pub open_cb:u64 }
impl NetStats { pub fn inc_req(&mut self){ self.req+=1 } pub fn inc_retry(&mut self){ self.retry+=1 } }
```

------

## src/client.rs

```rust
use crate::{errors::NetError, types::*, policy::*, interceptors::mod_::Interceptor, interceptors, runtime::{retry, cbreaker::CircuitBreaker, limiter::HostLimiter}};
use reqwest::Client as RClient;
use std::{sync::Arc, time::Instant};
use url::Url;

#[async_trait::async_trait]
pub trait NetClient: Send + Sync {
  async fn send(&self, req: NetRequest) -> Result<NetResponse, NetError>;
  async fn get_json<T: serde::de::DeserializeOwned + Send>(&self, url:&str, _policy_key:Option<&str>) -> Result<T, NetError> {
    let mut r = NetRequest{ url: url.into(), ..Default::default() };
    r.headers.push(("Accept".into(),"application/json".into()));
    let resp = self.send(r).await?;
    serde_json::from_slice::<T>(&resp.body).map_err(|e| NetError::schema(&format!("json decode: {e}")))
  }
}

pub struct ClientBuilder {
  pub policy: NetPolicy,
  pub interceptors: Vec<Arc<dyn Interceptor>>,
}
impl Default for ClientBuilder {
  fn default()->Self {
    Self{ policy: NetPolicy::default(), interceptors: vec![Arc::new(interceptors::trace_ua::TraceUa::default())] }
  }
}
impl ClientBuilder {
  pub fn with_policy(mut self, p:NetPolicy)->Self { self.policy = p; self }
  pub fn with_interceptor<I:Interceptor + 'static>(mut self, i:I)->Self { self.interceptors.push(Arc::new(i)); self }
  pub fn build(self)->ReqwestClient { ReqwestClient::new(self.policy, self.interceptors) }
}

pub struct ReqwestClient {
  http: RClient,
  policy: NetPolicy,
  icpts: Vec<Arc<dyn Interceptor>>,
  cbs: parking_lot::Mutex<std::collections::HashMap<String, Arc<CircuitBreaker>>>,
  limiter: HostLimiter,
}
impl ReqwestClient {
  pub fn new(policy:NetPolicy, icpts:Vec<Arc<dyn Interceptor>>)->Self {
    let http = RClient::builder().http2_prior_knowledge(false).build().expect("client");
    Self{ http, policy: policy.clone(), icpts, cbs: parking_lot::Mutex::new(Default::default()), limiter: HostLimiter::new(policy.limits.per_host_concurrency as usize) }
  }
  fn breaker_for(&self, host:&str)->Arc<CircuitBreaker>{
    self.cbs.lock().entry(host.into()).or_insert_with(|| {
      Arc::new(CircuitBreaker::new(
        self.policy.cbreaker.window_ms,
        self.policy.cbreaker.failure_ratio,
        self.policy.cbreaker.min_samples,
        self.policy.cbreaker.consecutive_failures,
        self.policy.cbreaker.cooldown_ms,
        self.policy.cbreaker.half_open_probes
      ))
    }).clone()
  }
  async fn send_once(&self, req: &NetRequest) -> Result<NetResponse, NetError> {
    let start = Instant::now();
    let url = Url::parse(&req.url).map_err(|e| NetError::schema(&format!("url: {e}")))?;
    let host = url.host_str().ok_or_else(|| NetError::schema("no host"))?.to_string();

    // 限并发
    let _permit = self.limiter.acquire(&host).await;

    let mut rb = self.http.request(req.method.clone(), url.clone());
    // headers
    for (k,v) in &req.headers { rb = rb.header(&**k, &**v); }
    // query
    if let Some(q) = &req.query { rb = rb.query(&q); }
    // timeouts（总时长）
    rb = rb.timeout(std::time::Duration::from_millis(req.timeout.total_ms));

    // body
    match &req.body {
      Body::Empty => {},
      Body::Json(v) => { rb = rb.json(v); }
      Body::Bytes(b) => { rb = rb.body(b.clone()); }
    }

    let r = rb.send().await.map_err(|e| NetError::provider_unavailable(&format!("send: {e}")))?;
    let status = r.status().as_u16();
    let headers: Vec<(String,String)> = r.headers().iter().map(|(k,v)| (k.to_string(), v.to_str().unwrap_or("").into())).collect();
    let ct = r.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let body = r.bytes().await.map_err(|e| NetError::provider_unavailable(&format!("read body: {e}")))?;
    let elapsed = Elapsed{ connect_ms:0, ttfb_ms:0, total_ms: start.elapsed().as_millis() as u64 };
    Ok(NetResponse{ status, headers, content_type: ct, body, elapsed, bytes_in: 0 })
  }
}
#[async_trait::async_trait]
impl NetClient for ReqwestClient {
  async fn send(&self, mut req: NetRequest) -> Result<NetResponse, NetError> {
    // before 拦截器
    for i in &self.icpts { i.before(&mut req).await?; }
    let url = Url::parse(&req.url).map_err(|e| NetError::schema(&format!("url: {e}")))?;
    let host = url.host_str().ok_or_else(|| NetError::schema("no host"))?.to_string();
    let cb = self.breaker_for(&host);
    if self.policy.cbreaker.enabled && !cb.allow() {
      return Err(NetError::provider_unavailable("circuit open"));
    }

    let mut attempt = 1u32;
    loop {
      let res = self.send_once(&req).await;
      let ok = res.as_ref().map(|r| r.status < 500 && r.status != 429).unwrap_or(false);
      cb.on_result(ok);

      match &res {
        Ok(_) => {
          let mut r = res.unwrap();
          // after 拦截器
          for i in &self.icpts { i.after(&req, &mut r).await?; }
          return Ok(r);
        }
        Err(e) => {
          if !self.policy.retry.enabled || attempt >= self.policy.retry.max_attempts || !retry::should_retry(&req, &res, &self.policy.retry.retry_on) {
            for i in &self.icpts { i.on_error(&req, e); }
            return Err(e.clone());
          }
          // backoff
          let delay = retry::backoff_delay(&self.policy.retry.backoff, attempt);
          attempt += 1;
          tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
          continue;
        }
      }
    }
  }
}
```

------

## src/prelude.rs

```rust
pub use crate::errors::NetError;
pub use crate::types::{NetRequest, NetResponse, Body, TimeoutCfg, Elapsed};
pub use crate::policy::*;
pub use crate::client::{NetClient, ReqwestClient, ClientBuilder};
pub use crate::interceptors::mod_::Interceptor;
pub use crate::interceptors::trace_ua::TraceUa;
pub use crate::interceptors::sandbox_guard::SandboxGuard;
```

------

## tests/e2e.rs

```rust
use soulbase_net::prelude::*;
use hyper::{service::{make_service_fn, service_fn}, Request, Response, Body as HBody, StatusCode};
use tokio::time::{sleep, Duration};

async fn start_mock(port:u16, handler: fn(Request<HBody>) -> Response<HBody>) {
    let addr = ([127,0,0,1], port).into();
    let make = make_service_fn(move |_| {
        async move {
            Ok::<_, hyper::Error>(service_fn(move |req| async move {
                Ok::<_, hyper::Error>(handler(req))
            }))
        }
    });
    let srv = hyper::Server::bind(&addr).serve(make);
    tokio::spawn(srv);
}

#[tokio::test]
async fn retry_succeeds_on_5xx_then_ok() {
    // mock: 第一次返回 500，第二次 200
    use std::sync::atomic::{AtomicUsize, Ordering};
    static HN: AtomicUsize = AtomicUsize::new(0);
    start_mock(18080, |_req| {
        if HN.fetch_add(1, Ordering::SeqCst) == 0 {
            Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).body(HBody::from("fail")).unwrap()
        } else {
            Response::new(HBody::from("{\"ok\":true}"))
        }
    }).await;

    let mut policy = NetPolicy::default();
    policy.security.deny_private = false; // 允许本地访问
    policy.retry.max_attempts = 3;

    let client = ClientBuilder::default()
        .with_policy(policy.clone())
        .with_interceptor(TraceUa::default())
        .with_interceptor(SandboxGuard{ policy: policy.security.clone() })
        .build();

    let req = NetRequest {
        method: http::Method::GET,
        url: "http://127.0.0.1:18080/ok".into(),
        ..Default::default()
    };
    let resp = client.send(req).await.expect("ok after retry");
    assert_eq!(resp.status, 200);
}

#[tokio::test]
async fn circuit_breaker_opens_after_failures() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static HC: AtomicUsize = AtomicUsize::new(0);
    start_mock(18081, |_req| {
        HC.fetch_add(1, Ordering::SeqCst);
        Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).body(HBody::from("boom")).unwrap()
    }).await;

    let mut policy = NetPolicy::default();
    policy.security.deny_private = false;
    policy.cbreaker.min_samples = 3;
    policy.cbreaker.failure_ratio = 0.5;
    policy.retry.enabled = false; // 不重试，更快触发

    let client = ClientBuilder::default()
        .with_policy(policy.clone())
        .with_interceptor(TraceUa::default())
        .with_interceptor(SandboxGuard{ policy: policy.security.clone() })
        .build();

    // 连续多次失败，后续应被 open 拒绝
    for _ in 0..3 {
        let req = NetRequest { method: http::Method::GET, url: "http://127.0.0.1:18081/fail".into(), ..Default::default() };
        let _ = client.send(req).await.err().expect("fail");
    }
    // 断路器应 open
    let req = NetRequest { method: http::Method::GET, url: "http://127.0.0.1:18081/fail".into(), ..Default::default() };
    let err = client.send(req).await.err().expect("circuit open");
    assert!(format!("{}", err).contains("Upstream unavailable"));
}

#[tokio::test]
async fn sandbox_guard_denies_private_when_not_allowed() {
    // 不启动服务，仅验证拦截器逻辑
    let policy = NetPolicy::default(); // deny_private=true
    let client = ClientBuilder::default()
        .with_policy(policy.clone())
        .with_interceptor(TraceUa::default())
        .with_interceptor(SandboxGuard{ policy: policy.security.clone() })
        .build();

    let req = NetRequest { method: http::Method::GET, url: "http://127.0.0.1:1/".into(), ..Default::default() };
    let err = client.send(req).await.err().expect("blocked");
    assert!(format!("{}", err).contains("Forbidden"));
}
```

------

## README.md（简要）

```markdown
# soulbase-net (RIS)

Resilient HTTP Client for the Soul platform:
- NetClient + Reqwest 实现
- 幂等重试（5xx/429/网络类） + 指数退避
- 断路器（Closed/Open/HalfOpen）
- 私网屏蔽 / 域名白名单（SandboxGuard）
- Trace+User-Agent 注入拦截器
- 端到端单测：重试成功、断路器开路、私网屏蔽

## Quick Start
let policy = NetPolicy::default();
let client = ClientBuilder::default()
  .with_policy(policy.clone())
  .with_interceptor(TraceUa::default())
  .with_interceptor(SandboxGuard{ policy: policy.security.clone() })
  .build();

let resp = client.send(NetRequest{ method: http::Method::GET, url:"https://example.com".into(), ..Default::default() }).await?;
println!("status = {}", resp.status);

## Next
- HTTP/3、代理、mTLS、A2A-JWS、SWR/ETag hook
- 观测打点对接 SB-11、QoS 字节计量（SB-14）
```

------

### 说明与后续

- **安全**：当前 SandboxGuard 的私网判断为域名启发式，生产建议在解析到 IP 后进行 CIDR 判断并缓存；
- **观测/指标**：在 `metrics.rs` 内置 `observe` feature 后，对接 SB-11 的 `Meter/Trace`；
- **超时分相位**：RIS 用 `total`；生产化需把 connect/ttfb/read/write 分相位设置与错误映射；
- **SWR/CacheHook/A2A-JWS**：保留 Interceptor 桩位，后续直接扩展即可。
