### **文档 SB-08：soulbase-tools（Tool SDK / Manifest / Registry / Invocation Protocol）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：为 Soul 生态提供**统一的工具（Tool）规范与调用协议**，覆盖**声明（Manifest）→ 注册（Registry）→ 发现（Discovery）→ 预检（Preflight）→ 受控执行（via Sandbox）→ 证据闭环（Evidence）→ 结果返回**的全链路；以**Schema-first**、**最小权限**、**默认拒绝**为原则，确保跨产品线一致的安全与可观测语义。
- **范围**：
  1. **Tool Manifest**：输入/输出 Schema、权限需求（Scopes）、能力映射（Capabilities）、副作用（SideEffect）、风险级别（SafetyClass）、超时/并发/限额、同意（Consent）需求等；
  2. **Tool Registry**：多租户注册、启停、版本/能力一致性校验与发现；
  3. **Invocation Protocol**：调用上下文、幂等键、预检（AuthZ/Quota/Policy）、证据记录、错误规范化；
  4. **与基座协同**：与 `soulbase-auth`/`-sandbox`/`-interceptors`/`-observe`/`-errors`/`-qos`/`-config` 对齐的接口与不变式。
- **非目标**：本模块**不直接执行**副作用（由 `soulbase-sandbox` 执行），**不**做 LLM 提案（提案来自 `soulbase-llm`），**不**承载业务策略（仅暴露策略钩子与显式配置）。

------

#### **1. 功能定位（Functional Positioning）**

- **SDK + 契约**：为工具开发者提供统一**SDK 与声明契约**，避免各处重复造轮子；
- **平台内“工具操作系统”**：通过 Manifest/Registry/Protocol，把工具纳入平台的**认证/授权/预算/证据**治理闭环；
- **与 LLM/Agent 的桥**：LLM 仅能见到**受限工具清单**并产生**ToolCall 提案**；是否执行与如何执行由**策略 + Sandbox**裁决。

------

#### **2. 系统角色与地位（System Role & Position）**

- 所处层级：**基石层 / Soul-Base**，被 SoulseedAGI 内核与所有应用直接依赖。
- 关系：
  - **soulbase-llm**：读取受限的 `tool_specs` 形成**提案**（不执行）；
  - **soulbase-auth**：判定 `resource="soul:tool:{tool_id}" action="invoke"` 的授权与配额；
  - **soulbase-sandbox**：根据 Manifest → Capability/Profile 执行；
  - **soulbase-interceptors**：绑定 Envelope、标准头、错误公共视图；
  - **soulbase-config**：白名单/限额/模型兼容/开关的热更；
  - **soulbase-qos**：扣减调用/字节/CPU 等预算；
  - **soulbase-observe**：指标/日志/Trace 与证据事件对齐；
  - **soulbase-errors**：错误稳定码与跨协议映射。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

**3.1 ToolId / 命名空间**

- **ToolId** 唯一：`<group>.<pkg>.<name>`，对外资源 URN：`soul:tool:<group.pkg.name>`。
- **多租户**：注册时可选择**租户级实例化**（不同租户的配置/白名单不同）。

**3.2 Tool Manifest（声明）**（Schema-first）

- 元信息：`id/name/version/display_name/description/tags`
- **IO 契约**：`input_schema`（JSON Schema）/ `output_schema`（JSON Schema）
- **权限需求**（Scopes）：`[{resource:"soul:fs:/...", action:"read"}, ...]`
- **能力映射**（Capabilities）：与 Sandbox 的 `fs/net/browser/proc/tmp` 对应（最小集）
- **副作用**（SideEffect）：`None|Read|Write|Network|Filesystem|Browser|Process`
- **风险级别**（SafetyClass）：`Low|Medium|High`（默认 Low）
- **同意要求**（ConsentPolicy）：是否需要明示同意、有效期/范围
- 执行约束：`timeout_ms/limits(max_bytes_in/out, files, depth, concurrency)`
- 并发与幂等：`concurrency="serial|parallel"`, `idempotency="keyed|none"`
- 兼容性：`provider_compat`（对 LLM/模型或平台版本的兼容矩阵）
- 版本化：遵循 **SemVer**，破坏性变更提升 MAJOR；`deprecations[]` 标注迁移建议

