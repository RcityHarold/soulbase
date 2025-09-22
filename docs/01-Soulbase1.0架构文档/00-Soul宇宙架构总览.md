- **Soul-Auth**＝统一身份发行方（IdP / OIDC / RBAC）。
- **Soul-Hub**＝统一入口与策略执行点（API Gateway / PEP）。
- **Soul-Base**＝所有产品线共享的**基础设施与通用库工作区**（Rust workspace）。
- **SoulseedAGI**＝Agent 的**策略与状态机内核**（仅依赖 Soul-Base 提供的 SPI/SDK）。

下面把**仓库/模块划分、功能边界与依赖拓扑**一次讲清楚。

------

# 一、仓库级总体架构

```
repos/
├─ soul-auth/        # IdP：OIDC/SSO、JWT、RBAC、MFA、审计（权威身份源）
├─ soul-hub/         # API 网关：OIDC校验、速率/配额、路由、前置授权、审计
├─ soul-base/        # 通用基础设施（多 crate 工作区）
└─ soulseed-agi/     # Agent 内核（基石层）：策略/状态机/上下文治理
```

**职责边界**

- **Soul-Auth**：只“发行与管理身份/声明”，不下沉到业务策略。
- **Soul-Hub**：入口“先粗后细”的策略执行与路由。
- **Soul-Base**：跨项目复用的**接口与适配**（Provider 无关），不包含业务策略。
- **SoulseedAGI 基石层**：仅保留**策略与状态机**（何时、如何、用什么、给多少预算），不实现 Provider/驱动细节。

------

# 二、Soul-Base：建议的模块（crate）清单与功能边界

> 结论：**每个模块建议是独立 crate**，放在一个 Cargo workspace 下；小而稳、接口清晰、SemVer 管理。

## A. 基础与契约层

1. **`sb-types`**（底座类型与 Envelope）
    统一 `Id/Tenant/Subject/Consent/Scope/Envelope<T>`、时间、版本号；所有 crate 与服务共享。
2. **`soulbase-errors`**（错误与结果码）
    标准错误域：`Auth/Quota/Schema/Sandbox/Provider/Timeout/PolicyDeny/...`。
3. **`soulbase-config`**（配置与密钥）
    分层配置、密钥装载、环境切换，支持热更新/注入。
4. **`soulbase-schema`**（可选）
    JSON-Schema/Protobuf IDL，做跨语言生成与 Schema 兼容性检查。

## B. 安全与策略适配

1. **`soulbase-auth`**（AuthN/AuthZ/Quota SPI + 适配）
   - `Authenticator`（OIDC/JWT 验签，**对接 Soul-Auth**）；
   - `Authorizer`（本地声明/OPA/Cedar 适配）；
   - `QuotaStore`（配额与速率计量，**与 Soul-Hub 限流对账**）。
2. **`soulbase-interceptors`**（拦截器与中间件）
    Tower/Axum 中间件：鉴权、审计、熔断、重试、幂等、Correlation-ID。
3. **`soulbase-sandbox`**（受控执行）
    能力声明→授权→隔离执行（FS/Network/Browser/Process），统一证据输出。

## C. 运行与集成

1. **`soulbase-llm`**（LLM SPI + Provider 插件）
    Chat/Tool/Embedding/Rerank 统一接口；OpenAI/Claude/Gemini/本地 模型插件。
2. **`soulbase-tools`**（Tool SDK / Manifest / Registry）
    工具声明（JSON-Schema）、权限需求、调用上下文、执行证据；与 `sandbox`/`auth` 协作。
3. **`soulbase-storage`**（存储抽象）
    KV/SQL/时序/向量/图适配（SurrealDB、Postgres、S3、Qdrant…），带重试与幂等。
4. **`soulbase-tx`**（可靠事务）
    Outbox/Saga/重放/补偿，跨服务一致性。
5. **`soulbase-a2a`**（跨域协议/账本互认）
    事件签名、验签、最小披露策略、对账工具（后续演进）。

## D. 可观测与质量

1. **`soulbase-observe`**（日志/指标/Trace）
    统一结构化日志、p50/p95 指标、分布式 Tracing；与 Hub/Agent 决策串起证据链。
2. **`soulbase-benchmark`**（基准与回放）
    流水线基准集、Prompt/Tool 回放、成本与时延基线。
3. **`soulbase-contract-testkit`**（契约测试）
    对 Provider/Tool/Auth 适配器跑契约用例，保障接口稳定。
