# 文档 SB-11-TD：`soulbase-observe` 技术设计（SDK/SPI · 采样/屏蔽/留存 · 导出适配 · 指标族落地）

> 对应规约：SB-11（统一观测 / Logs · Metrics · Traces · Evidence Bus）
>  目标：给出**可落地**的 SDK/SPI、采样/屏蔽/留存策略、导出适配器（Prometheus/OTLP/Logs-HTTP/Kafka…）与“标准指标族”的实现细节，保持与 `sb-types / -errors / -interceptors / -auth / -llm / -tools / -sandbox / -storage / -tx / -qos / -config` 的不变式一致。

------

## 1. Crate 结构与 Feature 规划

```
soulbase-observe/
  src/
    lib.rs
    errors.rs
    labels.rs            # 标签白名单/别名 + 校验
    model.rs             # LogEvent / MetricSpec / SpanCtx / EvidenceEnvelope<T>
    ctx.rs               # ObserveCtx（tenant/trace/envelope/config戳记）
    sdk/
      log.rs             # Logger / Builder / redaction
      metrics.rs         # Meter / Counter / Gauge / Histogram + 宏
      trace.rs           # Tracer SPI（与 OTEL 互操作）
      evidence.rs        # EvidenceSink：Envelope<EvidenceEvent> 统一出口
    pipeline/
      sampler.rs         # Head/Tail Sampler 组合
      redactor.rs        # 屏蔽/脱敏策略
      retention.rs       # 留存策略（TTL/分级）
      router.rs          # 批处理/队列/回压
    export/
      mod.rs
      prometheus.rs      # 拉取式导出（/metrics 注册器）
      otlp.rs            # OTLP gRPC/HTTP（Traces/Metrics/Logs）
      logs_http.rs       # JSON lines/批量 HTTP（Loki/自托管）
      kafka.rs           # Kafka 证据事件导出
      stdout.rs          # 本地开发（落 stdout）
    presets/
      metrics_families.rs# 标准指标族定义/注册助手
      spans.rs           # Span 命名/属性模板
    prelude.rs
```

**Features（按需裁剪）**

- `prometheus`（/metrics 暴露）
- `otlp`（OpenTelemetry 导出）
- `logs-http`（JSON lines）
- `kafka`（证据总线）
- `macros`（`obs_counter!`/`obs_histogram!` 等）
- `tail-sampling`（尾部采样器）
- `redaction-advanced`（正则/路径表达式屏蔽）

------

## 2. 统一数据模型（`model.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize)]
pub struct LogEvent {
  pub ts_ms: i64,
  pub level: LogLevel,               // Info|Warn|Error|Critical
  pub msg: String,                   // 公共视图消息（脱敏）
  pub labels: std::collections::BTreeMap<&'static str, String>,
  pub fields: serde_json::Map<String, serde_json::Value>, // 仅摘要/指纹/长度
}

pub enum LogLevel { Info, Warn, Error, Critical }

/// 指标描述（注册时使用）
pub struct MetricSpec {
  pub name: &'static str,            // e.g. "llm_requests_total"
  pub kind: MetricKind,              // Counter | Gauge | Histogram
  pub help: &'static str,
  pub buckets_ms: Option<&'static [u64]>, // Histogram专用（毫秒）
  pub stable_labels: &'static [&'static str], // 必填标签键
}
pub enum MetricKind { Counter, Gauge, Histogram }