**3.3 Tool Registry（注册/发现）**

- **状态**：`registered | enabled | paused | deprecated`
- **索引**：按租户 / 分组 / 标签 / 风险级别 / 副作用 / 能力域可查询
- **一致性**：Manifest 与运行期策略（Config/QoS/Auth）合成**可用视图**（AvailableSpec）

**3.4 ToolCall（调用）**

- `call_id`（对齐 LLM 提案或外部调用生成）
- `actor/tenant`、`origin`（`llm|api|system`）、`args`（按 `input_schema` 验证）
- **预检结果**：`preflight{authz:true/false, budget, profile_hash}`
- **结果**：`status(ok|denied|error)`, `error_code`, `output`（按 `output_schema` 验证）, `evidence_ref`

------

#### **4. 不变式（Invariants）**

1. **默认拒绝（deny-by-default）**：无授权或超出 Manifest/策略 → 拒绝；
2. **Schema-first**：所有输入/输出都必须经 JSON Schema 校验，失败 → `SCHEMA.VALIDATION_FAILED`；
3. **只经 Sandbox 执行副作用**：任何具副作用的工具**一律走 `soulbase-sandbox`**；
4. **证据双事件**：Begin/End 两条 Evidence，失败也必须产出；
5. **权限最小化**：实际执行能力 = **Grant ∩ Manifest ∩ PolicyConfig**；
6. **同意强约束**：`SafetyClass=High` 或 `SideEffect ∈ {Write,Process}` 必须有有效 `Consent`；
7. **幂等明确**：幂等工具必须支持 `Idempotency-Key`，非幂等禁止自动重试；
8. **最小披露**：对外只返回公共视图；敏感信息/证据摘要写入审计；
9. **多租户隔离**：`tenant` 必须贯穿授权、预算、路径映射/白名单与证据。

------

#### **5. 能力与接口（Abilities & Abstract Interfaces）**

> 以下为**抽象能力**；具体 Traits/方法在 SB-08-TD/RIS 中落地。

- **声明注册（Register）**
  - 注册/更新/启停 Tool Manifest；做 Schema/权限/能力一致性校验；
  - 生成/更新租户级可用视图（AvailableSpec），记录版本与策略哈希。
- **发现与过滤（Discover）**
  - 支持按租户/标签/风险/副作用/能力域/关键字过滤；
  - 为 LLM 生成**受限 ToolSpec 列表**（只含名称 + 输入 Schema 摘要，**不**包含权限与同意策略细节）。
- **预检（Preflight）**
  - 输入：`actor/tenant/tool_id/args/consent?`；
  - 步骤：`input_schema` 校验 → `soulbase-auth` 授权与配额 → 与 `soulbase-config` 合成 Profile → `soulbase-sandbox` Guard 校验；
  - 输出：`allow/deny`、`profile_hash`、`budget_snapshot`、`obligations`（如 mask/redact）。
- **受控执行（Invoke via Sandbox）**
  - 通过 `soulbase-sandbox` 执行 `ExecOp`（由 Manifest → Capability → Profile 映射生成）；
  - 记录 `EvidenceBegin/End` 与预算消耗；
  - 对 `output` 做 `output_schema` 校验与**结构化修复（可选）**。
- **错误规范化（Normalize Errors）**
  - 授权拒绝 → `AUTH.FORBIDDEN` / `POLICY.DENY_TOOL`；
  - 策略/沙箱拦截 → `SANDBOX.CAPABILITY_BLOCKED`；
  - 超时 → `PROVIDER.UNAVAILABLE` / `LLM.TIMEOUT`（依域）；
  - 预算超限 → `QUOTA.BUDGET_EXCEEDED`；
  - Schema 失败 → `SCHEMA.VALIDATION_FAILED`；
  - 未分类 → `UNKNOWN.INTERNAL`。
- **义务执行（Obligations）**
  - 按授权/策略返回的义务对 `output` 或证据摘要应用**mask/redact/watermark**；失败按策略**拒绝或降级**。
- **审计与观测（Audit/Observe）**
  - 统一生成 `Envelope<Tool*Event>`（注册/启停/调用开始/结束）；
  - 指标：调用数、拒绝数、错误码分布、p95 延迟、预算消耗（bytes/calls）。

