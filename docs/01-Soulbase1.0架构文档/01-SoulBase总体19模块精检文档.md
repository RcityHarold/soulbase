太好了！我已**解包并逐篇通读**你上传的《01‑Soulbase1.0架构文档.zip》，覆盖 **19 个模块（SB‑01…SB‑19）**，且每一模块均按「**功能规约 / 技术设计 / 最小可运行骨架（RIS）**」三件套输出。结合你之前的《SoulBase 项目总览（15 模块总装手册）》口径，我在此给出**整体系的“总装视角”体检报告**：先讲整体判断与端到端数据流，再给出**19 模块对接检查表（精简版）**、**关键缺陷/不清项清单**与**分阶段落地建议**。

------

## A. 总体判断（Executive Summary）

1. **方向正确、边界清晰，抽象层级统一。**
    SSoT 契约（SB‑01/02/03）、拦截面（SB‑05）、受控执行（SB‑06）、SPI（SB‑07/08/09/19）、可靠性（SB‑10）、观测（SB‑11）、门禁（SB‑12/13）贯穿始终，分层与职责符合大规模工程最佳实践。
2. **复杂度集中在 QoS/A2A/Tx，但有“可切换”策略。**
    SB‑14（配额/账页/留存）、SB‑15（跨域协议）、SB‑10（Outbox/Saga/幂等）是重模块，但你们已用 feature gates/RIS 后端分层推进，**能按阶段启停**。
3. **核心闭环已成型，仍有 3 类“跨模块缝隙”需统一：**

- **错误码与公共视图**：个别拼写与码位仍不统一（详见 §D‑1）。
- **幂等锚点**：`Envelope.envelope_id` 已在多处提及，但仍与 `Idempotency‑Key`、`(channel, seq)` 并行存在（§D‑2）。
- **Auth×QoS 职责**：入口/工具/网络侧存在**重复扣额或漏扣**的风险（§D‑3）。

> 以上三点一旦统一，端到端数据流会“更顺更稳”。

------

## B. 端到端数据流（从入口到证据/账页/回执）

**主干链路：**
 **拦截器**（SB‑05）→ **Auth（含配额前置）**（SB‑04 ⇆ SB‑14）→ **LLM 推理/提案**（SB‑07）→ **工具预检+受控执行**（SB‑08 ⇆ SB‑06）→ **存储/事务/可靠投递**（SB‑09 ⇆ SB‑10）→ **账页/结算**（SB‑14）→ **观测与证据**（SB‑11）→ **公共错误视图**（SB‑02）。
 **外呼与跨域**：经 **Net**（SB‑19）发起，串接 `Trace/UA → SandboxGuard → Auth(JWS/Bearer) → QoS‑Bytes → CacheHook/SWR`；跨域经 **A2A**（SB‑15）签名、反重放与回执；大对象经 **Blob**（SB‑17）；缓存与“单航班合并”由 **Cache**（SB‑16）承接。

**“一致性必备”**：

- 全链路**Envelope**（SB‑01）携带 `envelope_id/tenant/subject/correlation_id`；
- **Config 快照**（SB‑03）在调用开始固定 `config_version/hash`，并被 Evidence/账页/回执记录；
- **错误码**统一走 SB‑02 的公共视图；**证据/指标**统一走 SB‑11 的白名单标签。

------

## C. 19 模块 × 接口对接检查表（精简版）

> **字段含义**：
>  **职责**=模块定位；**输入**=来自哪些上游/上下文；**输出**=向哪些模块/介质交付；**关键接口**=对外 Facade/Trait；**关键依赖**=稳定耦合点。
>  （★ 为新增 16–19 四个模块）