/// Trace 上下文（与拦截器/OTEL互操作）
#[derive(Clone, Debug)]
pub struct SpanCtx {
  pub trace_id: Option<String>,
  pub span_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ObserveCtx {
  pub tenant: String,
  pub subject_kind: Option<String>,
  pub route_id: Option<String>,
  pub resource: Option<String>,
  pub action: Option<String>,
  pub code: Option<String>,          // 稳定错误码（如有）
  pub config_version: Option<String>,
  pub config_checksum: Option<String>,
  pub span: SpanCtx,
}

/// 证据信封：统一以 Envelope<T> 出口（只存摘要）
#[derive(Clone, Debug, serde::Serialize)]
pub struct EvidenceEnvelope<T: serde::Serialize> {
  pub envelope: sb_types::Envelope<T>,
}
```

------

## 3. SDK/SPI（`sdk/*`）

### 3.1 Logger（结构化日志 + 屏蔽）

```rust
#[async_trait::async_trait]
pub trait Logger: Send + Sync {
  async fn log(&self, ctx: &ObserveCtx, ev: LogEvent);
}

pub struct LogBuilder {
  level: LogLevel, msg: String,
  fields: serde_json::Map<String, serde_json::Value>,
}
impl LogBuilder {
  pub fn new(level: LogLevel, msg: impl Into<String>) -> Self { /* ... */ }
  pub fn field(mut self, k: &str, v: serde_json::Value) -> Self { /* ... */ }
  pub fn finish(self, ctx: &ObserveCtx, redactor: &dyn Redactor) -> LogEvent { /* 走屏蔽/裁剪 */ }
}
```

### 3.2 Meter（指标 + 宏）

```rust
#[async_trait::async_trait]
pub trait Meter: Send + Sync {
  fn counter(&self, spec: &'static MetricSpec) -> Box<dyn Counter>;
  fn histogram(&self, spec: &'static MetricSpec) -> Box<dyn Histogram>;
  // gauge 省略
}

pub trait Counter: Send + Sync {
  fn inc(&self, labels: &std::collections::BTreeMap<&'static str, String>, v: u64);
}
pub trait Histogram: Send + Sync {
  fn observe_ms(&self, labels: &std::collections::BTreeMap<&'static str, String>, ms: u64);
}

/// (可选) 宏：保证标签白名单 & 编译期拼写
#[macro_export]
macro_rules! obs_counter {
  ($meter:expr, $spec:expr, { $($k:literal => $v:expr),* $(,)? }, $val:expr) => {{
    let mut map = std::collections::BTreeMap::new();
    $( map.insert($k, $v.to_string()); )*
    $meter.counter($spec).inc(&map, $val);
  }};
}
```

### 3.3 Tracer（与 OTEL 互操作）

```rust
#[async_trait::async_trait]
pub trait Tracer: Send + Sync {
  fn start_span(&self, name: &str, ctx: &ObserveCtx) -> Box<dyn SpanGuard>;
  fn set_attr(&self, k: &str, v: &str);
}
pub trait SpanGuard: Send {
  fn end(self: Box<Self>, status_code: Option<&'static str>, elapsed_ms: u64);
}
```

> 默认实现基于 OpenTelemetry（feature: `otlp`），`ObserveCtx.span` 的 trace/span 写入/读取由拦截器打通。

### 3.4 EvidenceSink（证据总线）

```rust
#[async_trait::async_trait]
pub trait EvidenceSink: Send + Sync {
  async fn emit<T: serde::Serialize + Send + Sync + 'static>(&self, ev: EvidenceEnvelope<T>);
}
```

> 上游模块（Sandbox/Tools/LLM/Tx/Storage）统一调用 `emit()`；路由到 `export::{kafka|logs_http|otlp}` 按批发送。

------

## 4. 采样（`pipeline/sampler.rs`）

### 4.1 Head Sampler（入口概率/权重）

```rust
pub struct HeadRule {
  pub ratio: f64,                         // 0.0 ~ 1.0
  pub match_tenant: Option<String>,
  pub match_route: Option<String>,
  pub force_on_error: bool,               // 出错强采
}
pub trait HeadSampler: Send + Sync {
  fn should_sample(&self, ctx: &ObserveCtx) -> bool;
}
```

规则自上而下匹配；若 `code` 存在且 `force_on_error`，则必采。

### 4.2 Tail Sampler（按结果/尾延/高成本）

```rust
pub struct TailRule {
  pub slow_ms: Option<u64>,
  pub high_cost: bool,                    // 来自 usage/cost/budget 标签
  pub error_always: bool,
}
pub trait TailSampler: Send + Sync {
  fn decide(&self, span_name: &str, elapsed_ms: u64, ctx: &ObserveCtx, extra: &serde_json::Value) -> bool;
}
```

> 组合策略：`sample = head || tail`；tail 的实现为**环形缓冲 + 条件触发**（仅在 `tail-sampling` feature 开启时启用）。

------

## 5. 屏蔽/脱敏（`pipeline/redactor.rs`）

```rust
pub trait Redactor: Send + Sync {
  fn redact_log(&self, ev: &mut LogEvent);
  fn redact_json(&self, v: &mut serde_json::Value);
}

pub struct RedactPolicy {
  pub deny_keys: Vec<String>,                  // 正则/字面（password|secret|token|authorization|cookie）
  pub allow_keys: Vec<String>,                 // 允许例外
  pub max_string: usize,                       // 字符串截断
  pub max_array: usize,                        // 数组截断
  pub max_object_fields: usize,                // 对象字段上限
}
```

行为：深度遍历 JSON，匹配 `deny_keys` 的键值 → `"***"`；超长字段按长度截断；数组/对象按上限保留前 N 项并计数 `truncated_n`。

------

## 6. 留存策略（`pipeline/retention.rs`）

```rust
pub struct RetentionPolicy {
  pub metrics_days: u32,
  pub traces_days: u32,
  pub logs_days: u32,
  pub evidence_days: u32,
}
pub trait RetentionTagger {
  fn tag(&self, kind: &'static str, labels: &mut std::collections::BTreeMap<&'static str, String>);
}
```

> 留存由**后端**落地；SDK 在导出时附加 `retention=*` 标签或 header，帮助后端按类目归档。

------

## 7. 导出适配（`export/*`）

### 7.1 Exporter SPI

```rust
#[async_trait::async_trait]
pub trait LogExporter: Send + Sync { async fn export(&self, batch: Vec<LogEvent>) -> Result<(), ObserveError>; }
#[async_trait::async_trait]
pub trait MetricExporter: Send + Sync { async fn register(&self, spec: &'static MetricSpec); /* ...push/pull... */ }
#[async_trait::async_trait]
pub trait TraceExporter: Send + Sync { /* 与 OTEL SDK 绑定，接口留空占位 */ }
#[async_trait::async_trait]
pub trait EvidenceExporter: Send + Sync {
  async fn export<T: serde::Serialize + Send + Sync + 'static>(&self, batch: Vec<EvidenceEnvelope<T>>) -> Result<(), ObserveError>;
}
```

### 7.2 Prometheus（`export/prometheus.rs`）

- 采用**拉取式**：暴露 `/metrics` handler；
- `MetricSpec.kind == Histogram` 使用默认桶（毫秒）：`[5,10,20,50,100,200,500,1000,2000]`；
- 标签白名单由 `labels.rs` 提供，非白名单标签**拒绝注册**（防“标签爆炸”）。

### 7.3 OTLP（`export/otlp.rs`）

- 支持 `grpc` 与 `http/protobuf`；
- Traces 直接使用 OpenTelemetry SDK；Metrics/Logs 通过转换器桥接 `MetricSpec/LogEvent`；
- 批处理：`max_batch=20_000 events or 5s`；压缩 `gzip`；失败按指数退避（不阻塞业务路径）。

### 7.4 Logs HTTP（`export/logs_http.rs`）

- JSON lines，字段：`ts`, `level`, `msg`, `labels`, `fields`；
- 批量 `N=1000`/`2s` 推送；网络不可用时丢弃**低优先级**日志（`level=INFO`）并计数 `logs_dropped_total`。

### 7.5 Kafka（`export/kafka.rs`）

- topic：`evidence.<env>`；key=`tenant:envelope_id`；
- 借助 `EvidenceExporter` 实现精准一次**至少一次**（由消费端幂等）；支持压缩 `zstd`。

### 7.6 Stdout（`export/stdout.rs`）

- 本地开发使用；单行 JSON；默认只打印 `level>=WARN` 与所有 Evidence。

------

## 8. 标签与“标准指标族”（`labels.rs`/`presets/metrics_families.rs`）

### 8.1 标签白名单（最小集）

```rust
pub const LBL_MIN: &[&str] = &[
  "tenant","resource","action","route_id","service","method",
  "code","kind","retryable","severity",
  "model_id","provider","tool_id","sandbox_domain","storage_table","tx_kind",
  "config_version","config_checksum"
];
```

- SDK 在注册/观测时**静态检查**是否全部位于白名单；超出将被移除并计数 `labels_stripped_total{label}`。

### 8.2 指标族注册助手

```rust
pub mod metrics {
  pub static HTTP_REQS: MetricSpec = MetricSpec {
    name:"http_requests_total", kind:MetricKind::Counter,
    help:"HTTP requests", buckets_ms: None,
    stable_labels: &["tenant","route_id","code"]
  };
  pub static HTTP_LAT_MS: MetricSpec = MetricSpec {
    name:"http_latency_ms_bucket", kind:MetricKind::Histogram,
    help:"HTTP latency (ms)", buckets_ms: Some(&[5,10,20,50,100,200,500,1000,2000]),
    stable_labels: &["route_id"]
  };
  // …… LLM/Tools/Sandbox/Storage/Tx 族同理，按 SB-07/08/09/10 约定预置
}
```

------

## 9. 与上游模块的接入契约

- **拦截器（SB-05）**：
  - 入站：构造 `ObserveCtx{ tenant, route_id, config_* , span }`；
  - 出站：写 `http_requests_total` 与 `http_latency_ms_bucket`；错误 `code` 写入 labels；
- **Auth（SB-04）**：`authn_latency_ms`, `authz_allow_total{code}`；
- **LLM（SB-07）**：`llm_requests_total{provider,model}`、`llm_first_token_ms_bucket`；
- **Tools/Sandbox（SB-08/06）**：`tool_invocations_total{tool_id}`, `sandbox_exec_total{domain}`, `sandbox_budget_bytes{dir}`；Evidence 走 `EvidenceSink.emit()`；
- **Storage（SB-09）**：`storage_requests_total{kind,table}`, `storage_latency_ms_bucket{table}`；
- **Tx（SB-10）**：`tx_outbox_*`, `tx_saga_*`, `tx_idempo_*`；Dead/Replay 事件同时作为 Evidence 导出。

------

## 10. 错误与降级（`errors.rs`/`pipeline/router.rs`）

- **公共视图优先**：任何 `Logger/Meter/Tracer/Evidence` 错误**只影响观测**，不得回传到业务层；
- **回压策略**：
  - 队列满时丢弃低优先级日志；
  - 指标/Trace 尽可能缓冲；
  - Evidence **不丢弃**（失败则落本地 WAL / 再投，RIS 可简化为内存重试）。

------

## 11. 配置热更（与 `soulbase-config`）

- `observe.sampling.*`、`redaction.*`、`retention.*`、`exporters.*` 支持热更；
- SDK 采用 **双缓冲**：新策略在**下一个请求**或**下一个批次**生效；
- 关键变更（导出端点/凭证）在安全通道内更新（避免日志写入敏感信息）。

------

## 12. 测试与验收

- **契约测试**：
  - 标签白名单校验、稳定指标族注册；
  - 采样（head/tail）与屏蔽策略覆盖；
  - 导出器最小可用性（prom/otlp/logs_http/kafka）的 smoke tests。
- **黑盒**：
  - ERROR/CRITICAL 与 p99 慢调用必采；
  - 高成本事件保留；
  - 红线字段不出现在导出 payload。
- **性能**：
  - 观测开销 p95 ≤ 1ms（不含后端）；
  - 批处理/压缩带宽收益验证。

------

## 13. 开放事项

- **Tail-based 采样器**与 OTEL 的桥接策略（本地聚合 vs Collector）；
- Evidence 的**签名/摘要**与合规归档（与 `soulbase-a2a`）；
- 指标族的**自动仪表盘模板**与报警规则库（Grafana/Alertmanager）。

------

> 上述 TD 保持与全栈模块**同频共振**：统一标签/稳定错误码/Envelope 证据/最小披露。若确认无误，我将按“三件套”继续输出 **SB-11-RIS（最小可运行骨架）**，包含可编译的 SDK（Logger/Meter/Tracer/Evidence），内置 Prometheus/Stdout 导出、head/tail 采样/屏蔽/留存最小实现与标准指标族注册示例及单测。
