### **文档 SB-11：soulbase-observe（统一观测 / Logs · Metrics · Traces · Evidence Bus）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：为 Soul 生态提供**统一的可观测性基座**，以**结构化日志（Logs）**、**度量指标（Metrics）**、**分布式追踪（Traces）**、**证据事件（Evidence Bus）\**四路并行的方式，贯穿\**请求入站 → 授权/配额 → 工具受控执行 → LLM 调用 → 存储/事务 → 出站响应**的全链路，确保：
  1. **可定位**：单次请求可一跳定位到错误码、root cause、影响组件与关键参数摘要；
  2. **可量化**：统一的 p50/p95/p99、吞吐、成本/预算、错误率与重试/补偿率等指标；
  3. **可回放**：以 **Envelope** 为主线，重建关键副作用摘要；
  4. **可治理**：以**最小披露**与**红线屏蔽**保障隐私与安全合规。
- **范围**：定义**数据模型、标签（labels）与采样/留存/屏蔽策略**；提供**接入契约**与**导出适配**（OTLP/Prometheus/Logs/Loki/Kafka…）；与 `soulbase-*-` 全家桶（types/errors/auth/interceptors/llm/tools/sandbox/storage/tx/config/qos）一致化的接口。
- **非目标**：不绑定单一观测后端；不承担业务报警规则（提供规则模板与建议阈值）；不替代合规平台（提供导出钩子）。

------

#### **1. 功能定位（Functional Positioning）**

- **统一语义层**：沉淀**稳定标签键**与事件/指标命名约定，消除各服务统计口径漂移；
- **证据单一真相源（SSoT）**：以 `Envelope<T>` 作为**观测与审计**的共用载体（只存摘要）；
- **面向治理**：与 `soulbase-errors`、`-qos` 与 `-config` 协同，形成**错误/成本/性能**三位一体的客观视图。

------

#### **2. 系统角色与地位（System Role & Position）**

- 层级：**基石层 / Soul-Base**（横切全栈）；
- 关系：
  - `soulbase-interceptors`：入站初始化 `X-Request-Id/TraceContext/X-Config-*`，统一错误公共视图；
  - `sb-types`：复用 `Envelope/TraceContext/Subject/Consent`；
  - `soulbase-errors`：每条观测记录带稳定错误码 `code/kind/retryable/severity`；
  - `soulbase-llm/tools/sandbox/storage/tx`：产出规范化指标与 Evidence 事件；
  - `soulbase-config/qos`：驱动采样/留存/阈值与成本聚合策略。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

**3.1 统一标签键（Label Keys，最小必备）**

- **主体维度**：`tenant`, `subject_kind`, `client_id`（如有）
- **请求维度**：`route_id|service|method`, `resource`, `action`, `origin`(llm|api|system), `idempotent`(true|false)
- **错误维度**：`code`, `kind`, `retryable`, `severity`（来自 `soulbase-errors`）
- **性能/预算维度**：`model_id|provider`（LLM）、`tool_id`、`sandbox_domain`(fs|net|browser|proc|tmp)、`storage_table`、`tx_kind`(outbox|saga|idempo)
- **追踪维度**：`trace_id`, `span_id`, `envelope_id`, `partition_key`
- **配置快照**：`config_version`, `config_checksum`

**3.2 日志（Structured Logs）**

- **LogEvent**：`timestamp`, `level`, `message`, `labels{…}`, `fields{…}`（fields 仅存**摘要**：hash/size/count）
- **级别**：`INFO|WARN|ERROR|CRITICAL`；严格禁止在日志中落原文请求/响应/密钥/令牌。

**3.3 指标（Metrics）**

- **类型**：`counter/gauge/histogram`；
- **汇总维度**：由上节标签构成，严格控制**标签基数**；
- **序列**：每模块产出一组“核心指标族”（见下 §6 与各模块规约）。

**3.4 追踪（Traces）**

- **Span 命名规范**：`svc.{area}.{op}`（如 `svc.auth.decide`、`svc.llm.chat`、`svc.sandbox.exec.net`）；
- **Span 字段**：`start/end/duration`, 关键标签（见 3.1），**不**附带大对象。

**3.5 证据事件（Evidence Bus）**