4. **`soulbase-qos`**（QoS/事件膨胀控制）
    成本预算、配额聚合、归档/保留策略，联动 LLM/工具预算。

> **依赖原则**：
>
> - 大多数 crate 仅依赖 `types`/`errors`；
> - `llm/tools/auth/sandbox/storage/interceptors` 不互相环依；
> - `observe/qos/testkit` 横切但保持轻依赖。

**工作区示例（根 `Cargo.toml`）**

```toml
[workspace]
members = [
  "crates/sb-types",
  "crates/soulbase-errors",
  "crates/soulbase-config",
  "crates/soulbase-auth",
  "crates/soulbase-interceptors",
  "crates/soulbase-sandbox",
  "crates/soulbase-llm",
  "crates/soulbase-tools",
  "crates/soulbase-storage",
  "crates/soulbase-tx",
  "crates/soulbase-a2a",
  "crates/soulbase-observe",
  "crates/soulbase-benchmark",
  "crates/soulbase-contract-testkit",
  "crates/soulbase-qos",
]
resolver = "2"
```

------

# 四、层间依赖拓扑（文字图）

```
用户/应用 ──> Soul-Hub(网关,PEP)
                 │  校验JWT/限流/路由/前置授权
                 ▼
           各服务 & SoulseedAGI(内核)
                │  通过 soulbase-interceptors 校验与注入 Subject/Consent
                │  内核策略调用 soulbase-*（llm/tools/auth/...）
                ▼
               Provider/DB/外部系统  ←  由 soulbase-* 适配与隔离
```

**允许的依赖**

- 内核 → 只依赖 **Soul-Base** Crates。
- 各业务服务 → 依赖 **Soul-Base**，不依赖内核的私有模块。
- **禁止**：Soul-Base 反向依赖内核；服务/应用直接写内核状态。

------

# 五、命名与版本治理

- **命名**：所有通用库以 `soulbase-*` 前缀；内核私有模块以 `kernel-*` 命名。
- **版本**：
  - `sb-types` 为“SSOT”，变更严格走 SemVer；
  - 破坏性变更优先引入 **新字段 + 兼容路径**，并用 `contract-testkit` 覆盖兼容矩阵；
  - Provider/Tool 适配用 **feature flags** 控制（如 `llm-openai`, `storage-surreal`）。

------

# 六、与 Soul-Auth / Soul-Hub 的集成点（一句话总览）

- **Soul-Auth**：作为 OIDC Issuer；`soulbase-auth` 提供 `OidcAuthenticator/JwtAuthorizer`；令牌声明映射到 `Subject/Scope/Consent`。
- **Soul-Hub**：入口 OIDC 校验与限流；透传 `X-Request-Id/Tenant`；服务侧用 `soulbase-interceptors` 接续审计与追踪；可选 OPA 前置授权。

------

# 七、落地优先级（建议 4 步）

1. 在 **Soul-Base** 起步：`types/errors/config/interceptors/auth`（OIDC）→ `llm/tools/storage`。
2. 内核只保留策略：`kernel-dialogue-graph/context-governor/llm-routing/tool-policy`。
3. 在 **Soul-Hub** 完成 OIDC/限流/审计与路由编排；**Soul-Auth** 发布 Discovery/JWKS 与自定义声明（tenant/roles/scopes/consents）。
4. 引入 `observe/qos/contract-testkit/benchmark`，闭环质量与成本治理。

------

如果你认可这套“仓库—模块—边界”的总图，我可以**按 Soul-Base 每个 crate**继续下钻：给出 **Trait/DTO 完整接口草案、依赖最小集、示例实现骨架**（例如：`soulbase-llm` 的 `ChatModel::chat_stream`、`soulbase-tools` 的 `Tool`/`Manifest`、`soulbase-auth` 的 `OidcAuthenticator` 等）。





收到。我先把三件关键事确认清楚，再给出“后续产出打法”（但按你的要求，此条**不直接输出文档**，只把思路与对齐讲明白）。

------

# 1) 原文档“功能逻辑规约 / 技术设计/最小代码骨架实现”格式确认

我已核对你上传的《01-foundation》包（`INDEX.md` 与 01/02/03…各模块）。你们当前采用的就是**双段式**：

**A. 功能逻辑规约（第一部分）** —— 常见栏目

