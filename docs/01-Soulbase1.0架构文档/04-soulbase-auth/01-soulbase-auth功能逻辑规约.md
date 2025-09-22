### **文档 SB-04：soulbase-auth（认证/授权/配额 SPI / AuthN · AuthZ · Quota）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：为 Soul 全生态（SoulseedAGI、Soul-Auth、Soul-Hub、SoulBrowser、AI 记忆等）提供**统一的认证（AuthN）/ 授权（AuthZ）/ 配额（Quota）抽象层**与稳定语义，确保：
  1. **身份来源统一**（以 **Soul-Auth** 为唯一发行方/权威目录），
  2. **策略可插拔**（OPA/Cedar 等 PDP）、
  3. **决策可审计**（证据链 + Envelope）、
  4. **默认最小权限**（deny-by-default）、
  5. **跨产品一致的配额/预算**治理。
- **范围**：定义 **SPI 接口与语义**（Authenticator / Authorizer / QuotaStore / ConsentVerifier / DecisionCache / AttributeProvider）、**数据契约**（Subject/Scope/Consent/Decision/Evidence）、**决策标签**（obligations/retryable/severity），以及与 **Soul-Auth / Soul-Hub** 的协同口径。
- **非目标**：不负责**身份发行**（属 **Soul-Auth**）、不代替**入口网关 PEP**（属 **Soul-Hub**）、不内置具体 PDP/目录/配额后端（以适配器方式接入）。

**溯源**：本模块由原“身份与权限体系（Identity & Permission System）”与“统一拦截器链/工具生态规约/LLM 服务接口”等文档中**通用鉴权抽象**与**证据口径**下沉归并而成，语义与不变式与原文**同频**。

------

#### **1. 功能定位（Functional Positioning）**

- **SSoT on Access**：访问控制的统一真相源（统一 Subject/Scope/Consent/Decision/Evidence 语义）。
- **PEP 内嵌库**：为各服务/内核提供本地 **PEP 客户端**（网关前置粗粒度 + 服务内细粒度决策）。
- **策略桥接层**：屏蔽 PDP 异构（OPA/Cedar/自研），统一**授权请求模型**与**可机读的决策结果**。
- **成本/配额协同**：与 **soulbase-qos** 协同，将**授权结果**与**预算检查**统一收敛到一次决策。

------

#### **2. 系统角色与地位（System Role & Position）**

- 所处层级：**基石层 / Soul-Base**；被所有上层服务与内核直接依赖。
- 关系：
  - **Soul-Auth**（IdP/目录）：发行 OIDC/JWT、目录/RBAC、同意凭据；本模块**消费并验证**其令牌与声明。
  - **Soul-Hub**（API 网关/PEP）：入口 **粗粒度**校验（JWT/限流/基础策略）；本模块在服务内执行**细粒度**决策。
  - **sb-types**：复用 Subject/Scope/Consent/Envelope。
  - **soulbase-interceptors**：把请求上下文映射为决策输入，并把决策结果/证据挂入 Envelope。
  - **soulseedAGI 内核**：在工具/模型调用前通过 Authorizer 做**工具级授权**与**预算检查**。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

- **Subject**（来自 `sb-types`）：`kind ∈ {User, Service, Agent}` + `tenant` + `claims`。
- **Resource URN**：`soul:{domain}:{name}`，如 `soul:tool:browser`、`soul:model:gpt-4o`、`soul:storage:kv`。
- **Action**：`read|write|invoke|admin|list|configure` 等有限集合（可扩展）。
- **Scope**：`{resource, action, attrs?}` —— **最小权限原子**。
- **Consent**：高风险操作的**明示同意**，含 `scopes[]`、`expires_at`、`purpose`。
- **Policy**：RBAC/ABAC/规则集合（驻留于 PDP 或 Soul-Auth 目录）。
- **Decision（授权结果）**：
  - `allow: bool`、`reason: string?`、`obligations: []`（如 mask/redact/watermark）、
  - `evidence: json`（命中规则、输入快照指纹、版本/策略集哈希）、
  - `cache_ttl_ms`（允许的本地短时缓存）。
