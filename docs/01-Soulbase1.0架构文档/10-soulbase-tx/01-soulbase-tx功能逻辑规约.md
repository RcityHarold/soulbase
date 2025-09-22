### **文档 SB-10：soulbase-tx（可靠事务 / Outbox · Saga · Idempotency）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：为 Soul 生态提供**跨边界的可靠事务底座**，统一实现：
  1. **Outbox**（本地事务 + 异步投递）保证**数据库状态与事件投递一致**；
  2. **Saga**（有向补偿流程）在跨服务/跨资源时实现**最终一致**与**可回滚**；
  3. **幂等性**与**去重**（Idempotency/Exactly-once-at-least语义）；
  4. **重试/退避/死信**与**可回放**（Event replay）；
  5. **证据/审计**与**可观测**（延迟、成功率、补偿率、死信率）。
- **范围**：
  - 抽象：`OutboxStore`、`Dispatcher`、`SagaOrchestrator/Participant`、`IdempotencyStore`；
  - 语义：本地**写库 + 记录 Outbox 消息**的原子性、跨步骤补偿/超时/并发度控制；
  - 与基座集成：`soulbase-storage`（持久化/游标）、`-observe`（指标）、`-errors`（稳定码）、`-qos`（成本/速率）、`-interceptors`（Envelope 绑定）、`-auth`（投递时鉴权）。
- **非目标**：不替代消息中间件/队列（仍可用队列），本模块聚焦**可靠生产/消费协议与流程控制**；不强绑定具体数据库（默认优先 SurrealDB 适配）。

------

#### **1. 功能定位（Functional Positioning）**

- **一致性网关**：把“写库 + 发布事件/跨服务调用”转为**单进程原子提交 + 异步可重放**；
- **业务编排器**：以 Saga 模式描述业务步骤与**补偿函数**，提供**时限/并发/隔离/幂等**的执行框架；
- **可观测控制面**：统一重试/退避、死信箱/回放、幂等命中率与补偿率等指标。

------

#### **2. 系统角色与地位（System Role & Position）**

- 所处层级：**基石层 / Soul-Base**；被内核/工具服务/业务服务直接使用；
- 关联：
  - `soulbase-storage`：Outbox 表、Saga 状态表、幂等表、死信表；
  - `soulbase-interceptors`：绑定 `Envelope`、`TraceId`、`X-Request-Id`；
  - `soulbase-errors`：`TX.* / STORAGE.* / PROVIDER.UNAVAILABLE / UNKNOWN.*` 映射；
  - `soulbase-observe`：投递/重试/补偿/死信指标；
  - `soulbase-qos`：速率/并发/成本预算（投递/补偿开销）；
  - `soulbase-auth`：投递到外域时的凭证/授权；
  - `soulbase-a2a`：跨域/跨账本事务证据。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

**3.1 Outbox 消息（OutboxMessage）**

- 字段：`id, tenant, envelope_id, topic, payload(json), created_at, not_before, attempts, status(pending|inflight|done|dead), last_error, dispatch_key(幂等键)`
- 语义：与业务写入在**同一数据库事务**中插入 `outbox` 记录，事务提交后由 `Dispatcher` 异步投递（HTTP/队列/内部总线）。

**3.2 Idempotency（幂等）**

- 表：`idempo`（`key, tenant, hash, status, result_digest, ttl`）；
- 语义：生产端/消费端均可用；**重复请求或重试**命中则短路返回缓存结果（有 TTL 与大小上限）。

**3.3 Saga**

- 组成：`SagaInstance{id, tenant, state, steps[], cursor, created_at, updated_at, timeout_at}`
- Step：`{name, action_uri|fn, compensate_uri|fn?, deadlines, retry_policy, concurrency_tag?}`
- 状态：`running | compensating | completed | failed | cancelled`
- 语义：按序/并发执行步骤；某步失败 → 进入 `compensating`，按逆序调用补偿；支持**部分可补偿/不可补偿**声明与**人工干预**挂起。

