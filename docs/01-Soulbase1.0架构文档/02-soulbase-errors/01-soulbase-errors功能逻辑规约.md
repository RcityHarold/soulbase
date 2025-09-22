### **文档 SB-02：soulbase-errors（错误域与统一编码 / Error Domain & Codes）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：为 Soul 全生态（SoulseedAGI、Soul-Auth、Soul-Hub、Soul-Browser、AI 记忆等）提供**统一的错误域（Error Domain）与稳定错误编码体系**，确保：
  1. 不同组件之间**错误语义一致**、**可机读**、**可观测**、**可治理**；
  2. 支持**跨协议映射**（HTTP/gRPC/队列/CLI），并内置**重试与可恢复性语义**；
  3. **零敏感信息泄露**与**最小必要披露**，区分**用户可见信息**与**开发者诊断细节**。
- **范围**：仅定义**错误分类、编码、字段规范与映射规则**；不实现日志/指标/追踪（属 `soulbase-observe`），不实现网关行为（属 Soul-Hub），不实现策略（保留在内核）。
- **溯源**：从原架构中“全局 ID 与错误处理”、工具/LLM/拦截器/事务的分散错误口径**统一下沉**；形成平台级的**单一真相源（SSoT）**。

------

#### **1. 功能定位（Functional Positioning）**

- **标准化**：建立**命名空间化**错误域与**稳定错误码**（Stable Code），支持版本化与契约测试。
- **桥接层**：提供到 HTTP 状态码、gRPC Status、消息队列 nack/retry 的**规范映射口径**（本规约层面定义规则）。
- **治理入口**：为 SLO/熔断/退避/拨测与回放提供**可分类、可聚合**的错误标签（retryable、severity、category）。

------

#### **2. 系统角色与地位（System Role & Position）**

- 所处层级：**基石层 / Soul-Base**，被所有上层直接依赖。
- 与相关模块关系：
  - `sb-types`：复用 `Envelope/Subject/TraceContext` 等承载上下文。
  - `soulbase-interceptors`：负责把运行时错误**就地规范化**为本模块定义的错误结构并**外显给调用方**。
  - `soulbase-observe`：消费错误标签做日志/指标/Trace 聚合。
  - `soulbase-contract-testkit`：对错误码与映射规则做**契约测试**。
  - Soul-Hub：作为入口/出口，对 HTTP/gRPC 映射结果进行**一致化呈现**与审计。

------

#### **3. 错误域模型与分类（Error Model & Taxonomy）**

**3.1 顶层分类（ErrorKind，稳定枚举）**

- `Auth`（未认证/未授权/声明无效/过期）
- `Quota`（配额/速率限制/预算不足）
- `Schema`（请求体/参数/JSON-Schema/IDL 校验失败）
- `PolicyDeny`（策略拒绝：工具/模型/操作被策略屏蔽）
- `Sandbox`（受控执行失败：越权、封禁、环境限制）
- `Provider`（外部提供方失败：LLM、对象存储、第三方 API）
- `Storage`（数据库/缓存/对象存储错误）
- `Timeout`（超时）
- `Conflict`（资源冲突/并发写入/乐观锁失败）
- `NotFound`（资源不存在）
- `Precondition`（前置条件未满足/不一致）
- `Serialization`（序列化/反序列化失败）
- `Network`（网络不可达/连接被重置）
- `RateLimit`（限流命中，属 `Quota` 子类，保留独立枚举以便快速判定）
- `QosBudgetExceeded`（成本/Token/执行预算用尽）
- `ToolError`（工具执行错误：参数、执行、结果不一致）
- `LlmError`（模型调用错误：上下文超限、内容安全/拒绝、对齐策略拒绝）
- `A2AError`（跨域互认/验签/账本不一致）
- `Unknown`（未知/未分类——**目标≤0.1%**，仅作为兜底）

> **说明**：`Provider` 泛指第三方；若错误来自 LLM，使用更具体的 `LlmError`；若来自工具，使用 `ToolError`。

**3.2 错误结构（逻辑字段）**