| 模块                       | 职责                          | 输入              | 输出                | 关键接口                   | 关键依赖             |
| -------------------------- | ----------------------------- | ----------------- | ------------------- | -------------------------- | -------------------- |
| **SB‑01 types**            | **SSoT 契约**                 | —                 | Envelope/Subject 等 | 类型本身                   | 全体                 |
| **SB‑02 errors**           | **稳定错误域**                | code/kind 映射    | 公共视图            | `ErrorBuilder`             | 全体                 |
| **SB‑03 config**           | **快照/热更**                 | 多源 cfg          | `ConfigSnapshot`    | `Loader/SnapshotSwitch`    | SB‑05/06/14/19       |
| **SB‑04 auth**             | 验签/授权/同意/（前置）配额   | Token/Consent     | 决策、主体上下文    | `AuthFacade.authorize(..)` | SB‑01/02/03/14       |
| **SB‑05 interceptors**     | 入/出站拦截与统一响应         | 请求上下文        | 标准响应            | `InterceptorChain`         | SB‑01/02/03/04/11    |
| **SB‑06 sandbox**          | 受控执行/能力声明             | Manifest/Policy   | Begin/End Evidence  | `Sandbox.run(..)`          | SB‑02/03/11          |
| **SB‑07 llm**              | Provider 无关 LLM SPI         | Prompt/Policy     | 输出/结构化         | `ChatModel.*`              | SB‑03/08/11          |
| **SB‑08 tools**            | Tool SDK/注册/预检/调用       | LLM 提案/API      | 结果+证据           | `ToolRegistry/Invoker`     | SB‑04/06/07/11       |
| **SB‑09 storage**          | Repo/Graph/Vector/Migration   | 数据/证据         | 读写/迁移           | `Repository<T>/Tx`         | SB‑01/10/11          |
| **SB‑10 tx**               | Outbox/Saga/幂等              | 业务事件          | 可靠投递/补偿       | `Outbox/Orchestrator`      | SB‑01/09/11          |
| **SB‑11 observe**          | 日志/指标/Evidence            | 关键路径          | 统一观测面          | `Logger/Meter/Sink`        | 全体                 |
| **SB‑12 benchmark**        | 基准/回放/门禁                | 测试输入          | Report              | `Runner.run_suite(..)`     | SB‑07/08/11          |
| **SB‑13 contract‑testkit** | 契约测试                      | Spec/Case         | RunReport           | `Runner.run(..)`           | SB‑01/02/05..        |
| **SB‑14 qos**              | 配额/账页/留存                | 调用前后计量      | `LedgerLine`        | `QosFacade.*`              | SB‑03/11/04          |
| **SB‑15 a2a**              | 跨域通道/签名/回执            | 请求/证据         | 收据链/互认         | `A2AFacade/Inbound`        | SB‑01/02/10/11/14    |
| **★ SB‑16 cache**          | 统一缓存 & SingleFlight & SWR | `key(tenant[..])` | 命中/刷新           | `get_or_load/do_once`      | SB‑03/11/19          |
| **★ SB‑17 blob**           | 对象存储/Presign/Retention    | bytes/meta        | `BlobRef/Digest`    | `put/get/presign`          | SB‑06/10/11/18       |
| **★ SB‑18 crypto**         | Canonical/Digest/JWS/AEAD/KMS | 原文/值对象       | 签名/密文           | `sign/verify/aead_*`       | SB‑03/11/15/19       |
| **★ SB‑19 net**            | HTTP 客户端/策略/拦截器       | Request+Policy    | Response/Stream     | `client.send(..)`          | SB‑03/04/11/14/16/18 |

> **结论**：四个新增模块**自然嵌入**既有主干：**Net** 成为外呼统一入口、**Crypto** 成为 A2A/Net/Blob 的安全底座、**Blob** 负责大对象、**Cache** 则在只读与热点流程明显降 p95。接口粒度与命名都与既有 SPI 风格一致。

------

## D. 关键缺陷 / 不明确项（优先级从高到低）

> **原则**：只在**现有模块内**做修订；**不新增模块**，除非工程化“非改不可”。

### D‑1 错误码与公共视图（高优）

