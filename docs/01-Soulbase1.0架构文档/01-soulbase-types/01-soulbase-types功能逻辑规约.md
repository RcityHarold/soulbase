### **文档SB-01：sb-types（数据契约底座 / Data Contract Primitives）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：提供跨 Soul 生态（SoulseedAGI、Soul-Auth、Soul-Hub、Soul-Browser、AI 记忆等）统一、稳定、可版本化的**数据契约底座**（IDs、主体/租户、权限范围、同意凭据、Envelope 传输壳、因果/关联链路、分区键、时间戳等），消灭重复定义与语义漂移。
- **范围**：仅包含**与业务无关的通用原语**与约束；不包含错误域（归 `soulbase-errors`）、鉴权实现（归 `soulbase-auth`）、策略/状态机（保留在 SoulseedAGI 内核）。
- **溯源**：本模块从原文档的以下内容**下沉与归并**而来：
  1. 《文档03：核心数据模型（Core Data Models）》中的**通用原语**与承载字段（非强领域语义部分）；
  2. 《全局 ID 与错误处理》中的 **ID 体系/因果相关标识**（错误域另行下沉至 `soulbase-errors`）；
  3. 多处文档反复使用的 **主体/租户/范围/同意** 与 **Envelope** 口径（如工具链路、拦截器链、LLM 接口、A2A 协议）。
- **非目标**：不承诺特定存储/消息总线/模型供应商；不承载任何领域专属枚举或状态机。

------

#### **1. 功能定位（Functional Positioning）**

- 作为**单一真相源（SSoT）\**的数据类型库：所有服务/内核/工具/网关都以它为\**唯一契约**。
- 作为**显式数据流**的载体：通过 Envelope + 因果/关联标识，让事件/命令/调用的来龙去脉**可追溯、可回放**。
- 作为**安全语义的地基**：统一主体（Subject）、权限范围（Scope）、同意（Consent）的**结构与口径**，便于 AuthN/Z 与审计。

------

#### **2. 系统角色与地位（System Role & Position）**

- 所处层级：**基石层 / Soul-Base 底座**；被 SoulseedAGI 内核与各业务服务直接依赖。
- 与相关模块关系：
  - `soulbase-auth`：消费 `Subject/Scope/Consent` 做鉴权判定；
  - `soulbase-interceptors`：把网关/上游头信息与令牌映射到 `Envelope` 固定字段；
  - `soulbase-llm` / `soulbase-tools` / `soulbase-sandbox`：共享 `Id`、因果链、Envelope；
  - Soul-Auth/Soul-Hub：其输出（令牌/头）在服务侧被解析为本模块定义的主体与证据。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

**3.1 标识与时间（Identity & Time）**

- **Id**：不透明、全局唯一字符串，禁止外部依赖其内部结构。
- **CausationId / CorrelationId**：因果与跨流程关联的链路 ID。
- **Timestamp**：ms 精度的 UTC 时间戳；明确与业务时区解耦。
- **PartitionKey**：分区键，决定数据与事件的物理/逻辑分布（如 `tenant:conversation_id`）。

**3.2 主体与多租户（Subject & Multi-Tenancy）**

- **TenantId**：租户唯一标识。
- **Subject**：执行者/发起者（User / Service / Agent）；携带 `tenant` 与若干声明（claims）。
- **SubjectKind**：`User | Service | Agent`（仅类型，不含授权逻辑）。

**3.3 权限范围与同意（Scope & Consent）**

- **Scope**：`resource` + `action` + 可选 `attrs`（键值属性），表述最小必要权限单元。
- **Consent**：在高风险/敏感操作中，记录**用户明示同意**的范围与有效期（与 `Scope` 组合使用）。

**3.4 Envelope（统一传输壳）**

- **Envelope**：事件/命令/调用的统一外壳，承载：
  - `envelope_id`（Id）
  - `causation_id` / `correlation_id`（可选）
  - `produced_at`（Timestamp）
  - `partition_key`（字符串，必须明确且可复算）
  - `actor`（Subject）
  - `consent`（可选）
  - `schema_ver`（SemVer，表示 **Payload** 结构版本）
  - `payload`（类型参数 `T`，承载具体业务负载）
- **语义**：Envelope 为**不可变记录**；它绑定上下文、安全与审计信息，使任何负载 `T` 在平台内**可验证、可回放**。

**3.5 可观测与追踪（Observability & Tracing）**

- 规范化携带/映射 `TraceId/SpanId` 到 Envelope 的审计扩展域（不强制字段名，但强制**存在**与**传递**）。

------

#### **4. 不变式（Invariants）**

1. **不可变性**：Envelope 一经创建不可修改；变更以新 Envelope 追加表达（Append-Only）。
2. **明确分区**：每个 Envelope 必须有可复算的 `partition_key`，禁止“隐式分区”。
3. **版本化**：
   - `schema_ver` 遵循 **SemVer**：`MAJOR.MINOR.PATCH`；
   - **向后兼容规则**：`MINOR/PATCH` 只能新增可选字段；删除/重命名/强制改变语义必须提升 `MAJOR`。
4. **无策略**：本模块**不**落地任何策略/授权/路由逻辑；仅定义数据结构。
5. **主体一致性**：`actor.tenant` 与 `partition_key` 的租户维度必须一致（可静态/运行时校验）。
6. **最小必要披露**：`Subject/Consent` 仅包含授权与审计所需声明，避免携带多余敏感信息。
7. **可序列化与跨语言**：所有类型具备稳定的序列化语义（JSON 为最低通用格式），可生成 Schema。

