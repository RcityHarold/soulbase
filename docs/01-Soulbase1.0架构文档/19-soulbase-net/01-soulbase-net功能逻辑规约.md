### **文档 SB-19：soulbase-net（统一韧性 HTTP 客户端 / Resilient HTTP Client）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：为 Soul 生态提供一个**安全、可观测、可治理**的**统一出网客户端**，在**不改变业务语义**前提下提供：
  1. **标准化请求构建与发送**（JSON/Bytes/Stream/Upload/Download/Range）；
  2. **韧性策略**（连接/首包/总体超时，幂等重试，指数退避+抖动，遵循 `Retry-After`，**断路器** Half-Open 探测，限并发/速率）；
  3. **安全策略**（TLS 最低版本、证书校验/可选 pinning、mTLS、域名/路径白名单、私网与环回/IPv6 过滤、最大重定向深度、Header 白名单）；
  4. **治理与观测**（统一 `User-Agent`、Trace/Envelope 透传、可观测指标、错误稳定码映射、QoS 字节计量、SWR/Cache 钩子）；
  5. **可扩展拦截器**（认证 Bearer、A2A JWS 签名、Sandbox 出网校验）。
- **范围**：
  - 抽象：`NetClient/NetRequest/NetResponse/NetPolicy` 与 `Interceptor` 链；
  - 传输：HTTP/1.1、HTTP/2（默认），HTTP/3 可选；
  - 解析：JSON 严格模式、Bytes、Stream。
- **非目标**：不取代网关（Soul-Hub）；不提供浏览器渲染；不做长连接 WebSocket（可后续扩展）。

------

#### **1. 功能定位（Functional Positioning）**

- **出网唯一门面**：LLM Provider 调用、Tools 的 `net.http` 只读、A2A HTTP 传输、内部服务出网均通过 `soulbase-net`。
- **策略收口**：把**超时/重试/断路器/重定向/代理/域名白名单**从业务中抽离，集中实施并可热更。
- **观测与成本**：把**字节/请求/尾延**统一暴露给 `SB-11 observe` 和 `SB-14 qos`。

------

#### **2. 系统角色与地位（System Role & Position）**

- 层级：**基石层 / Soul-Base**（横切出网调用）。
- 关系：
  - **SB-05 Interceptors**：透传 `X-Request-Id/Trace/EnvelopeId`、统一错误公共视图；
  - **SB-06 Sandbox**：对 `net.http` 出口实施**域名白名单/私网屏蔽/带宽限制**（通过 `Interceptor`）；
  - **SB-07 LLM**：Provider HTTP 调用复用 `NetClient` 的韧性/认证；
  - **SB-15 A2A**：HTTP 传输层 `JWS` 签名/验签拦截器；
  - **SB-16 Cache**：GET 的 SWR/条件请求（ETag/If-None-Match）钩子；
  - **SB-11/14**：指标与字节计量对账；
  - **SB-03**：`NetPolicy` 与代理/DNS/证书策略按快照热更。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

- **NetRequest**：`{method,url,headers,query,body(json|bytes|stream),timeout{connect,ttfb,total},retry,cbreaker,redirect,proxy}`
- **NetResponse**：`{status,headers,body(bytes|stream),content_type,elapsed{connect,ttfb,total},bytes_in,outcome}`
- **NetPolicy**：
  - **超时**：`connect_ms/ttfb_ms/total_ms/read_ms/write_ms`；
  - **重试**：只对**幂等**（GET/HEAD/PUT?/DELETE/OPTIONS）和明确标注幂等的 POST；`backoff{base,factor,jitter,cap}`；遵守 `Retry-After`；
  - **断路器**：`open_on{ratio|consecutive}`、`half_open{probe}`、`cooldown_ms`；按 host/port/tenant 维度；
  - **重定向**：开启/最大次数/是否跨域保留方法与 Body；
  - **安全**：TLS 最低版本、证书校验/可选 pinning、mTLS；**域名白名单**、私网/环回/链路本地/IPv6 策略、最大响应体/最大上传体；
  - **限流**：每 tenant/每 host 并发/速率上限；
  - **缓存钩子**：ETag/Last-Modified 条件请求、SWR 模式开关。
- **Interceptor**：`before(req)->req'`、`after(res)->res'`；内置 `Trace/UA/Bearer/A2A-JWS/Sandbox-Guard/CacheHook/QoS-Bytes`。

------

#### **4. 不变式（Invariants）**

1. **安全默认**：TLS 开启、证书校验必开；HTTP 明文默认拒绝（除显式允许内网）；
2. **最小披露**：请求/响应日志只记录**公共视图与摘要**，不落 Body 原文；
3. **幂等重试**：仅在**幂等语义成立**时重试；POST 重试必须显式标注为幂等并附幂等键；
4. **私网屏蔽**：默认禁止访问 `127.0.0.0/8`、`10.0.0.0/8`、`172.16.0.0/12`、`192.168.0.0/16`、`169.254.0.0/16` 等（由 Sandbox 决策可覆写）；
5. **统一错误码**：连接/解析/证书/超时/5xx 均映射为 `PROVIDER.UNAVAILABLE` 或 `…TIMEOUT`，对外只暴露公共视图；
6. **可观测**：每次请求产出 `net_*` 指标与 trace span；
7. **热更安全**：`NetPolicy` 热更对**新请求**生效，不影响进行中的请求。

------

#### **5. 能力与接口（Abilities & Abstract Interfaces）**

> 具体 Trait/方法在 TD/RIS 落地，这里定义能力与行为。