- **拼写/命名**：在 A2A TD 文档出现 `SCHEMA_VAILDATION`（拼写）；全局采用 `SCHEMA_VALIDATION`。
- **命名一致**：`SANDBOX.PERMISSION_DENY` 建议统一为 `SANDBOX.PERMISSION_DENIED`（或保留旧码并映射到新码，逐步迁移）。
- **码位缺口**：`TX.*` 系列（如 `TX.TIMEOUT/IDEMPOTENT_BUSY/IDEMPOTENT_LAST_FAILED`）未在 SB‑02 码表中显式列全；`A2A.*` 子码已存在但建议补齐 `A2A.REPLAY/A2A.CONSENT_REQUIRED/A2A.LEDGER_MISMATCH` 的对外映射。
- **执行建议**：在 SB‑02 补齐**统一码表 + HTTP/gRPC 映射**，SB‑05 响应面只走公共视图。

### D‑2 幂等锚点统一（高优）

- 目前并存：`Idempotency‑Key`（网关/客户端）、`Envelope.envelope_id`（推荐）、`(channel, seq)`（A2A）。
- **统一约定**：
  1. **全链路主锚**= `Envelope.envelope_id`；
  2. **局部锚**= `(channel, seq)`、`Idempotency‑Key` 仅做**二级校验**；
  3. SB‑10/14/15/19 的去重、结算、收据与重试逻辑**以 `envelope_id` 为准**。

### D‑3 Auth × QoS 的职责边界（高优）

- 入口拦截/工具调用/Net 出站**可能**出现“双重扣额或漏扣”。
- **统一策略**：在 `AuthFacade.authorize(..)` 内部调用 `QosFacade.check_and_consume`（或 `reserve`），上层仅得到**单一决策**（Allow/RateLimited/Exceeded + DegradePlan）。Net 的 `QoS‑Bytes` 只做**字节级**预算记录，不再单独扣“次数配额”。

### D‑4 Config 快照一致性（中高）

- 多模块写了 `policy_hash/config_version`，但不是所有 Evidence/账页/回执都**强制**记录。
- **统一要求**：调用开始取 **Snapshot Hash**，贯穿 Evidence/账页/回执，**体现“按旧配置完成本次调用”**；新快照只影响后续调用。

### D‑5 Sandbox 安全阈值（中高）

- RIS 已覆盖大部分守则，但需**默认拒绝 + 白名单**：路径归一化、域名/端口白名单、SSRF/重定向校验、解压炸弹防护、CPU/内存/时长限额、上传/下载体积上限。
- **同意强制**：高风险能力（出网/文件系统/进程）默认要求 `Consent`。

### D‑6 Cache 键规范与一致性（中）

- 建议**强约束** Key：`tenant[:subject|roles_hash]:ns:policy_hash:resource:key`；
- SWR 只在**只读**路径启用；与 Tx（提交后）/Storage（变更通知）联动**主动失效**，避免“先读后写”短窗口脏读。

### D‑7 Net 拦截器与 SB‑05 的复用关系（中）

- Net 的拦截器链条与 SB‑05 概念近似，易产生**重复实现**。
- **建议**：Net 仅提供**网络段特有**拦截器（超时/重试/断路器/字节预算/缓存钩子），身份与审计类拦截器**通过 SB‑05 统一注入**或以“适配器”模式引用同一实现。

### D‑8 Blob 与 Crypto 的对齐（中）

- Blob `put` 的 **幂等**建议基于 `(bucket, key, sha256)`；ETag 算法与 Digest 统一为 **SHA‑256**；开启**可选的服务端加密（AEAD）**走 SB‑18，key 派生绑定 `{tenant, resource, envelope_id}`。
- **日志红线**：仅记录 `BlobRef + Digest`，不落原文；Retention 策略落 SB‑14。

### D‑9 Observe 观测面统一（中）

- 标签白名单需集中到 SB‑11；/metrics 暴露一致化；错误码覆盖率计量（≥99.9%）落地。

### D‑10 Surreal 适配串联（中）

- 现状多处 RIS 内存后端；建议优先补齐 SB‑09 的 Surreal 适配（Repo/Tx/Migrator），随后把 SB‑10/11/14/15 的存储切换上来，避免实现分叉。

------

## E. 分阶段落地（从 RIS → MVP → Prod）

