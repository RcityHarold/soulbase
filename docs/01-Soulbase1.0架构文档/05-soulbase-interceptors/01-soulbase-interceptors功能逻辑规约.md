### **文档 SB-05：soulbase-interceptors（统一拦截器链 / 中间件）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：为 Soul 生态的所有服务（含 SoulseedAGI 内核暴露的 API）提供一套**可组合、协议无关**的“统一拦截器链”，在**不侵入业务**的前提下落地横切能力：
   **上下文初始化 → 关联/追踪 → 配置戳记 → 认证映射 → 多租户防护 → 细粒度授权+配额 → 结构校验 → 熔断/超时/重试 → 审计/规范化错误 → 义务执行（obligations） → 统一响应头。**
- **范围**：规范 **链路阶段、标准头、环境上下文、与其他基石模块的对接契约**；覆盖 **HTTP/gRPC/消息队列** 三类入口模型；提供**路由到资源/动作**的映射口径与“最小必要披露”。
- **非目标**：不替代网关（Soul-Hub）的入口 PEP、限流与路由；不内嵌策略/目录/预算实现（这些分别由 `soulbase-auth` / `soulbase-qos` / Soul-Auth 提供）。拦截器仅**编排调用**并**落实不变式**。

------

#### **1. 功能定位（Functional Positioning）**

- **PEP in-process（进程内策略执行点）**：与 Soul-Hub 的“入口粗粒度 PEP”形成**双层防护**，在服务内落实**细粒度授权与预算**。
- **SSoT on Enveloping（封装真相源）**：以 `sb-types::Envelope<T>` 为统一承载，对**来向、主体、因果/关联、快照版本**进行标准化封装与审计。
- **Error Normalizer（错误规范化）**：把异常统一映射为 `soulbase-errors` 的**稳定错误码**与**公共/审计视图**。
- **Obligations Enforcer（义务执行器）**：落实授权决策中的 `obligations`（如脱敏、打水印、响应裁剪）。

------

#### **2. 系统角色与地位（System Role & Position）**

- 所处层级：**基石层 / Soul-Base**，所有 API/消息入口**默认启用**。
- 关联模块：
  - `sb-types`：构建/传递 `Envelope`、注入 `TraceContext`；
  - `soulbase-config`：读取**当前快照版本/校验和**，透出响应头；
  - `soulbase-auth`：认证映射（Subject）/ 细粒度授权（Authorizer）/ 配额（Quota）/ 同意验证（Consent）；
  - `soulbase-errors`：错误码与映射；
  - `soulbase-observe`：结构化日志/指标/Trace 的标签口径；
  - Soul-Hub：前置 OIDC 校验、限流、路由与基础审计，拦截器链在服务内**延续与细化**。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

- **Interceptor Stage（阶段）**：`ContextInit → ConfigStamp → AuthNMap → TenantGuard → AuthZQuota → SchemaGuard → Resilience → Audit/Errors → Obligations → ResponseStamp`。
- **Route Policy（路由策略）**：将“协议路由（path/method/topic）”映射为 `ResourceUrn` + `Action` + 可选 `Attrs` 模板。
- **Standard Headers（标准头）**：
  - 入站读取：`Authorization`、`X-Request-Id`、`X-Trace-Id` / `traceparent`、`X-Soul-Tenant`、`X-Consent-Token`、`Idempotency-Key`；
  - 出站写入：`X-Request-Id`、`X-Trace-Id`、`X-Config-Version`、`X-Config-Checksum`、`X-Obligations`（仅标识）、（必要时）`Retry-After`。
- **Envelope Binding**：在进入业务处理前构造 `Envelope<Incoming>`；在出站路径构造 `Envelope<Outgoing>`，形成**可回放**证据链。
- **Obligations**：授权返回的**义务集合**，最常见有：`mask`（字段脱敏）、`redact`（字段删除）、`watermark`（响应添加水印标记）。

------

#### **4. 不变式（Invariants）**