- **Quota**：按 `tenant|subject|resource|action` 维度的**预算/速率**（单位：`tokens|calls|bytes|ops`）。
- **DecisionKey**：`(tenant, subject_id, resource, action, attrs_hash)`。

------

#### **4. 不变式（Invariants）**

1. **Deny-by-Default**：未显式允许即拒绝。
2. **最小必要披露**：决策对外仅暴露 `allow/obligations` 与必要 code；**证据完整**但仅进入审计。
3. **单向映射**：令牌→Subject/Scope/Consent 的映射**稳定且可回放**（随 Envelope 记录）。
4. **强一致身份来源**：所有可写路径必须使用 **Soul-Auth** 发行的凭据或其代理链（mTLS/STS）。
5. **决策可审计**：每次授权产生**证据**（策略版本、命中规则、输入摘要、决定时间），可通过事件回放。
6. **可缓存但可撤销**：本地决策缓存遵循 `cache_ttl_ms` 且支持**撤销/黑名单**（revocation）。
7. **预算原子性**：授权与配额消耗对**同一 DecisionKey** 原子化（成功即扣，失败必不扣）。
8. **环境一致**：`tenant`/`aud` 与请求目标一致；跨租户资源访问须显式跨域策略与证据。

------

#### **5. 能力与抽象接口（Abilities & Abstract Interfaces）**

> 具体接口在 TD/RIS 落地，这里定义能力与行为口径。

- **认证（AuthN）**：
  - **OIDC/JWT 验签**（对接 Soul-Auth `issuer/jwks`）；
  - **API Key** / **mTLS 证书** / **服务间 Token**（可选）；
  - 产出 **Subject**，并按映射规则挂 `claims/tenant/roles/permissions/consents`。
- **授权（AuthZ / PDP 适配）**：
  - 输入：`subject, resource, action, attrs, consent?, env`；
  - 输出：**Decision**（allow/obligations/evidence/cache_ttl）；
  - 适配：**本地规则** / **远程 PDP（OPA/Cedar/自研）**。
- **配额与预算（Quota/QoS）**：
  - 统一 `check_and_consume(key, cost)`：按租户/主体/资源/动作扣配额或拒绝；
  - 与 `soulbase-qos` 共享预算视图，支持**软/硬限额**与**成本标签**（LLM token、工具调用）。
- **同意验证（ConsentVerifier）**：
  - 校验 `Consent` 结构、有效期、范围与操作一致性；必要时回源 **Soul-Auth** 做二次确认。
- **属性与上下文（AttributeProvider）**：
  - 拉取额外**环境属性**（地理/设备/风险评分）与**资源属性**（所有者/机密级别）。
- **决策缓存（DecisionCache）**：
  - 结合 `Decision.cache_ttl_ms` 与撤销列表；本地或分布式缓存（可选）。
- **审计与事件**：
  - 生成 `Envelope<AuthDecisionEvent|AuthDenyEvent|QuotaEvent>`（仅摘要），进入观测与回放。
- **跨协议映射**：
  - 与 `soulbase-errors` 对齐：`AUTH.UNAUTHENTICATED`、`AUTH.FORBIDDEN`、`QUOTA.RATE_LIMITED` 等稳定码；
  - 对 **Soul-Hub**：提供**建议的入口状态码/头部**（如 `WWW-Authenticate`、`Retry-After`）。

------