**阶段 1｜竖切 MVP（尽快可用）**
 启用：`types + errors + config + interceptors + auth(含前置配额) + llm + tools + sandbox(白名单) + storage(Surreal 单实例) + observe(+白名单) + net + cache(SWR 仅只读)`；
 可选：`contract‑testkit` 少量强用例、`benchmark` 小基线。

**阶段 2｜增强可靠性**
 上 **Outbox+Idempotency**（SB‑10），Saga 只覆盖 1–2 条跨服务链；
 `qos` 打开 `reserve/settle` 与账页聚合；
 `a2a` 影子流量；
 `/metrics` 与**错误码覆盖**落真。

**阶段 3｜外延与对账**
 `a2a` 换 **JWS/COSE + Outbox**；
 `crypto` 对接 keystore/轮换；
 `blob` 启用 AEAD 与生命周期归档；
 `qos` 补定价/对账器与异常成本报警。

------

## F. 针对 16–19 新增模块的“鹰眼”点评

- **SB‑16 cache**：
  - **亮点**：SingleFlight + SWR、订阅式失效、与 Net 的 CacheHook；Key 设计考虑 `tenant/roles_hash/policy_hash`。
  - **改进**：强制记录 `config_version`；只读路径默认 SWR=on，**写后**通过 Storage 事件或 Tx 提交回调**强失效**。
- **SB‑17 blob**：
  - **亮点**：Presign get/put、幂等 PUT、Retention/Evidence 钩子。
  - **改进**：把 ETag/Digest 统一到 **sha256**；加“**大小/类型**策略”与**计量上报**，日志只落 `BlobRef+Digest`。
- **SB‑18 crypto**：
  - **亮点**：Canonical JSON/CBOR、JWS/COSE、AEAD/HKDF、KeyPolicy。
  - **改进**：A2A/Net 统一默认曲线/算法（建议 Ed25519 + JWS Compact）；确保恒时比较/zeroize 全面开启；记录 `kid` 与 `alg` 于 Evidence。
- **SB‑19 net**：
  - **亮点**：策略注入、分相位超时（connect/ttfb/total）、断路器/重试、`QoS‑Bytes` 与 `CacheHook/SWR`。
  - **改进**：与 SB‑05 的拦截面**去重**；把 JWS/Bearer 鉴权实现复用自 SB‑04；重试策略与**幂等规则**绑定（仅幂等方法自动重试）。

------

## G. 工程执行清单（不新增模块前提）

1. **统一码表与拼写修复**（SB‑02）：补 `TX.* / A2A.* / SANDBOX.*`，修正 `SCHEMA_VALIDATION` 等；SB‑05 仅输出公共视图。
2. **幂等主锚生效**：SB‑10/14/15/19 全面以 `Envelope.envelope_id` 去重/结算/收据；局部锚仅作辅助。
3. **Auth×QoS 融合**：`AuthFacade` 内聚 `QosFacade.check_and_consume/reserve`，Net 的 `QoS‑Bytes` 只记字节预算。
4. **Config 快照落证**：所有 Evidence/账页/回执写 `config_version/hash`。
5. **Sandbox 基线**：默认拒绝 + 白名单；高风险能力需要 `Consent`；限额/SSRF/路径归一化落地。
6. **Cache 只读化**：SWR 仅在只读；配合 Storage/Tx 事件驱动失效；Key 统一规范。
7. **Blob‑Crypto 对齐**：`ETag=sha256`、可选 AEAD（HKDF 绑定 tenant/resource/envelope_id）；日志脱敏。
8. **Observe 统一**：标签白名单、/metrics 暴露一致、错误码覆盖率统计≥99.9%。
9. **Surreal 适配优先**：先落 SB‑09，再切 SB‑10/11/14/15 的 Store。
10. **CI 双门禁减压**：Contract‑TestKit 只保留关键正/负用例；Benchmark 走短基线，夜间全量。

------

## 结语

从**整体生命体**视角看，Soulbase 的信息与数据已经具备“**上善若水、温润如玉**”的流态：契约一致、路径清晰、证据可追、成本可控。接下来只需把本文列出的**跨模块统一项**（错误码、幂等锚、Auth×QoS、Config 快照）做实，再按“三阶段上电”推进，你们就能在**不新增模块**的前提下，把 19 个模块**融为一体**、稳定上线。