- `code`（**稳定错误码**，形如 `AUTH.UNAUTHENTICATED`、`LLM.TIMEOUT`；参见 4. 不变式）
- `kind`（上面列举的顶层分类）
- `message_user`（**用户可见**的短语，脱敏且可本地化）
- `message_dev`（开发者诊断信息，默认不外泄，进入审计/日志）
- `http_status`（建议状态码，如 401/403/422/429/500/503）
- `grpc_status`（建议 gRPC status，如 `UNAUTHENTICATED`/`PERMISSION_DENIED`/`RESOURCE_EXHAUSTED`/`UNAVAILABLE`）
- `retryable`（`None | Transient | Permanent`）
- `severity`（`Info | Warn | Error | Critical`）
- `cause_chain`（上游错误链摘要，按最小披露原则脱敏）
- `meta`（键值 Map：`provider=model:gpt-4o`、`quota.bucket=tenantA`、`tool=browser` 等）
- `trace`（可选 TraceContext，或引用 `Envelope.trace`）
- `correlation_id`（优先引用现有 `Envelope` 字段）

**3.3 典型错误码（Stable Codes，示例集合）**

- **Auth**：`AUTH.UNAUTHENTICATED`、`AUTH.FORBIDDEN`、`AUTH.TOKEN_EXPIRED`、`AUTH.CLAIM_INVALID`
- **Quota**：`QUOTA.RATE_LIMITED`、`QUOTA.BUDGET_EXCEEDED`
- **Schema**：`SCHEMA.VALIDATION_FAILED`、`SCHEMA.TYPE_MISMATCH`
- **Policy**：`POLICY.DENY_TOOL`、`POLICY.DENY_MODEL`
- **Sandbox**：`SANDBOX.PERMISSION_DENY`、`SANDBOX.CAPABILITY_BLOCKED`
- **Provider/LLM**：`LLM.TIMEOUT`、`LLM.CONTEXT_OVERFLOW`、`LLM.SAFETY_BLOCK`、`PROVIDER.UNAVAILABLE`
- **Storage**：`STORAGE.UNAVAILABLE`、`STORAGE.CONFLICT`、`STORAGE.NOT_FOUND`
- **Tool**：`TOOL.INPUT_INVALID`、`TOOL.EXECUTION_FAILED`、`TOOL.RESULT_INCONSISTENT`
- **A2A**：`A2A.SIGNATURE_INVALID`、`A2A.LEDGER_MISMATCH`
- **Unknown**：`UNKNOWN.INTERNAL`

------

#### **4. 不变式（Invariants）**

1. **稳定编码**：`code` 为**平台稳定识别符**，一经发布不得语义漂移；仅允许**新增**，**不允许复用**旧码。
2. **一对一映射**：每个 `code` 必须定义**建议**的 `http_status`/`grpc_status` 与 `retryable`；上层可按策略**降级/升级**但不得违背语义。
3. **最小披露**：默认仅向外暴露 `code` + `message_user`；`message_dev/cause_chain/meta` 进入审计与内部日志。
4. **可聚合**：错误必须可按 `kind/code/retryable/severity` 聚合统计，用于 SLO/熔断/拨测。
5. **协议无关**：错误结构与编码**先于**协议存在；映射仅是投影。
6. **零策略**：本模块不做“是否重试/如何降级”的决策，仅提供**建议**标签（`retryable` 等）。
7. **一致追踪**：错误应关联 `correlation_id/trace`，确保端到端追踪与回放。
8. **本地化**：仅 `message_user` 可本地化；编码/枚举**永远使用英文**，避免跨语言分歧。

------

#### **5. 能力与接口（Abilities & Abstract Interfaces）**

> 具体 Trait/类型与构造器在“技术设计文档（SB-02-TD）”中实现；此处规定**能力与行为**。

- **构造能力**：用统一工厂/构造器按 `kind+code` 生成错误对象，自动填充建议映射与标签。
- **跨协议映射**：提供 `to_http_status()`、`to_grpc_status()` 的**规范口径**；其他协议参照此规则扩展。
- **重试语义**：提供 `retryable()` 与**指数退避建议**枚举（仅标签，策略由上层实现）。
- **包装与透传**：可以将外部错误（DB/HTTP 客户端/LLM SDK）**分类包装**为平台错误；保留 cause，但**默认脱敏**。
- **脱敏输出**：提供 `to_public()`/`to_audit()` 两种视图，分别用于**对外 JSON**与**内部审计**。
- **指标标签**：规范导出 `kind/code/retryable/severity/provider/tool/model/tenant` 等标签键集合，供 `soulbase-observe` 直接消费。
- **契约测试钩子**：暴露全部**已注册稳定码**列表与映射，以便 `soulbase-contract-testkit` 校验“**无缺失/无漂移/无重复**”。