1. **零信任默认拒绝**：路由未声明 `ResourceUrn/Action` → 默认禁止通过细粒度授权阶段（日志告警）。
2. **单一真相源**：所有可审计的入站调用**必须**绑定 `Envelope`，并带 `partition_key`、`causation_id/correlation_id`（可派生）。
3. **租户一致**：`X-Soul-Tenant` 与 `Subject.tenant` **必须一致**；不一致直接拒绝。
4. **最小必要披露**：对外仅暴露 `soulbase-errors` 公共视图与必要响应头；敏感信息仅进入审计视图。
5. **义务优先**：有 `obligations` 时，响应体**先执行义务**再返回；无法满足义务→拒绝并记录。
6. **Config 可追溯**：每个响应都带 `X-Config-Version/Checksum`；热更期间两者**一致且可回滚**。
7. **幂等保护（可选）**：收到 `Idempotency-Key` 的写操作，按声明窗口**去重**；冲突返回缓存结果。
8. **协议无关**：HTTP/gRPC/消息入口共享**同一语义**；差异仅在适配层。

------

#### **5. 能力与抽象接口（Abilities & Abstract Interfaces）**

> 下列能力在技术设计与 RIS 中分别落为 **Stage 适配点**，可按需启用/组合。

- **ContextInit**：
  - 生成或采纳 `X-Request-Id`、`TraceContext`；派生 `correlation_id`；
  - 读取 `soulbase-config` 的快照 `version/checksum`。
- **ConfigStamp**：
  - 将 `X-Config-Version`/`X-Config-Checksum` 注入响应头；出现热更→以**快照读视图**保证一致。
- **AuthNMap**：
  - 解析 `Authorization`（或来自网关的用户态头部），通过 `soulbase-auth::Authenticator` 产出 `Subject`；
  - 映射 `X-Soul-Tenant` 与 `Subject.tenant` 校验。
- **Route Policy 解析**：
  - 将 `path + method`（HTTP）/ `service + method`（gRPC）/ `topic`（MQ） 映射到 `ResourceUrn`、`Action`、`attrs` 模板（可引用路径变量/查询）。“未命中→拒绝”。
- **AuthZQuota（授权与配额）**：
  - 调用 `soulbase-auth::Authorizer` 与 `QuotaStore` 完成**一次性**决策（含 `consent` 与预算扣减）。
- **SchemaGuard（结构校验）**：
  - 对接服务侧声明的请求/响应 Schema（JSON Schema/Protobuf 描述）；违规→`SCHEMA.VALIDATION_FAILED`。
- **Resilience（韧性护栏）**：
  - 对调用处理函数设置**超时/熔断/重试/限并发**（与 Hub 的前置限流互补）。
- **Audit/Errors（审计与错误规范化）**：
  - 将错误统一转换为 `soulbase-errors` 稳定码；产生 `Envelope<AuditEvent>`；
  - 在日志/Trace 标签中输出 `code/kind/retryable/severity/resource/action/tenant`。
- **Obligations**：
  - 按 `Decision.obligations` 对响应进行脱敏/裁剪/水印；无法满足时**拒绝**并记录。
- **ResponseStamp**：
  - 统一写入标准响应头与缓存建议；在必要时设置 `Retry-After`。

------

#### **6. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **开销**：拦截器链自身 p95 **≤ 2ms**（不含远程 PDP/Quota）；
- **一致性**：所有非 2xx 的对外错误响应**100%** 包含稳定错误码；
- **租户防护**：租户不一致请求**100%** 拒绝；
- **义务执行**：带 `obligations` 的响应**100%** 执行相应裁剪/脱敏；
- **可追溯**：响应头 `X-Request-Id`/`X-Config-*` **100%** 存在；
- **验收**：
  - 契约测试：路由策略→资源动作映射正确；
  - 黑盒：未声明路由拒绝、越权拒绝、预算不足拒绝、结构校验失败 → 稳定码正确；
  - 回放：`Envelope` 序列可复现一次请求的**授权证据与义务执行**。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：来自 Soul-Hub 的头部（OIDC 校验已做/未做皆可）、Soul-Auth 的发行与撤销、远程 PDP/Quota。
- **下游**：业务 Handler / 内核 API；`soulbase-observe` 采集日志/指标。
- **边界**：
  - 不实现目录/策略/配额本身；
  - 不直接写持久化（审计事件由上游观察面消费）；
  - 不负责业务字段级 Schema 的声明（仅消费服务提供的 Schema）。

------

#### **8. 风险与控制（Risks & Controls）**