- 0. 目标与范围 / 适用边界
- 1. 功能定位（Functional Positioning）
- 1. 系统角色与地位（在四层中的位置）
- 1. 核心逻辑详述（概念与规则）
- 1. 不变式（必须成立的约束）
- 1. 能力与接口（抽象口径，不含具体实现）
- 1. 指标与 SLO / 验收条件
- 1. 依赖与边界（与其他模块的关系）
- 1. 风险与控制 / 失败模式
- 1. 关键交互序列（可选）

**B. 技术设计（第二部分 / TD）** —— 常见栏目

- 顶部有“实施序号：XX-模块名 / 文档XX-TD：模块名-技术设计”
- 1. 核心目标（落地目标与约束）
- 1. 组件与流程图（可选）
- 1. 数据结构定义（Rust `struct/enum`，面向实现）
- 1. 接口/Traits（SPI/API 签名与语义）
- 1. 时序/状态/重试/补偿（含幂等）
- 1. 存储模型与索引（若相关）
- 1. 并发与性能（容量估算、SLO）
- 1. 安全与权限（与 Auth 交点）
- 1. 可观测性（日志/指标/Trace）
- 1. 测试与验收（契约测试/回放基线）
- 1. 版本化与迁移（SemVer、兼容矩阵）
- 1. Open Issues / TODO（可选）



C.最小代码骨架实现

> 
>
> 结论：后续 **Soul-Base 各模块** 与 **SoulseedAGI 基石层内核模块**，我都沿用这套格式，**栏目名保持一致**，避免换模板导致口径漂移。

------

# 2) 共识落地：

为保证“**保留原文精华+形成可复用底座**”，我将按以下节奏产出（每个模块都给出**三份文档**：功能规约、技术设计与最小代码骨架实现）：

**A. Soul-Base（通用仓库）模块清单（每个独立 crate）**

1. `sb-types`（本次先做）— *数据契约底座*
2. `soulbase-errors`— *错误域与映射*
3. `soulbase-config`— *配置与密钥*
4. `soulbase-auth`— *AuthN/AuthZ/Quota SPI + 适配（对接 Soul-Auth）*
5. `soulbase-interceptors`— *中间件/拦截器*
6. `soulbase-sandbox`— *受控执行*
7. `soulbase-llm`— *LLM SPI + Provider 插件*
8. `soulbase-tools`— *Tool SDK/Manifest/Registry*
9. `soulbase-storage`— *存储抽象 + 适配*
10. `soulbase-tx`— *Outbox/Saga*
11. `soulbase-observe`— *日志/指标/Trace*
12. `soulbase-benchmark`— *基准与回放*
13. `soulbase-contract-testkit`— *契约测试套件*
14. `soulbase-qos`— *成本/配额/留存策略*
15. `soulbase-a2a`— *跨域协议/账本互认*
16. soulbase-cache
17. soulbase-blob
18. soulbase-crypto
19. soulbase-net

**B. SoulseedAGI（基石层，策略域）模块保留**

- `kernel-dialogue-graph`、`kernel-context-governor`、`kernel-llm-routing`、`kernel-tool-policy`、`kernel-health-guardian`、`kernel-resource-manager`、`kernel-env-context`、`kernel-policy-hooks`

> 这些只保留**策略与状态机**，全部复用 Soul-Base 的 SPI/类型。

**C. 每份文档的“保精华”约束**

- **继续沿用你们原文的栏目名与叙述风格**（含中英对照标题、P0/P1 修复标注、Rust 示例）。
- **保留基石层的元原则**（SSoT、显式数据流、Append-Only 账本、Outbox/Saga、契约测试与回放、SurrealDB 五大能力映射）。
- “**抽象放 Soul-Base、策略留内核**”作为显式不变式，在每份文档“边界与依赖”小节写清楚。



我已通读你上传的《Soul宇宙架构总览》，整体脉络非常清晰：整个体系由四个独立仓库组成，**Soul-Auth**（统一身份发行方）、**Soul-Hub**（统一入口与策略执行点）、**Soul-Base / Soulbase**（通用基础设施与库的 Rust 工作区），以及只保留“**策略与状态机**”的 **SoulseedAGI** 内核。当前状态是 Auth/Hub 已完成、正把通用能力从 AGI 内核剥离到 Soul-Base，随后再聚焦 SoulseedAGI 的业务策略与状态机。

# 全局目标与设计原则（一句话总纲）