- **Envelope**：
  - 公共字段：`envelope_id, produced_at, tenant, subject_id, partition_key, trace`
  - 事件种类：`ToolInvokeBegin/End`, `SandboxBegin/End`, `AuthDecision`, `OutboxDispatched/Dead`, `SagaStepDone/Compensate`, `StorageQuery`, `LlmRequest/Delta/Response`…
  - **只存摘要**：`inputs_digest/outputs_digest/side_effects/budget_used/policy_hash/error_code`

------

#### **4. 不变式（Invariants）**

1. **最小披露**：日志/证据**仅**存 hash/长度/计数/指纹；任何明文密钥/Token/PII 均屏蔽或脱敏；
2. **稳定标签**：所有指标与事件**必须**携带 `tenant` 与 `code`（如适用），追踪 span **必须**携带 `trace_id`；
3. **结构化优先**：只接受结构化日志（JSON）；拒绝自由文本作为主通道；
4. **采样先导**：默认 head-based 采样（如 1%），对 `ERROR/CRITICAL`、`p99 慢调用`、`安全/高风险路径`强制全量；
5. **留存可控**：指标 ≥ 14d、追踪 ≥ 7d、日志 ≥ 7–30d（可配置），证据事件按合规策略长期归档（摘要）；
6. **时钟统一**：全部时间戳以 **UTC 毫秒**；
7. **幂等与去重**：以 `envelope_id + span_id` 作为去重锚点（落地端处理）。

------

#### **5. 能力与接口（Abilities & Interfaces）**

> 仅定义能力口径；具体 SPI/代码在 TD/RIS 落地（本模块通常提供 SDK/中间件）。

- **日志 SDK**：`log(event: LogEvent)`；支持 redaction（字段屏蔽表）、动态级别与关键路径强制落盘；
- **指标 SDK**：`counter/gauge/histogram` 注册与上报；内置**标准桶**（latency：`[5,10,20,50,100,200,500,1000,2000]ms`）；
- **追踪 SDK**：与 OpenTelemetry 兼容的 Span API；拦截器自动注入 `TraceContext`；
- **证据 Bus**：`emit(envelope<EvidenceEvent>)`；提供**异步缓冲 + 批量**与**回压**策略；
- **导出器（Exporters）**：`otlp`, `prometheus`, `logs-http/loki`, `kafka`; 支持压缩与 TLS；
- **采样器（Samplers）**：head-based（概率/比率）、tail-based（按错误码/时延/代价），可级联；
- **红线屏蔽（Redaction）**：内置敏感键正则（`password|secret|token|authorization|cookie`）；提供**白名单例外**机制；
- **成本聚合**：接受 `soulbase-llm/tools/sandbox/storage/tx` 的**预算/成本**事件，按租户/模型/工具聚合。

------

#### **6. 指标与 SLO（Metrics, SLOs & Acceptance）**

**6.1 指标族（示例，均带最小标签集）**

- **入站/出站**（interceptors）：`http_requests_total{tenant,route_id,code}`；`http_latency_ms_bucket{route_id}`；`idempotency_hits_total{tenant}`
- **Auth**：`authn_latency_ms`，`authz_allow_total{code}`，`quota_consumed_total{bucket}`
- **LLM**：`llm_requests_total{provider,model}`，`llm_first_token_ms_bucket`，`llm_tokens{type=input|output}`，`llm_errors_total{code}`
- **Tools/Sandbox**：`tool_invocations_total{tool_id}`，`sandbox_exec_total{domain}`，`sandbox_budget_bytes{dir}`，`sandbox_errors_total{code}`
- **Storage**：`storage_requests_total{kind,table}`，`storage_latency_ms_bucket{table}`，`storage_errors_total{code}`
- **TX**（Outbox/Saga/Idempo）：`tx_outbox_dispatched_total{topic}`，`tx_outbox_dead_total{code}`，`tx_saga_started_total{def}`，`tx_idempo_hits_total`

**6.2 验收与 SLO**