**3.4 Dead Letter（死信）**

- Outbox/Saga 超过重试上限或致命错误 → 进入 `dead_letters`；人工处理/回放。

**3.5 Evidence/审计**

- `Envelope<TxEvent>`：`TxOutboxEnqueued/Dispatched/Failed`, `TxSagaStarted/StepDone/Compensate/Completed/Failed` 等事件，**仅摘要**。

------

#### **4. 不变式（Invariants）**

1. **本地原子提交**：业务写入与 Outbox 记录**同事务提交**；
2. **至少一次投递**：投递失败可重试；下游**必须幂等**（或由本模块提供消费幂等守卫）；
3. **幂等优先**：生产/消费路径默认启用 Idempotency；
4. **超时与补偿**：Saga 步骤有**明确时限**；失败时按逆序补偿，补偿失败进入死信；
5. **可回放**：Outbox/Saga 支持**按游标回放**（审计/修复）；
6. **最小披露**：记录 `payload_digest` 与错误摘要；原文不写日志；
7. **稳定错误**：`TX.TIMEOUT` / `TX.DEADLETTER` / `TX.IDEMPOTENT_HIT` / `STORAGE.*` / `PROVIDER.UNAVAILABLE`；
8. **租户一致**：Outbox/Saga/Idempo 记录**强制携带 tenant**且所有查询默认按租户过滤。

------

#### **5. 能力与抽象接口（Abilities & Abstract Interfaces）**

> 具体 Trait/数据结构在 TD/RIS 落地；此处定义行为口径。

- **Outbox（生产）**
  - `outbox.enqueue(topic, payload, not_before?, dispatch_key?)`：返回 `id`；
  - **事务内**使用：由 `soulbase-storage` 的 `Tx` 扩展方法提供 `enqueue_in_tx(..)`；
  - 入库时校验 `payload`（JSON Schema 可选）、计算摘要与幂等键。
- **Dispatcher（投递）**
  - 轮询 `outbox`，按 `not_before/attempts`/并发度拉取批次 → 发送（HTTP/队列/内部 bus）→ 成功 `done`，失败更新 `attempts/last_error/not_before`（退避）；
  - 支持**速率限制/并发桶/优先级**（与 QoS 协作）。
- **Idempotency（生产/消费）**
  - `idempo.check_and_put(key, hash)`：若命中返回**历史结果摘要**；否则写入占位；
  - 完成后 `idempo.finish(key, result_digest)`；
  - 可独立用于 API 层（写操作的幂等键）。
- **Saga Orchestrator**
  - `saga.start(definition, input)` → `instance_id`；
  - `saga.tick(instance_id)`：推进一步（支持并发步骤与隔离标签）；
  - `saga.compensate(instance_id)`：进入补偿；
  - 定义支持**代码函数 URI**（本地）或 **HTTP URI**（远程参与者），统一返回`Ok/Retry/Fail`与**幂等特征**。
- **Dead Letter & Replay**
  - `dead.replay(outbox_id|saga_id)`：重置状态并重新投递/推进；
  - `dead.inspect(..)`：查询诊断摘要（错误码/次数/最后一次错误）。
- **审计与指标**
  - 统一生成 `TxEvent`（Begin/Success/Retry/Backoff/DeadLetter/Replay）；
  - 指标：`tx_outbox_enqueued/dispatched/success_rate/retry_total/dead_total`、`tx_saga_active/compensate_total/failed_total`、`idempo_hits` 等。

------