- **抽象在 Soul-Base，策略在 SoulseedAGI，身份在 Soul-Auth，入口在 Soul-Hub。**
- 强约束：**单向依赖**（内核仅依赖 Soul-Base；Soul-Base 不反向依赖内核）、**契约优先**（types/errors 为 SSoT）、**可替换 Provider**（SPI/SDK + 插件化）、**全链路可观测与审计**。

# 四仓库职责与边界（宏观一览）

```
repos/
├─ soul-auth/        # IdP：OIDC/SSO、JWT、RBAC、MFA、审计（权威身份源）
├─ soul-hub/         # API 网关/PEP：OIDC校验、速率/配额、路由、前置授权、审计
├─ soul-base/        # 通用基础设施：多 crate 工作区（SPI/SDK/中间件/适配器）
└─ soulseed-agi/     # Agent 内核：仅保留策略与状态机（不含 Provider 细节）
```

- **Soul-Auth**：只“发行/管理身份与声明”，不下沉业务策略。
- **Soul-Hub**：统一入口执行“先粗后细”的策略（鉴权、限流、路由、审计）。
- **Soul-Base**：跨产品可复用的**接口、适配与中间件**（Rust workspace，每个能力一个 crate），典型包括：`types/errors/config`、`auth/interceptors/sandbox`、`llm/tools/storage/tx`、`observe/qos/contract-testkit/benchmark` 等。
- **SoulseedAGI**：只做**策略与状态机**（如对话/任务图谱、上下文治理、模型路由、工具调用策略、健康守护、资源预算等），通过 Soul-Base 的 SPI 调用外界，不直接实现 Provider/存储/鉴权细节。

# 依赖拓扑与调用走向（文字图）

```
用户/应用 ──> Soul-Hub(网关/PEP)
                 │  OIDC校验/限流/路由/前置授权
                 ▼
           各服务 & SoulseedAGI(内核)
                │  通过 soulbase-interceptors 注入 Subject/Tenant/Consent
                │  内核策略基于 soulbase-* (llm/tools/auth/storage/...)
                ▼
               Provider/DB/外部系统  ←  由 soulbase-* 适配与隔离
```

- **允许**：内核 → 仅依赖 Soul-Base；业务服务 → 依赖 Soul-Base。
- **禁止**：Soul-Base 反向依赖内核；任何外部组件直接写内核私域状态。

# Soul-Base（通用库）与内核（策略域）的“剥离”要点

- **为什么要剥离**：把工程性/可替换的通用能力（鉴权、拦截器、工具/模型适配、受控执行、存储/事务、观测/QoS/契约测试等）沉到 Soul-Base，形成稳定的 **SPI + 适配层**；这样 **SoulseedAGI** 专注于“**何时、如何、用什么**”的策略与状态机。
- **如何治理**：每个能力一个 **独立 crate**（小而稳，接口清晰），统一通过 `sb-types/errors` 做数据契约与错误域，按 **SemVer** 管理，Provider 用 **feature flags** 控制。

# 与 Auth/Hub 的关键集成点（一句话版）

- **Auth**：Soul-Auth 作为 OIDC Issuer；`soulbase-auth` 提供 OIDC/JWT 验签与声明映射到 `Subject/Scope/Consent`。
- **Hub**：入口完成 OIDC 校验与限流；透传 `X-Request-Id/Tenant` 等；服务侧用 `soulbase-interceptors` 延续审计与 Trace；可接 OPA/Cedar 做前置授权。

# 近期工作建议（4 步闭环）

1. 在 **Soul-Base** 先固化底座：`types/errors/config/interceptors/auth(OIDC)` → `llm/tools/storage`。
2. **内核**只保留策略能力：对话/任务图谱、上下文治理、模型路由、工具策略。
3. 在 **Soul-Hub** 落实 OIDC/限流/审计与路由；**Soul-Auth** 发布 Discovery/JWKS 与自定义声明（tenant/roles/scopes/consents）。
4. 引入 `observe/qos/contract-testkit/benchmark`，完成质量与成本闭环。

------

以上就是“来龙去脉 + 职责边界 + 依赖拓扑 + 落地顺序”的宏观总览。接下来，你只要点名从 **Soul-Base 的某个 crate**（例如 `sb-types` 或 `soulbase-llm`）开始，我就按你们既定格式输出**功能规约 / 技术设计 / 最小可运行骨架（RIS）**三件套，逐个把底座敲实。