------

#### **5. 能力与接口（Abilities & Abstract Interfaces）**

> 以下为**抽象能力**，技术细节/Traits/代码在“第二部分：技术设计”中给出。

- **类型定义能力**：提供 `Id/Timestamp/TenantId/Subject/Scope/Consent/Envelope<T>/CausationId/CorrelationId` 等原语及其**字段语义**。
- **Schema 能力**：为上述类型输出/校验 JSON-Schema 或等效 IDL 的**生成规范**（用于跨语言与契约测试）。
- **版本治理能力**：约定 `schema_ver` 的增量/破坏性变更规则与**兼容性矩阵**。
- **校验能力**：
  - Envelope 结构完整性校验（必填字段、分区键不为空、时间/ID 合法）；
  - `Subject` 与 `Consent/Scope` 的**结构性**校验（不做授权判定）。
- **映射能力（对接 Soul-Auth / Soul-Hub）**：
  - **令牌→主体**映射口径（`sub/iss/aud/tenant/roles/permissions/consents → Subject/Scope/Consent`）；
  - **头→Envelope**映射口径（`X-Request-Id/TraceId/Tenant/Consent-Token` 等）。
- **观测能力**：定义审计必需的扩展域（e.g. evidence snapshot 占位符），以便 `soulbase-observe` 使用。

------

#### **6. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **契约稳定性**：
  - **SLO-1**：`MINOR/PATCH` 版本发布后，对旧版消费者**零破坏**（契约测试通过率 100%）。
  - **SLO-2**：`MAJOR` 版本升级须提供**兼容垫片或迁移脚本**与发布说明。
- **Schema 生成**：
  - **SLO-3**：类型变更 1 分钟内能生成/更新对应 Schema 工件（CI 保障）。
- **校验**：
  - **SLO-4**：Envelope 结构与分区键校验**必须**在服务入口（拦截器）执行；失败即拒绝进入业务层。
- **验收标准**：
  - 通过 `soulbase-contract-testkit` 的**跨版本兼容用例**；
  - 通过 `soulbase-benchmark` 的**序列化/反序列化基线**（延迟/体积在阈值内）。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：无（作为底座，不依赖业务或外部系统）。
- **下游消费者**：`soulbase-auth`、`soulbase-interceptors`、`soulbase-llm`、`soulbase-tools`、`soulbase-sandbox`、`soulbase-storage`、`soulbase-qos`、`soulbase-observe`、`soulbase-tx`、`soulbase-a2a`、SoulseedAGI 内核各模块。
- **边界**：
  - 不实现任何鉴权、存储、网关、模型适配；
  - 不承载“强领域”结构（如对话节点、人格维度、业务状态机）；
  - 仅提供**可观测字段位**与**契约生成规则**，不绑定具体日志/追踪后端。

------

#### **8. 风险与控制（Risks & Controls）**

- **口径漂移**：多个仓库各自定义 `Subject/Envelope` → **控制**：以 `sb-types` 为唯一引用源，CI 设“重复定义”守卫。
- **破坏性变更**：无意修改字段语义 → **控制**：SemVer 严格执行 + 契约测试 + 需要架构评审。
- **过度泄露**：在 `Subject/Consent` 中携带非必要敏感信息 → **控制**：最小披露审查 + 拦截器自动剔除未使用声明。
- **滥用因果链**：错误复用 `causation_id/correlation_id` → **控制**：拦截器按标准规则注入/派生，统一文档化。
- **分区键错误**：导致热点或跨租户污染 → **控制**：提供参考分区策略/lint，入站校验必过才入库。

------

#### **9. 关键交互序列（Key Interaction Sequences，示意）**

**9.1 入站请求→Envelope 化（服务入口）**

1. Soul-Hub 完成 OIDC/JWT 基校验并透传头；
2. 服务侧拦截器解析令牌→构造 `Subject`；
3. 生成 `envelope_id / causation_id / correlation_id / produced_at / partition_key`；
4. 补齐 `consent`（如有）与追踪信息→形成 `Envelope<IncomingCommand>`；
5. 交由业务处理/内核策略；原始 Envelope 作为**审计记录**写入观察管道。

**9.2 工具/外部行动调用（链路内）**

1. 内核策略批准工具调用→封装 `Envelope<ToolInvocation>`；
2. 依据 `Subject/Scope/Consent` 做授权（在 `soulbase-auth`）；
3. 执行完成→生成结果 `Envelope<ToolResult>` 并记录因果关联。

------

#### **10. 开放问题（Open Issues / TODO）**

- PartitionKey 的**推荐模板**与**跨产品线最佳实践清单**（文档化）；
- 追踪字段（TraceId/SpanId）在不同技术栈（Rust/Node/Go）下的**标准映射表**；
- 多语言绑定（TypeScript/Go/Java）的 Schema 生成与**发布节奏**对齐；
- 对 Consent 的**细粒度属性**（如目的/上下文）的最小必需集合（与合规协同）。

------

> 本规约为 `sb-types` 的**唯一语义来源**。后续“第二部分：技术设计文档（TD）”将给出 Rust 结构定义、Traits（如 `Partitioned`, `Versioned`, `Auditable` 等）、Schema 生成规则、校验与契约测试基线，以及与 `soulbase-auth/interceptors/observe` 的对接位。