------

#### **6. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **SLO-1**：生产环境中，`UNKNOWN.*` 的占比 **≤ 0.1%**，且每次回归中不增长。
- **SLO-2**：所有对外响应错误必须携带**稳定错误码**（非 200/OK 路径 100% 覆盖）。
- **SLO-3**：错误映射的一致性（HTTP/gRPC）在契约测试中**100% 通过**。
- **SLO-4**：Top N 错误（按 24h 计）可在观测面**一跳定位**到 `kind/code` 与重试标签。
- **验收**：
  - 契约测试：对每个 `code` 的 HTTP/gRPC 映射、脱敏视图、标签完整性；
  - 回放：在 `soulbase-benchmark` 中对错误对象序列化/反序列化稳定性做基线。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：无（错误域自身是底座）。
- **下游**：所有服务/内核/中间件/SDK；`soulbase-interceptors` 负责落地入口/出口标准化；`soulbase-observe` 负责指标/日志。
- **边界**：
  - 不做持久化/日志/埋点；
  - 不依赖具体 Provider/DB 库；
  - 不包含策略决策；
  - 不替代业务校验（例如业务规则错误仍应映射到合适的 `kind/code`）。

------

#### **8. 风险与控制（Risks & Controls）**

- **码表膨胀/重复**：不同团队新增重复码 → **控制**：集中注册与 CI Lint（禁止重复/未映射/未文档化）。
- **语义漂移**：同一 `code` 不同服务含义不一 → **控制**：修改需发起 ADR 并 bump **MAJOR**；契约测试守护。
- **敏感泄露**：把 `message_dev` 外泄 → **控制**：`to_public()` 默认屏蔽，`Soul-Hub` 仅透传公共视图。
- **误分类导致故障放大**（把永久性错误标成可重试） → **控制**：增设“**错误模拟与拨测**”用例，Policy/HealthGuardian 侧置专门护栏。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 入站请求失败（Schema 校验失败）**

1. `soulbase-interceptors` 执行校验 → 失败 → 构造 `SCHEMA.VALIDATION_FAILED`；
2. 生成公共视图（HTTP 422 + `code` + `message_user`），审计视图含 `message_dev` 与 `cause_chain`；
3. `soulbase-observe` 记录指标（`kind=Schema code=SCHEMA.VALIDATION_FAILED`）。

**9.2 工具调用被策略拒绝**

1. 内核策略钩子返回拒绝 → 标准化为 `POLICY.DENY_TOOL`（HTTP 403 / 永不重试）；
2. 公共视图返回简要原因，开发者视图记录被拒绝的 `tool`、所需 `scope` 等 meta。

**9.3 LLM 提供方超时**

1. Provider SDK 返回超时 → 包装为 `LLM.TIMEOUT`（HTTP 503 / `retryable=Transient`）；
2. `soulbase-observe` 聚合到**外部依赖可用性看板**；HealthGuardian 可据此熔断/降级。

------

#### **10. 开放问题（Open Issues / TODO）**

- `PolicyDeny` 与 `Auth.Forbidden` 的细粒度边界是否需要更细的子码（例如 `POLICY.DENY_SAFETY` vs `POLICY.DENY_BUDGET`）。
- 是否提供**跨语言错误码生成器**（TS/Go/Java 枚举与映射），随版本自动发布。
- A2A 错误与证据链的最佳最小字段集合（签名算法、链上/账本指纹对齐）。
- 与 `soulbase-qos` 的细粒度联动（把 `QosBudgetExceeded` 接入统一成本预算看板）。

------

> 本规约确定了 `soulbase-errors` 的**语义与边界**。确认无误后，我将输出**第二部分：技术设计（SB-02-TD）**，给出 `ErrorKind/ErrorCode`、公共/审计视图、跨协议映射与构造器的 Rust 接口与约束实现；随后给出**第三部分：最小可运行骨架（RIS）**以直接落库。