- **策略遗漏/路由未声明** → 默认拒绝 + 告警；
- **租户污染**（Header 与 Subject 不一致）→ 强制拒绝 + 审计；
- **错误泄密** → 只返回公共视图，`message_dev/causes/meta` 仅入审计；
- **义务无法满足**（字段不存在/类型不符）→ 拒绝；
- **PDP/Quota 慢或不可用** → 超时/熔断/降级（只读路由可选放行策略需显式配置，默认拒绝）；
- **幂等键滥用** → 限制窗口与结果体大小、附加签名/租户绑定。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 HTTP 请求（细粒度授权）**

1. `ContextInit`：采纳/生成 `X-Request-Id`、注入 `TraceContext`；
2. `ConfigStamp`：读取快照 `version/checksum`；
3. `AuthNMap`：Bearer→`Subject`（或消费 Hub 透传的用户态头）；
4. `TenantGuard`：`X-Soul-Tenant` 与 `Subject.tenant` 一致性校验；
5. `RoutePolicy`：`GET /v1/memory/items` → `soul:storage:kv` + `Read`；
6. `AuthZQuota`：调用 `Authorizer/QuotaStore` 完成决策；
7. `SchemaGuard`：请求体 Schema 校验；
8. 业务处理；
9. `Obligations`：对响应按需脱敏/裁剪；
10. `ResponseStamp`：写标准头；`Audit/Errors`：写入事件并对异常做码表规范化。

**9.2 消息入口（异步任务）**

1. 从消息元信息解析 Subject/租户与 `correlation_id`；
2. 套用 Topic→资源/动作映射；
3. 授权+配额→通过后处理；
4. 失败统一 NACK 标签与重试策略建议（交由队列层实现）。

**9.3 幂等写操作（可选）**

1. 读 `Idempotency-Key`；
2. 查询去重存储（内存/Redis）；命中→直接返回缓存结果；
3. 未命中→执行业务→缓存结果（带 TTL/租户键）→返回。

------

#### **10. 标准路由→资源/动作映射规则（建议）**

- **HTTP**：
  - `GET/HEAD` → `Action::Read`
  - `POST/PUT/PATCH` → `Action::Write`（如需“调用”，由路由显式声明 `Invoke`）
  - `DELETE` → `Action::Write`（或 `Admin`，按路由策略）
- **gRPC**：以 `service.method` 映射资源与动作（服务注册时提供策略表）。
- **MQ**：以 `topic`/`queue` 名称映射，支持通配与前缀策略。

> 以上为**默认规则**；任何偏离须在路由策略表中**显式声明**。

------

#### **11. 标准头与字段（建议最小集）**

- **入站读取**：
   `Authorization` | `X-Request-Id` | `traceparent`/`X-Trace-Id` | `X-Soul-Tenant` | `X-Consent-Token` | `Idempotency-Key`
- **出站写入**：
   `X-Request-Id` | `X-Trace-Id` | `X-Config-Version` | `X-Config-Checksum` | `X-Obligations`（如 `"mask,watermark"`） | `Retry-After`（当 `RateLimited` 时）

------

#### **12. 指令与开关（仅结构，策略在服务/配置层）**

- **Allow-Read-On-PDP-Down**（bool，默认 false）：PDP 不可用时对**只读**路由的降级放行。
- **Idempotency-Window**（秒）：幂等窗口大小。
- **Obligation-Strict**（bool，默认 true）：义务失败是否阻止响应。
- **Max-Body-For-Cache**（字节）：幂等缓存结果体上限。
- **Resilience**：超时、最大并发、重试次数/间隔、熔断阈值。

------

#### **13. 开放问题（Open Issues / TODO）**

- `Obligations` 与具体数据结构（JSON vs Protobuf）的**跨协议执行**细节标准化；
- 幂等缓存与**隐私数据**的安全隔离策略（加密/脱敏存储）；
- 消息入口（MQ）下 `Envelope` 的**因果/关联**派生规则与重试可观测统一模型；
- 与 `soulbase-a2a` 的跨域审计头对齐（签名/账本指纹）是否纳入默认响应头位。

------

> 本规约与原始“统一拦截器链/横切能力”文档**同频共振**：以 **Envelope + 稳定错误码 + 双层 PEP + 最小披露 + 回放可审计** 为不变式，进一步将“路由→资源/动作”与“义务执行/幂等/快照戳记”纳入统一口径。若你认可，我们将按“三件套”流程继续输出 **SB-05-TD（技术设计）**。