#### **6. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **认证延迟**：本地 JWT 验签 p95 **≤ 2ms**；带远程 introspect p95 **≤ 20ms**。
- **授权延迟**：本地规则 p95 **≤ 3ms**；远程 PDP p95 **≤ 25ms**（含网络）。
- **决策命中缓存比**：≥ **85%**（常规流量）；缓存过期/撤销正确率 **100%**。
- **配额检查**：原子扣减成功率 **≥ 99.99%**；双写/对账偏差 **= 0**。
- **错误映射一致性**：契约测试 100% 通过；`UNKNOWN.*` 占比 **≤ 0.1%**。
- **验收**：
  - 黑盒：标准用例（已授权/未授权/过期/撤销/越权/预算不足）全通过；
  - 回放：`AuthDecisionEvent` 链路可复现行为；
  - 压测：在目标 QPS 下满足延迟与正确率。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：**Soul-Auth**（Issuer/目录/同意/角色）、远程 PDP（OPA/Cedar）、配额后端（Redis/DB）。
- **下游**：所有服务/内核；**Soul-Hub** 在入口前置粗粒度策略（本模块在服务内细粒度收口）。
- **边界**：
  - 不存储长期用户目录（读取 **Soul-Auth**）；
  - 不定义业务域资源（仅资源命名规范与 attrs 容器）；
  - 不负责入口路由/限流（由 **Soul-Hub** 执行，配额语义在本模块统一）。

------

#### **8. 风险与控制（Risks & Controls）**

- **JWK 轮换不及时** → 定时刷新 + 双缓存 + 失败回退旧钥；
- **本地缓存导致越权** → TTL 严格、撤销通道（push/poll）、高风险操作**强制直连 PDP**；
- **策略漂移（多 PDP/多租户）** → 策略版本/哈希入证据；契约测试守护；
- **同意缺失或伪造** → Consent 验签 + 范围匹配 + 高风险操作二次确认；
- **配额与授权割裂** → 合并为单次决策流程（允许/扣额/拒绝三态）；
- **跨租户污染** → 决策与配额键强制包含 `tenant`；入口与服务双校验。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 入站请求（服务内细粒度）**

1. **Soul-Hub** 完成 OIDC/限流 → 透传头；
2. **soulbase-interceptors** 验签 → 生成 **Subject** → 构造授权请求 `(subject, resource, action, attrs)`；
3. **Authorizer** 决策：若 `allow=true` →（可选）**QuotaStore** 扣额 → 放行；否则返回 `AUTH.FORBIDDEN/QUOTA.*`。
4. 生成 `AuthDecisionEvent`（含证据摘要）并入观测。

**9.2 工具调用（Agent 内核）**

1. 内核提出调用 `tool=browser`；
2. **Authorizer** 校验 `scope(resource="soul:tool:browser", action="invoke")` 与 `Consent`；
3. 高风险 `safety=High`：要求有效 `consent`，否则 `POLICY.DENY_TOOL`；
4. 扣预算（调用次数/带宽）→ 成功执行。

**9.3 LLM 推理**

1. 内核路由模型 `soul:model:gpt-4o`；
2. **Authorizer** 校验 `action="invoke"` + **Quota** 检查 tokens 预算；
3. 扣费并记录 `QosBudget`；超限 → `QUOTA.BUDGET_EXCEEDED`。

**9.4 撤销与缓存失效**

1. Soul-Auth 更新/撤销权限 → 通知/轮询 → **DecisionCache** 失效；
2. 高风险资源强制直连 PDP，绕过缓存。

------

#### **10. 开放问题（Open Issues / TODO）**

- **Policy 模型对齐**：是否在平台级统一细分域（Tool/Model/Storage）的**属性字典**与**常用规则库**。
- **Obligations 执行**：如 `mask/redact/watermark` 的**执行落点**（PEP vs 业务）与**验收口径**。
- **多地域/边缘节点**：JWK/策略/撤销的近端分发与一致性保障。
- **细粒度预算单位**：更多统一度量（GPU 秒/IOps/网流量）与跨组件核算协同。
- **人机协同授权**：对特定高风险调用引入**人审**流程的最小闭环（待与内核/Hub 共同定义）。

------

> 本规约确立了 `soulbase-auth` 的**语义边界、能力清单与不变式**，与原始文档的“身份与权限体系”**同频共振**并增强了策略/证据/预算一体化的工程闭环。若确认无误，我将按“三件套”流程继续输出 **SB-04-TD（技术设计）**。
