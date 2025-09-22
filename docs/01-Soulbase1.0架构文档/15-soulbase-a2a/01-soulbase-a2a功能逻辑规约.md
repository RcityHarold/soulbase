### **文档 SB-15：soulbase-a2a（跨域协议 / 账本互认 · Attestation & Agreement）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：为 Soul 生态提供一套**安全、最小披露、可审计**的**跨域（A2A, Agent-to-Agent / Account-to-Account）交换协议**，用于在**不同租户/组织/平台**之间，进行**请求/通知/凭证/账页**等事件的**可信交换与互认**，实现：
  1. **身份与能力证明（Attestation）**：对消息与主体进行**强认证与抗抵赖签名**；
  2. **最小必要披露（Minimal Disclosure）与同意（Consent）**：遵循可验证凭证与范围同意；
  3. **账本互认（Ledger Agreement）**：对跨域操作与成本/用量形成**双向一致的“收据链”（Receipt Chain）**；
  4. **防重放/时序一致（Anti-Replay & Ordering）**：通道内**单调序号 + 时间窗 + Nonce**；
  5. **可回放审计（Evidence-first）**：所有交互产生**Envelope**留痕，支持追溯与对账。
- **范围**：
  - 协议角色、消息类型与状态机；
  - 身份与密钥（DID/JWK/证书）交换、能力协商、同意与撤销；
  - 签名/验签、反重放、回执与对账；
  - 与 **SB-10 Tx**（可靠事务）、**SB-14 QoS**（账页）、**SB-11 Observe**（证据/指标）联动。
- **非目标**：不提供区块链共识或跨链桥；不替代业务语义，只定义**安全与互认层**。

------

#### **1. 功能定位（Functional Positioning）**

- **跨域信任层**：位于业务之下、传输之上，解决**谁**（身份）、**能做什么**（能力/授权）、**是否发生**（证据）与**双方都承认**（互认账本）。
- **证据单一真相源（SSoT）**：所有 A2A 交互以**签名 Envelope**为主线，证据与账页与观察面一致。
- **默认拒绝 + 最小披露**：没有能力凭证与同意，**禁止**任何敏感数据出域；仅发送**摘要/哈希承诺**与必要字段。

------

#### **2. 系统角色与地位（System Role & Position）**

- 层级：**基石层 / Soul-Base**（横跨所有模块的跨域外延）。
- 关系：
  - **SB-04 Auth**：本域对外签发/验证 OIDC/JWT/VC（可验证凭证），A2A 只消费公共声明；
  - **SB-10 Tx**：A2A 消息与本地 Outbox 同事务入库，投递失败走重试/死信/回放；
  - **SB-14 QoS**：跨域计量入账页；对账差异进入 A2A 对账流程；
  - **SB-11 Observe**：输出 `A2A*` 指标与 Evidence；
  - **SB-06/08/07/09**：作为 A2A 的上层业务语义消费者/生产者（工具请求、模型代理、跨域数据读写等）。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

- **A2A 通道（Channel）**：双方建立的一条**半双工或全双工**逻辑链路，具**通道 ID**、**密钥指纹**、**序列号（seq）**、**时钟偏差容忍**与**策略快照 hash**。
- **主体与密钥（Identity & Keys）**：主体以 `Subject`（SB-01 types）+ **公钥集（JWK/COSE）**表述，支持 DID/证书指纹。
- **能力凭证（Capability Token）**：表达**资源/动作/条件**，可采用 macaroon/JWT/VC 子集，支持**范围与过期**。
- **同意凭据（Consent Proof）**：对**高风险/敏感**操作的同意与约束（范围/期限/目的）。
- **消息类型（Message）**：
  - `Offer`（能力/策略/价目/留存提议）
  - `Request`（跨域调用/查询/结算请求）
  - `Notice`（事件通知/账页片段）
  - `Receipt`（回执，包含哈希承诺/签名/序列）
  - `Ack/Nack`（传输层确认/拒绝）
- **收据链（Receipt Chain）**：双方对**同一交互**生成**对称签名回执**（request hash 与 response hash），与账页行条目绑定。

------

#### **4. 不变式（Invariants）**

1. **显式身份与能力**：每条消息必须绑定**主体 ID + 密钥指纹 + 能力凭证**；
2. **最小披露**：payload 发送**摘要/选择性字段**；敏感内容优先以“**承诺（Commitment）**”方式（哈希+盐）表达；
3. **时间窗 + 序列 + Nonce**：每条消息包含 `ts`、`seq`、`nonce`，超窗或乱序/重复即拒绝；
4. **双签回执**：完成后双方各自签名回执，**缺一**则视为未完成；
5. **等幂**：以 `(channel_id, seq)` 与 `envelope_id` 为幂等锚点；
6. **撤销生效**：能力/同意撤销表下发后，**新消息**必须按撤销表校验，旧消息按“快照生效”；
7. **可回放**：所有签名与摘要进入 Evidence，与本地 Outbox/Saga/Ledger 一致。

------

#### **5. 能力与接口（Abilities & Abstract Interfaces）**

> 仅定义**行为口径**；具体 SPI 在 TD/RIS 落地。

- **通道管理（Channel Manager）**
  - `open(peer_metadata)` → `channel_id`（包含对方公钥、策略摘要、支持特性）；
  - `rotate_keys(channel_id)`：双边密钥轮换；
  - `close(channel_id)`：优雅关闭。