> 若需要，我可以把这份体检报告转成 **10–12 条工程 issue（含验收标准）**，或输出 **端到端时序图/依赖拓扑图（文字版→图片）** 供团队评审使用。
>  另外，本回答在评估时参考了你此前的**15 模块总装手册**（“怎么用/怎么串/潜在缝隙”口径），与 ZIP 内的 19 个模块三件套对齐。

------





非常清晰，这样推进**更高效**。下面我把前面体检结论落成 **12 条工程 Issue（每条含：范围/改动对象/修改指引/验收标准/是否动代码）**。整体依据你们对 19 个模块“三件套”的既有模板与职责口径整理，无新增模块，只在**现有文档与 RIS 实现里对齐与加固**。

------

## ISSUE 01｜统一错误码与公共视图（拼写/码表/映射一致）

**范围模块**：SB‑02 `errors`（主）、SB‑05 `interceptors`、SB‑06 `sandbox`、SB‑10 `tx`、SB‑11 `observe`、SB‑15 `a2a`
 **改动对象**：

- 《SB‑02‑功能规约.md》→「错误码表」小节（若无则新建）
- 《SB‑02‑技术设计.md》→「HTTP/gRPC 映射与对外视图」
- 《SB‑05/06/10/15‑技术设计.md》→引用统一码表的小节
- 《SB‑02‑RIS.md》《SB‑05‑RIS.md》→示例/测试用码位替换
   **如何修改**：

1. 统一拼写：将所有 `SCHEMA_VAILDATION` 改为 `SCHEMA_VALIDATION`；`SANDBOX.PERMISSION_DENY` 改为 `SANDBOX.PERMISSION_DENIED`（保留旧码兼容映射，标注弃用）。
2. 补齐专用码位：新增（或显式列出）`TX.TIMEOUT / TX.IDEMPOTENT_BUSY / TX.IDEMPOTENT_LAST_FAILED`、`A2A.REPLAY / A2A.CONSENT_REQUIRED / A2A.LEDGER_MISMATCH / A2A.SIGNATURE_INVALID`、`SANDBOX.CAPABILITY_BLOCKED`。
3. 输出稳定映射表：每个错误码→`http_status/grpc_status/retryable/severity`。
4. SB‑05 统一“公共视图”：对外响应一律 `ErrorObj::to_public()`，严禁裸 `String`。
    **验收标准**：

- `contract-testkit` 负向用例 100% 使用 SB‑02 码位并通过；
- 代码库全局 grep 无未知码或拼写错；
- /metrics 误码率 < 0.1%（SB‑11 指标）。
   **是否动代码**：是（RIS 示例、拦截器与错误建造器替换）。

------

## ISSUE 02｜全链路幂等锚点统一：以 `Envelope.envelope_id` 为唯一主锚

**范围模块**：SB‑01 `types`、SB‑10 `tx`、SB‑14 `qos`、SB‑15 `a2a`、SB‑19 `net`
 **改动对象**：

- 《SB‑01‑功能规约.md》→「幂等与关联」
- 《SB‑10/14/15‑技术设计.md》→「去重/结算/收据键」
- 《SB‑10/14/15/19‑RIS.md》→实现与测试
   **如何修改**：

1. 规范：声明**全局主锚**=`Envelope.envelope_id`（推荐 UUIDv7/128bit），`Idempotency‑Key` 与 `(channel,seq)` 仅为**二级校验**。
2. SB‑10：Outbox/去重表唯一键切到 `envelope_id`；重放策略以主锚判断。
3. SB‑14：账页去重锚= `envelope_id`；历史聚合与异常重结算以主锚合并。
4. SB‑15：收据链、反重放窗口以 `envelope_id` + `seq` 校验，主判定看 `envelope_id`。
5. SB‑19：对外请求默认注入 `X-Env-Id`。
    **验收标准**：