#### **6. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **Outbox 投递**：p95 延迟（入库→首次投递）≤ **1s**（常规负载）；成功率 ≥ **99.9%**；
- **重试/死信**：`deadletter_rate` ≤ **0.01%**（24h）；`retry_success_rate` ≥ **95%**；
- **Saga**：平均完成时长符合业务 SLO；补偿成功率 ≥ **99%**；
- **幂等命中**：重复调用命中率统计可用；误判率（错误命中） = **0**；
- **错误规范化**：`UNKNOWN.*` ≤ **0.1%**；
- **验收**：契约测试覆盖**本地原子提交 + 投递**、**重试/退避**、**幂等**、**Saga 正/逆序**、**死信/回放**与**指标**。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：`soulbase-storage`（SurrealDB 适配）、`soulbase-config`（退避/并发/过期/死信策略）、`soulbase-auth`（外呼凭证）、`soulbase-qos`（速率/预算）、`soulbase-interceptors`（上下文）、`soulbase-errors`/`-observe`。
- **下游**：HTTP/队列（Kafka/NATS/AMQP/自研 bus）、远程参与者服务。
- **边界**：不内置消息中间件；不承诺跨数据库分布式强一致（采用最终一致）。

------

#### **8. 风险与控制（Risks & Controls）**

- **双写不一致** → Outbox 与业务写入**同事务**；失败回滚；
- **乱序/重复** → **幂等键** + 消费端幂等守卫；**有序主题**可通过 `dispatch_key` 串行化；
- **雪崩重试** → 指数退避 + 抖动 + 并发上限 + QoS 速率限制；
- **补偿失败** → 死信 + 人工介入通道（标记/注释/二次回放）；
- **跨租户污染** → 所有记录带 `tenant`，投递时注入授权上下文，拦截器二次校验；
- **长事务阻塞** → 降低业务事务粒度；Outbox 仅写**必要摘要**。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 本地写库 + 可靠投递（Outbox 模式）**

1. 业务代码开启 DB 事务；
2. 写入业务表 → 在**同事务**中 `outbox.enqueue_in_tx(topic, payload, not_before?, dispatch_key?)`；
3. `COMMIT` 成功 → `Dispatcher` 轮询发现新消息 → 尝试投递；
4. 成功 → 标记 `done`；失败 → 更新 `attempts/not_before` 并按退避重试；超上限 → **死信**；
5. 期间生成 `TxOutbox*` 事件与指标。

**9.2 Saga（有补偿的跨服务流程）**

1. `saga.start`（定义若干 Step：本地写库、远程调用、工具执行…每步附补偿函数）；
2. Orchestrator 执行 Step1（本地）→ Enqueue Outbox 触发远程 Step2 → 等待回执（Polling/Callback/Outbox 消费）；
3. 若 StepN 失败 → 进入 `compensating`，按逆序执行 `compensate`；
4. 成功/失败/补偿均落证据与指标；无法补偿 → **死信**并报警；
5. 支持 `pause/resume/cancel` 与**人工干预**（设置下一步状态）。

**9.3 幂等 API**

1. 上游请求带 `Idempotency-Key`；
2. `idempo.check_and_put(key, hash)` 命中 → 返回历史结果；未命中 → 执行业务 + Outbox；
3. 完成后 `finish(key, result_digest)`；
4. 指标记录命中率与窗口利用率。

------

#### **10. 开放问题（Open Issues / TODO）**

- **跨域账本（A2A）** 的签名/对账：Outbox 负载中纳入**证据指纹**与**对账周期**；
- **长流程 Saga** 的持久化心跳/超时扫描与**分片调度**；
- **多队列后端**（Kafka/NATS/AMQP）的可插拔 Dispatcher 与一致语义适配；
- **观察诊断 UI**：对死信/重试/补偿的可视化与一键回放工具；
- **流量削峰**：在高峰期将投递移动到批量/分段投递策略，与 QoS 协同。

------

> 本规约与已完成模块**同频共振**：**参数化 + 租户强约束（依赖 storage）**、**稳定错误语义（errors）**、**观测闭环（observe）**、**预算/速率（qos）**、**Envelope 证据链（interceptors/types）**。确认无误后，我将继续输出 **SB-10-TD（技术设计）**，给出 Outbox/Saga/Idempotency 的接口与状态机详解、存储表结构、退避策略、死信与回放协议以及与 SurrealDB 的落地映射。