- **能力/策略协商（Capability/Policy Negotiation）**
  - `negotiate(channel, offer)`：达成最小交集能力/价目/留存策略；
- **消息签名与验签（Signer/Verifier）**
  - `sign(envelope) -> detached_signature`；`verify` 返回**证据结构**；
- **反重放与序列控制（Replay Guard）**
  - `check(channel, seq, ts, nonce)`：维护滑窗与 nonce 集合；
- **请求/回执（Request/Receipt）**
  - `send(channel, request) -> receipt_local`；
  - 接收方 `handle(request) -> receipt_remote`；
  - `sync_receipt(channel, receipt_remote)`：互存收据，更新账页与 Evidence。
- **撤销与对账（Revocation & Reconcile）**
  - `publish_revocations()`；`reconcile(ledger_local, ledger_peer) -> diffs[]`；
- **错误规范化（Normalize）**
  - `A2A.SIGNATURE_INVALID / A2A.REPLAY / A2A.CONSENT_REQUIRED / A2A.CAPABILITY_DENY / A2A.LEDGER_MISMATCH / PROVIDER.UNAVAILABLE` 等。

------

#### **6. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **指标**
  - `a2a_messages_total{type,peer}`、`a2a_verify_latency_ms`、`a2a_replay_blocked_total`
  - `a2a_receipts_total{status}`（双签完成/缺失）
  - `a2a_ledger_diff_usd{peer}`、`a2a_revocation_hits_total`
- **SLO**
  - 验签延迟 p95 **≤ 5ms**（本地密钥）；
  - 重放拦截率 **= 100%**；
  - 双签完成率 **≥ 99.99%**；
  - 对账差异**≤ 0.5%** 月累计。
- **验收**
  - 契约测试覆盖**签名/反重放/双签/撤销**；
  - 基准测试覆盖**验签开销**；
  - 端到端演示**跨租户工具调用 + 成本互认**场景。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：`soulbase-auth`（发行/验证声明）、`soulbase-config`（策略/价目/留存）、`soulbase-qos`（账页）、`soulbase-observe`（证据/指标）、`soulbase-tx`（可靠投递）；
- **下游**：传输层（HTTP/Kafka/NATS/自研总线）；
- **边界**：不提供传输 QoS（由总线/网关负责）；不实现业务语义，仅保证**安全与互认**。

------

#### **8. 风险与控制（Risks & Controls）**

- **密钥轮换/吊销不及时** → 通道元数据包含**轮换策略与生效时间窗**；收到新指纹即**并行验证**过渡期内消息；
- **重放与乱序** → `seq + ts + nonce` 强校验；过窗消息拒绝；
- **能力漂移/越权** → 仅在**协商的交集能力**内执行；能力变更需重新协商；
- **账页不一致** → 双签收据链 + 定期对账；差异进入 **A2A.LEDGER_MISMATCH** 并触发人工复核；
- **隐私泄露** → 默认只传摘要/承诺；必要明文字段走**字段级同意**；
- **时钟漂移** → 允许 ±300s 偏差；偏差过大进入 `A2A.CLOCK_SKEW`（可选）并拒绝。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 通道建立与能力协商**

1. A→B：`Offer{identity, jwk_fingerprint, capabilities, pricing, retention, policy_hash}`（签名）；
2. B 验签+校验 → 回 `Ack{capabilities=交集, policy_hash}`（签名）；
3. 双方存证 `ChannelOpenEvidence`，后续消息绑定 `channel_id`。

**9.2 跨域工具调用（最小披露）**

1. A→B：`Request{tool_id, args_digest, consent_proof?, qos_estimate, seq, ts, nonce}`（签名）；
2. B 验签+反重放+能力/同意检查 → 在本域执行（仅凭**承诺验证**/最小必要字段）；
3. B→A：`Receipt{request_hash, result_digest, usage_summary, code, seq}`（签名）；
4. A 存证收据，与 **SB-14 QoS** 结算入账页。

**9.3 价目与对账**

1. 定期交换 `LedgerSummary{period, total_usd, lines_digest}`（签名）；
2. 差异 → `A2A.LEDGER_MISMATCH` 事件，触发复核/重放。

**9.4 撤销与关闭**

1. 任何一方发布 `Revocations{capability_ids, consent_ids}`；
2. 新消息按撤销表强校验；
3. `Close{reason}`（签名）→ 双方归档通道证据与账页。

------

#### **10. 错误与公共视图（Errors & Public View）**

- 统一稳定码（由 **SB-02** 提供/扩展）：
  - `A2A.SIGNATURE_INVALID`、`A2A.REPLAY`、`A2A.CONSENT_REQUIRED`、`A2A.CAPABILITY_DENY`、`A2A.LEDGER_MISMATCH`、`PROVIDER.UNAVAILABLE`、`SCHEMA.VALIDATION_FAILED`、`UNKNOWN.INTERNAL`。
- 对外返回**公共视图**：`{code, message, correlation_id}`；诊断细节仅写入 Evidence。

------

> 若你认可该“功能逻辑规约”，我将进入 **SB-15-TD（技术设计）**，输出：
>  **消息与收据结构（JWS/COSE）**、**通道/序列/反重放状态机**、**能力/同意凭证数据结构**、**Signer/Verifier SPI 与密钥轮换**、**与 Tx/QoS/Observe 的落地接口**，并随后提供 **SB-15-RIS（最小可运行骨架）**。