- 同请求重放/重试不产生重复账页/重复投递；
- 端到端统计 `dedup_hits`>0 且与重试次数匹配；
- 合同测试包含“重放 3 次仅一次生效”。
   **是否动代码**：是（存储唯一索引/拦截器头部/去重逻辑）。

------

## ISSUE 03｜Auth×QoS 职责内聚：入口只检查一次

**范围模块**：SB‑04 `auth`（主）、SB‑14 `qos`、SB‑05 `interceptors`
 **改动对象**：

- 《SB‑04‑功能规约.md》→「授权决策模型」
- 《SB‑04‑技术设计.md》→「AuthFacade 内部对接 QosFacade」时序
- 《SB‑05‑技术设计.md》→「入口策略」引用变更
- 《SB‑04/05‑RIS.md》→实现/单测
   **如何修改**：

1. `AuthFacade.authorize(..)` 内部调用 `QosFacade.check_and_consume/reserve`，向上游返回**单一决策**（Allow/RateLimited/Exceeded + DegradePlan）。
2. 禁止在工具/网络侧再次扣“次数配额”（`QoS‑Bytes` 仅记录字节预算）。
    **验收标准**：

- 端到端每请求**仅一次**配额扣减；
- 降级分支（RateLimited→DegradePlan）可被观测到；
- 双重扣额的保护用例通过。
   **是否动代码**：是。

------

## ISSUE 04｜Config 快照贯穿：证据/账页/回执强制记录 `config_version/hash`

**范围模块**：SB‑03 `config`（主），联动 SB‑06/08/11/14/15
 **改动对象**：

- 《SB‑03‑功能规约.md》→「快照一致性」
- 各模块《技术设计.md》→对应数据模型新增 `config_version/hash` 字段
- 相关《RIS.md》→头部注入与证据写入
   **如何修改**：
- 入站确定快照→贯穿到 Evidence/Ledger/Receipt；响应头写 `X-Config-Version/Checksum`。
   **验收标准**：
- 任一请求全链路 `config_hash` 一致；
- 热更期间旧请求按旧快照完成；
- 合同测试覆盖“热更中请求不漂移”。
   **是否动代码**：是。

------

## ISSUE 05｜Sandbox 默认拒绝 + 高风险 Consent 强制

**范围模块**：SB‑06 `sandbox`（主）、SB‑08 `tools`
 **改动对象**：

- 《SB‑06‑功能规约.md》→「能力白名单/黑名单与限额」
- 《SB‑06‑技术设计.md》→路径归一化、SSRF/重定向校验、CPU/内存/时长/体积上限；
- 《SB‑08‑技术设计.md》→`Preflight` 增加 Consent 校验
- 《SB‑06/08‑RIS.md》→默认 Guard 实现与负向用例
   **如何修改**：
- 默认拒绝任何“出网/文件/进程”，按白名单逐项放行；高风险能力需要 `Consent`。
   **验收标准**：
- 负向用例（越权 URL、压缩炸弹、路径穿越）必然 `SANDBOX.PERMISSION_DENIED`；
- 限额超阈值返回 `SANDBOX.LIMIT_EXCEEDED`；
- 日志无敏感原文，仅证据摘要。
   **是否动代码**：是。

------

## ISSUE 06｜Net 拦截器与 SB‑05 去重：实现复用与职责切分

**范围模块**：SB‑19 `net`（主）、SB‑05 `interceptors`、SB‑04 `auth`、SB‑11 `observe`
 **改动对象**：

- 《SB‑19‑功能规约.md》→「网络段专属拦截器」
- 《SB‑19‑技术设计.md》→以适配器复用 SB‑05 的身份/审计实现
- 《SB‑19‑RIS.md》→移除重复逻辑，保留超时/重试/断路器/字节预算/缓存钩子
   **如何修改**：
- SB‑19 不再自带 Auth/Audit 逻辑，改为引用 SB‑05/04 提供的实现。
   **验收标准**：
- 代码层 grep 无重复 Auth/Audit 拦截；
- 出站请求具备 Trace/UA/审计标签；
- 重试仅对幂等方法启用。
   **是否动代码**：是（但不破坏外部 API）。

------

## ISSUE 07｜Cache 键规范与写后强失效（只读路径 SWR）