- **请求构建与发送**
  - `client.send(NetRequest) -> NetResponse`（通用）；
  - 便捷：`get_json<T> / post_json<R> / get_bytes / download_stream / upload_stream`；
  - JSON 严格：拒绝非 UTF-8/NaN/Inf；
  - 支持 `Range`、`If-None-Match/If-Modified-Since`、`gzip/br` 解压。
- **策略注入**
  - `with_policy(NetPolicy)` / `policy_resolver(tenant,host)->NetPolicy`（来自 `SB-03`）；
  - 支持 per-tenant/per-host 的覆写。
- **拦截器体系**
  - 顺序执行：`Trace/UA→SandboxGuard→Auth(Bearer/JWS)→QoS-Bytes(budget check)→Send→QoS-Bytes(record)→CacheHook/SWR`；
  - 自定义扩展：LLM Provider 专用 Header/签名；A2A JWS 签名。
- **连接栈**
  - HTTP/1.1 & HTTP/2（默认）；HTTP/3 可选；连接池/Keep-Alive 配置；
  - DNS：解析超时/缓存 TTL/Happy Eyeballs（v4/v6），可热更。
- **下载/上传**
  - 大文件下载为 `Stream`；上传串流；最大体积限制（策略）。
- **代理/企业网络**
  - 支持 HTTP/HTTPS 代理与 no_proxy 列表；
- **错误映射**
  - `DNS/Connect/Handshake/Write/Read` → `PROVIDER.UNAVAILABLE`；
  - `Connect/TTFB/Total Timeout` → 对应 `…TIMEOUT` 稳定码（最终对外为公共视图）；
  - `TLS` 失败 → `AUTH.FORBIDDEN` 或 `PROVIDER.UNAVAILABLE`（按策略）。

------

#### **6. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **指标**（SB-11 对齐）：
  - `net_requests_total{tenant,host,method,scheme,outcome}`
  - `net_latency_ms_bucket{phase=connect|ttfb|total}`
  - `net_bytes{dir=in|out,tenant,host}`
  - `net_retry_total{reason}`、`net_circuit_open_total{host}`、`net_redirect_total{host}`
- **SLO**：
  - 小对象（JSON/≤32KB）p95：同机房 **≤ 80ms**；跨区视网络；
  - 断路器生效后失败风暴显著收敛（重试/失败比下降**≥ 50%**）；
  - 私网/白名单拦截命中率 100%。
- **验收**：
  - 契约：超时/重试/断路器/重定向/白名单/私网过滤/大小限制；
  - 基准：并发 256 下 tail 延迟稳定；
  - 混沌：DNS/证书/网络抖动/5xx/429/代理异常，客户端自动降级/退避。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：SB-06/07/08/15 等业务调用方；SB-03（策略）；
- **下游**：系统 DNS、TLS、代理；
- **边界**：不做 WebSocket/长轮询（可后续扩展）；不替代应用级重试控制（仅实施安全的幂等重试）。

------

#### **8. 风险与控制（Risks & Controls）**

- **重试放大** → **幂等守则** + `Retry-After` + **指数退避 + 抖动**，并结合断路器；
- **SSRF/私网穿透** → 默认**私网屏蔽** + **域名白名单**（Sandbox 可细化）；
- **证书与 TLS 降级** → 最低版本 + pinning 可选 + mTLS 支持；
- **尾延波动** → 连接池/Happy Eyeballs + 按主机断路器 + QoS 限速；
- **缓存不一致** → Cache Hook 只在 GET + 明确开启下使用，配合 ETag/SWR，避免缓存写入；
- **日志泄露** → 严格公共视图：不记录敏感 Header/Body；大响应只记长度与 hash 摘要。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 幂等 GET + SWR + Cache**

1. 调用方 `get_json` → `Trace/UA` 注入 → `SandboxGuard` 检查域名 → 发送；
2. 响应含 `ETag` → 缓存并记录 TTL；下次发 `If-None-Match`；
3. 缓存过期触发 SWR：**先返回旧值**，后台刷新回填。

**9.2 LLM Provider POST（幂等键）**

1. 构造 `NetRequest`（POST + idempotency key）；
2. 超时：`connect_ms=1000, ttfb_ms=3000, total_ms=10000`；
3. 发生 429/5xx → 看 `Retry-After` 重试；无则指数退避（最多 2 次）。

**9.3 A2A HTTP 传输**

1. `A2A-JWS` 拦截器对 `canonical(body)` 进行 detached 签名，写入 `Authorization: A2A …` 或 `JWS` 头；
2. 对端用 `soulbase-crypto` 验签；
3. 错误映射统一 `A2A.SIGNATURE_INVALID/PROVIDER.UNAVAILABLE`。

------

#### **10. 开放问题（Open Issues / TODO）**

- **HTTP/3** 引入与回退策略；
- **请求去重**（SingleFlight for GET）是否下沉到 `soulbase-net` 还是留给 `soulbase-cache`；
- **智能路由**：跨 Provider/Endpoint 的熔断/权重路由（后续与 LLM 路由器结合）；
- **流量镜像**（Shadow）对接 Benchmark；
- **端到端 mTLS**：对内服务调用是否默认强制 mTLS。

------

> 若你认可本规约，下一步我将输出 **SB-19-TD（技术设计）**：给出 `NetClient/NetRequest/NetResponse/NetPolicy/Interceptor` 的 Trait 与字段、重试/断路器/私网过滤算法细节、与 `SB-11/14/06/07/15/16/03` 的具体接线点；随后给出 **SB-19-RIS**（基于 `reqwest` 的最小实现 + 断路器 + 幂等重试 + 指标打点 + 单测）。