- **一致性**：≥ 99.9% 的错误事件带 `code/kind/retryable/severity`；
- **覆盖率**：核心路径（Auth/LLM/Tools/Sandbox/Storage/Tx）**100%** 产出指标；
- **延迟基线**：指标采集开销 p95 ≤ 1ms（不包括后端写入）；
- **采样正确性**：Tail-sampling 命中**所有错误事件**与 p99 慢调用。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：所有服务/SDK/中间件（提供事件/指标/Span）；
- **下游**：可插拔导出器（Prometheus/OTLP/Loki/Kafka 等）；
- **边界**：不直接持久化业务数据；不在 SDK 内做业务重试逻辑（仅缓冲/回压与降级）。

------

#### **8. 风险与控制（Risks & Controls）**

- **隐私泄露** → 默认屏蔽敏感键；日志/证据只存摘要；可配置**强制屏蔽表**；
- **标签爆炸** → 对高基数标签（如 `trace_id`）仅在 Traces/Logs 使用，禁止进入 Metrics；强制**标签白名单**；
- **观测雪崩** → 后端异常时**降级为本地环形缓冲**，超限丢弃低优先级日志；错误率/掉包率可报警；
- **采样偏差** → 以 **错误码/代价/尾延**为优先的 Tail-sampling；关键租户/路由支持**固定采样权重**；
- **时钟漂移** → 统一 NTP；对跨区追踪以 `TraceContext` 时间为准。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 请求入站 → 出站**

1. 拦截器 `ContextInit` 生成 `X-Request-Id/Trace`，写入 Span（`svc.http.inbound`）；
2. AuthN/AuthZ/Quota 产生指标与（必要时）Evidence；
3. 业务处理/工具执行/LLM 调用/存储/事务按模块各自产出指标与 Evidence；
4. 错误经 `soulbase-errors` 规范化，日志仅公共视图 + 事件摘要；
5. 出站写 `X-Config-*` 与 `code`；Span 结束并聚合。

**9.2 工具受控执行（Sandbox）**

1. `SandboxBegin` Evidence：`capability/profile_hash/inputs_digest`；
2. 执行过程按域计量 `budget_used`；
3. `SandboxEnd` Evidence：`status/error_code/side_effects/outputs_digest`；
4. 指标：`sandbox_exec_total/latency/budget_bytes/errors_total`。

**9.3 可靠事务（TX）**

1. `OutboxEnqueued` → `Dispatched|Retry|Dead`；
2. `SagaStarted` → `StepDone|Compensate|Completed|Failed`；
3. `IdempoCheck` → `Hit|Miss`；
4. 指标：`tx_*` 家族。

------

#### **10. 配置（Config）与策略建议**

```yaml
observe:
  sampling:
    head: 0.05                 # 5% 全链路采样
    tail:
      error_always: true
      slow_ms: 1000            # p99 慢调用强制保留
      high_cost: true          # 高成本（tokens/bytes/cpu_ms）事件保留
  redaction:
    keys: ["password", "secret", "token", "authorization", "cookie"]
    allowlist: []              # 例外键名单
  retention:
    metrics_days: 14
    traces_days: 7
    logs_days: 7
    evidence_days: 30          # 仅摘要
  exporters:
    prometheus: { port: 9090 }
    otlp: { endpoint: "https://otlp.your.org", tls: true }
    logs_http: { url: "https://loki.your.org/ingest" }
    kafka: { brokers: ["k1:9092"], topic: "evidence" }
```

------

#### **11. 开放问题（Open Issues / TODO）**

- **证据与合规**：Evidence 摘要最小字段清单与加密/签名策略（与 `soulbase-a2a` 协调）；
- **成本归集**：跨模块成本（LLM tokens / Sandbox bytes / Storage IO / TX 重试）的一体化“账本”视图；
- **动态采样**：基于 SLO 偏差的自适应采样（SRE 策略）；
- **跨区域追踪**：Trace join 与时钟校正策略；
- **观测 UI**：统一仪表盘模板（Grafana/Tempo/Loki/Jaeger）与报警规则库（阈值建议）。

------

> 本规约与全栈模块**同频共振**：以 **Envelope** 为证据载体、以 **稳定错误码**为治理原语、以**最小披露**为安全底线、以**统一标签**为度量基座。若确认无误，我将按“三件套”继续输出 **SB-11-TD（技术设计）**，给出 SDK/SPI、采样/屏蔽/留存策略、导出适配与指标族落地细节，并随后提供 **SB-11-RIS（最小可运行骨架）**。