**范围模块**：SB‑16 `cache`（主）、SB‑09 `storage`、SB‑10 `tx`、SB‑19 `net`
 **改动对象**：

- 《SB‑16‑功能规约.md》→「Key 规范与 TTL/SWR」
- 《SB‑16‑技术设计.md》→写后失效（订阅存储变更/Tx 提交回调）
- 《SB‑16‑RIS.md》→`get_or_load/do_once` 实现与示例
   **如何修改**：
- 统一 Key：`tenant[:subject|roles_hash]:ns:policy_hash:resource:key`；SWR 仅用于**只读**；写后通过存储事件/Tx 回调**强失效**。
   **验收标准**：
- “先读后写”不出现脏读；
- 命中率/陈旧度指标可观测；
- Cache 针对不同租户隔离。
   **是否动代码**：是。

------

## ISSUE 08｜Blob×Crypto 对齐：ETag=SHA‑256、可选 AEAD、日志脱敏

**范围模块**：SB‑17 `blob`（主）、SB‑18 `crypto`
 **改动对象**：

- 《SB‑17‑功能规约.md》→「幂等语义与 ETag/Digest」
- 《SB‑18‑技术设计.md》→AEAD/HKDF 策略（绑定 `{tenant, resource, envelope_id}`）
- 《SB‑17/18‑RIS.md》→`put/get/presign` 与 AEAD demo、日志仅落 `BlobRef+Digest`
   **如何修改**：
- 以 `(bucket,key,sha256)` 作为 PUT 幂等锚；开启可选 AEAD；统一日志脱敏。
   **验收标准**：
- 相同内容重复 PUT 不产生重复对象；
- AEAD 往返正确；
- 日志/证据不含原文。
   **是否动代码**：是。

------

## ISSUE 09｜Observe 统一：标签白名单与 /metrics 暴露

**范围模块**：SB‑11 `observe`（主），全员引用
 **改动对象**：

- 《SB‑11‑功能规约.md》→「稳定标签白名单」
- 《SB‑11‑技术设计.md》→Prom/OTLP 暴露与采样策略
- 《SB‑11‑RIS.md》→白名单校验、中高基线指标族
   **如何修改**：
- 只允许白名单标签；导出 `/metrics`；错误码覆盖率统计（目标 ≥99.9%）。
   **验收标准**：
- 非白名单标签在编译期/单测被拒；
- 基线仪表板可见关键直方图；
- 覆盖率指标达标。
   **是否动代码**：是。

------

## ISSUE 10｜Surreal 适配优先：统一落地 Repo/Tx/Migrator

**范围模块**：SB‑09 `storage`（主），联动 SB‑10/11/14/15
 **改动对象**：

- 《SB‑09‑功能规约.md》→「Repository/Tx 约定」
- 《SB‑09‑技术设计.md》→参数化查询/租户强约束/迁移策略
- 《SB‑09‑RIS.md》→Surreal 适配；迁移脚本模板
- 调整《SB‑10/11/14/15‑RIS.md》→切换到 Surreal 适配
   **如何修改**：
- 先补 SB‑09 适配与迁移；再将 SB‑10/11/14/15 的存储从内存切至 Surreal。
   **验收标准**：
- 集成测试通过（读写/事务/迁移）；
- Outbox/Ledger/A2A 存储一致；
- 回滚脚本有效。
   **是否动代码**：是。

------

## ISSUE 11｜CI 双门禁减负：契约最小集 + 基准短基线

**范围模块**：SB‑12 `benchmark`、SB‑13 `contract-testkit`
 **改动对象**：

- 《SB‑12‑功能规约.md》《SB‑13‑功能规约.md》→「CI 运行策略」
- 《SB‑12/13‑技术设计.md》→用例分层/跳过策略/夜间全量
- CI 配置示例（附在两者 RIS 或单独 `ci/examples`）
   **如何修改**：
- PR 只跑核心正向 + 高价值负向用例、短基准；夜间跑全量。
   **验收标准**：