------

#### **6. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **SLO-1（正确性）**：输入/输出 Schema 校验覆盖率 **= 100%**；
- **SLO-2（安全）**：副作用调用经 Sandbox 的比例 **= 100%**；无授权直通 **= 0**；
- **SLO-3（性能）**：预检开销 p95 **≤ 5ms**（不含 PDP/Sandbox）；
- **SLO-4（证据）**：Begin/End 证据双事件缺失 **= 0**；
- **SLO-5（错误规范化）**：`UNKNOWN.*` 占比 **≤ 0.1%**；
- **验收**：
  - 契约测试：Manifest/Schema 校验、预检→执行→证据、错误映射；
  - 回放：基于 Evidence 可重建一次调用的关键行为与结果摘要；
  - 压测：在目标 QPS 下满足 p95 预算。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：`soulbase-auth`（AuthN/Z/Quota/Consent）、`soulbase-config`（策略/白名单/映射）、`soulbase-qos`（预算）、`soulbase-llm`（提案）
- **下游**：`soulbase-sandbox`（受控执行）、`soulbase-observe`（观测）、`soulbase-interceptors`（入站/出站规范化）
- **边界**：
  - 不落持久化业务数据（仅工具注册/状态与运行元数据）；
  - 不包含具体容器/虚拟化（由 Sandbox 实现）；
  - 不做模型路由/上下文压缩（交内核）。

------

#### **8. 风险与控制（Risks & Controls）**

- **权限漂移/越权** → Manifest 与 Grant/Policy 的**三方交集**；**默认拒绝**；注册/更新走契约校验；
- **Schema 漂移** → 强制 SemVer；`MINOR` 仅可新增可选字段；CI 契约测试守护；
- **注入/命令拼接** → 由 Sandbox 的 `PolicyGuard` 在 Pre-Exec 阶段拦截；
- **大输出/泄露** → `output_schema` + 义务（mask/redact）+ 输出大小阈值；
- **幂等困境** → 统一 `Idempotency-Key`、结果缓存窗口与可重放 Evidence；
- **多租户污染** → Tool 实例与证据都带 `tenant`；路径/域名白名单也按租户分隔。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 LLM 提案 → 工具受控执行**

1. `soulbase-llm` 产生 `ToolCallProposal{name, call_id, arguments}`；
2. 内核策略筛选/排序→**预检**：`input_schema` 校验 + `soulbase-auth` 授权/配额 + `sandbox` Guard；
3. 通过→`sandbox` 执行（Begin Evidence）→ 计量/预算扣减 → 生成结果（End Evidence）；
4. `output_schema` 校验/修复 + 义务执行→返回；失败走稳定码并记录证据。

**9.2 API 直呼（无 LLM）**

1. 客户端携 `Idempotency-Key` 调用工具；
2. **拦截器链**注入 Envelope/tenant/trace→走**同样预检与执行**路径。

**9.3 注册与热更**

1. 新工具注册/版本升级→Registry 验证（Schema/权限/能力一致）→生效；
2. 策略/白名单/价表热更→新请求立即使用**合成后的可用视图**。

------

#### **10. 开放问题（Open Issues / TODO）**

- **Tool 组合/链式调用**的契约（输出→下一个输入的 Schema 合规性自动检查）与预算聚合；
- **长任务（Async Tool）**的标准协议（状态轮询/回调/Evidence 分段）；
- **多模工具**（图像/音频处理）的通用输出摘要与脱敏策略；
- **跨域（A2A）工具**的凭证传递与最小必要披露（与 `soulbase-a2a` 协调）；
- **可移植封装**：打包工具为“胶囊”（caplet），标准化分发与签名校验。

------

> 本规约与已完成模块**同频共振**：**LLM 只提案**、**Sandbox 受控执行**、**Auth/QoS 签发/扣账**、**Interceptors/Errors 规范对外**、**Observe/Evidence 可回放**。若确认无误，下一步我将输出 **SB-08-TD（技术设计）**，给出 `Tool Manifest/Registry/Invoker` 的接口与状态机、预检与执行编排、与 Sandbox/Auth/Observe 的耦合点，并在随后提供 **SB-08-RIS（最小可运行骨架）**。