- 主干 CI 时长 ≤ N 分钟（团队约定）；
- 关键用例覆盖不下降；
- 失败日志可定位到具体契约断言。
   **是否动代码**：否（多为配置与用例裁剪；RIS 示例可小改）。

------

## ISSUE 12｜A2A 生产化：JWS/COSE + Outbox 可靠投递 + 反重放

**范围模块**：SB‑15 `a2a`（主）、SB‑18 `crypto`、SB‑10 `tx`、SB‑11 `observe`、SB‑14 `qos`
 **改动对象**：

- 《SB‑15‑功能规约.md》→「签名/回执/反重放/收据链」
- 《SB‑15‑技术设计.md》→Ed25519 + JWS Detached（含 `kid/alg`），反重放窗口，收据链校验；Outbox 集成
- 《SB‑18‑技术设计.md》→KeyStore/轮换/撤销
- 《SB‑15/18‑RIS.md》→签名/验签 demo、反重放用例
   **如何修改**：
- 替换 RIS 的 HMAC 伪签为 JWS/COSE；A2A 消息出入统一走 SB‑10 Outbox；反重放以 `envelope_id` 主锚 + `seq/nonce` 窗口。
   **验收标准**：
- 跨域验签 100% 通过；
- 模拟重放被拒；
- 双边账页互认一致。
   **是否动代码**：是。

------

## 文档修改范围「矩阵」总览（回答你“改哪份”的问题）

| Issue             | 功能规约      | 技术设计 | RIS（最小代码骨架） |
| ----------------- | ------------- | -------- | ------------------- |
| 01 错误码统一     | ✅（主）       | ✅        | ✅                   |
| 02 幂等主锚       | ✅             | ✅        | ✅                   |
| 03 Auth×QoS       | ✅             | ✅        | ✅                   |
| 04 Config 快照    | ✅             | ✅        | ✅                   |
| 05 Sandbox 基线   | ✅             | ✅        | ✅                   |
| 06 Net×拦截器去重 | ✅（职责声明） | ✅        | ✅                   |
| 07 Cache 规范     | ✅             | ✅        | ✅                   |
| 08 Blob×Crypto    | ✅             | ✅        | ✅                   |
| 09 观测统一       | ✅             | ✅        | ✅                   |
| 10 Surreal 适配   | ✅             | ✅        | ✅                   |
| 11 CI 门禁策略    | ✅             | ✅        | （可选）            |
| 12 A2A 生产化     | ✅             | ✅        | ✅                   |

> 结论：**不仅仅改“功能规约”**。为了让其它 AI 能“对照即改、改完即跑”，**大多数 Issue 同步要求更新「技术设计」与「RIS」**（至少补充最小示例与单测），这样你的三件套才能保持闭环一致。

------

## 可直接下发给“执行 AI”的修改指引模板（示例）

> 以 ISSUE 01 为例，其他按此格式展开：

- **定位文件**：`/soulbase/sb-02-errors/文档/《SB-02-功能规约.md》#错误码表`
- **编辑动作**：
  - 替换：`SCHEMA_VAILDATION`→`SCHEMA_VALIDATION`；`SANDBOX.PERMISSION_DENY`→`SANDBOX.PERMISSION_DENIED`（保留兼容映射表）。
  - 新增表格行：`TX.TIMEOUT / TX.IDEMPOTENT_BUSY / TX.IDEMPOTENT_LAST_FAILED / A2A.REPLAY / A2A.CONSENT_REQUIRED / A2A.LEDGER_MISMATCH / SANDBOX.CAPABILITY_BLOCKED`，并填写 `http_status/grpc_status/retryable/severity` 四列。
- **同步文件**：
  - `《SB-02-技术设计.md》#公共视图映射`：更新映射函数伪码；
  - `《SB-05-技术设计.md》#统一出错响应`：仅调用 `ErrorObj::to_public()`；
  - `《SB-02-RIS.md》`：替换示例代码；新增 2 个负向单测。
- **验收脚本**：运行 `cargo test -p sb02_errors && cargo test -p sb05_interceptors`；运行 `./scripts/grep_unknown_error_codes.sh` 确认无未知码。

