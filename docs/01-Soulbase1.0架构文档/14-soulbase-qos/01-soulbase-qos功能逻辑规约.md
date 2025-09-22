### **文档 SB-14：soulbase-qos（配额 · 成本 · 留存 / Quotas · Costing · Retention）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：为 Soul 生态提供**统一的资源治理基座**，实现：
  1. **配额（Quota）**：对 **租户 / 项目 / 主体 / 资源 / 动作**分层的**软限/硬限**与**突发桶（burst）**；
  2. **成本核算（Costing）**：按 **价目表（Pricing）\**对 \*\*LLM/工具/沙箱/存储/事务\*\*用量记账，形成\**账页（Ledger）\**与\**对账（Reconcile）**；
  3. **留存（Retention）**：对事件/日志/制品/数据执行**TTL/分级归档**策略（成本与合规驱动）；
  4. **门禁与节流（Gating & Throttling）**：在**调用前**给出**许可/拒绝/降级**决策，保障稳定性与成本上限；
  5. **观测与异常（Telemetry & Anomaly）**：输出统一指标与证据，侦测超额/激增/异常成本。
- **范围**：`策略加载（Policy）· 价目表（Pricing）· 预授权/扣额（Reservation/Debit）· 账页（Ledger）· 留存与归档（Retention/Archival）· 对账（Reconcile）· 异常检测（Anomaly）`。
- **非目标**：不替代网关限流（在 Soul-Hub 侧已有）；不做金融结算/发票开具（仅提供账页与对账数据）。

------

#### **1. 功能定位（Functional Positioning）**

- **前置许可**：在**调用之前**给出是否允许、可用预算、可选**降级方案**（切小模型、禁工具、只读路径）；
- **统一记账**：将 **LLM tokens / 沙箱 bytes / 工具调用 / TX 重试 / 存储 IO / 容量与时长（GB·day）**统一进账页；
- **成本与留存一体化**：以**成本/合规**为目标，驱动**数据留存与归档**（例如证据/日志/制品的不同留存等级）。

------

#### **2. 系统角色与地位（System Role & Position）**

- 层级：**基石层 / Soul-Base**；
- 关系：
  - **soulbase-auth**：在 `check_and_consume` 前后配合授权；返回 `QuotaOutcome`（Allowed/RateLimited/BudgetExceeded）；
  - **soulbase-llm/tools/sandbox/storage/tx**：作为**计量源**；这些模块在完成动作后填报真实用量；
  - **soulbase-interceptors**：入站注入租户与追踪；出站写统一错误公共视图；
  - **soulbase-config**：加载 **配额策略/价目表/留存规则**，支持热更；
  - **soulbase-observe**：输出 QoS 指标与证据（拒绝/降级/超额/归档统计）。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

- **BudgetKey（预算键）**：`{tenant, project?, subject?, resource, action}`
   例：`{tenant:A, subject:u123, resource:soul:model:gpt-4o, action:invoke}`
- **Unit（计量单位）**：`tokens_in / tokens_out / calls / bytes_in / bytes_out / cpu_ms / gpu_ms / storage_gb_day / objects / retries`
- **Window（时间窗）**：`per_min / per_hour / per_day / per_month / rolling(N)`（滚动窗口支持精细配额）
- **Limit（限额）**：`{soft, hard, burst}`（软限触发**节流/降级**，硬限触发**拒绝**；burst 为令牌桶大小）
- **QuotaPolicy**：`BudgetKey` + `Window` + `Limit` + `Priority`（interactive|background）+ `DegradePlan`（降级模型/禁工具）
- **PricingTable（价目表）**：按 provider/model/region/阶梯价定义 **单价**（/1k tokens，/GB·day，/call…）
- **Charge（计费项）**：`{unit, quantity, unit_price, amount_usd, meta}`（meta 含 provider/model/region）
- **Ledger（账页）**：`{tenant, period, line_items[], total_usd, anomalies[]}`（支持日/周/月聚合）
- **RetentionRule**：`{class: hot|warm|cold|frozen, ttl_days, archive_to?, selector}`（按选择器作用于证据/日志/制品/对象存储）
- **Reservation（预留）**：一次调用开始前，依据**预估**用量预留额度；完成后**结算**实用量（差额冲回或追加扣减）
- **Debt（欠额）**：软性透支额度；到期必须补回（用于企业月结/信用期）

------

#### **4. 不变式（Invariants）**

1. **一致口径**：所有单位与账页语义平台唯一；
2. **原子结算**：预留 → 执行 → 结算（差额）在账面一致；失败会**冲销**预留；
3. **默认拒绝**：超过硬限、非法租户/预算键或无价目 → 拒绝；
4. **幂等**：同一 `envelope_id` 的结算**等幂**（重复上报不会重复扣费）；
5. **可追溯**：每次 `Reservation/Debit/Credit/Archive` 都生成 Evidence 与指标；
6. **热更安全**：策略/价目表热更**不影响**已开始调用的结算口径（按调用开始时的快照）；
7. **最小披露**：账页/证据仅存**摘要**（模型/单位/数量/金额），不存原文数据。

------

#### **5. 能力与接口（Abilities & Abstract Interfaces）**

> 具体 Trait/代码在 TD/RIS 落地，这里定义**行为口径**。

- **策略与价目表加载**
  - 从 `soulbase-config` 加载 `QuotaPolicy[] / PricingTable / RetentionRule[]`；暴露 **版本哈希**，供 Evidence 标注；
- **预授权 / 预留**
  - `reserve(ctx: BudgetKey, est: UsageEstimate) -> ReservationHandle | QuotaOutcome`
  - 软限命中 → 返回 `Allowed + DegradePlan`（建议降级）；硬限命中 → `BudgetExceeded`；
- **结算 / 冲销**
  - `settle(handle, actual: UsageActual) -> Charge[]`；失败 → 冲销预留；
  - **等幂**：以 `envelope_id` 防重；
- **直扣**（无需预留的简单单位）
  - `check_and_consume(key, cost: u64) -> QuotaOutcome`（与 `soulbase-auth` 同名接口一致）；
- **账页与对账**
  - `ledger.append(tenant, period, charges[])`；
  - `reconcile(provider_invoices[], sampling_from_observe[]) -> discrepancies[]`（差异阈值与报警）；
- **节流与降级**
  - 提供 `throttle(key, rate)` 与 `DegradePlan` 建议（如：切 `gpt-4o-mini`、禁工具/只读）；
- **留存与归档**
  - 根据 `RetentionRule` 对证据/日志/制品执行 TTL 与分级归档；暴露 `RetentionJob`（可按租户/类目执行）。

------

#### **6. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **指标（最小集）**
  - `qos_reserve_total{tenant,resource,action,outcome}`（Allowed/RateLimited/BudgetExceeded）
  - `qos_settle_usd_total{tenant,resource}`、`qos_units_total{unit}`
  - `qos_throttle_total{tenant,resource}`、`qos_degrade_total{plan}`
  - `qos_reconcile_diff_usd{provider}`（对账差值）
  - `qos_retention_archived_total{class}` / `qos_retention_expired_total{class}`
- **SLO**
  - 预留/检查延迟 p95 ≤ **3ms**（内存快路径）/ ≤ **15ms**（远端存储路径）；
  - 计量与 Provider 账单的误差 **≤ 0.5%**（月维度）；
  - 对账差异报警 **≤ 24h** 内处理；
  - 留存/归档作业日常成功率 **≥ 99.9%**。
- **验收**
  - 契约测试覆盖**软限/硬限/突发桶/降级/幂等结算**；
  - 基准测试覆盖**检查延迟**与在突发 QPS 下的节流正确性。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：`soulbase-config`（策略/价目/留存）、`soulbase-observe`（指标与证据）、`soulbase-errors`（稳定码）
- **下游**：`soulbase-auth/llm/tools/sandbox/storage/tx`（检查/结算调用）、`Soul-Hub`（入口限流）
- **边界**：不发票、不收款；不承担 Provider SDK 直接计量（通过各模块回报）；

------

#### **8. 风险与控制（Risks & Controls）**

- **计量漂移**：与 Provider 账单存在差异 → **双计量**（本地预估 + Provider 回报）+ 对账；
- **透支与雪崩**：软限透支导致成本上升 → **Debt 上限** + 自动降级 + 节流 + 报警；
- **热更抖动**：价目/策略热更引发前后不一致 → **快照结算** + 版本哈希记录；
- **标签爆炸**：预算键维度过细 → 采用**层级合并**与**策略继承**（tenant→project→subject）；
- **隐私合规**：账页/证据仅存摘要，屏蔽敏感值；
- **重放/重复结算**：以 `envelope_id` 作为**幂等锚点**。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 LLM 调用**

1. 内核拟定模型 `m` 与提示：向 QoS `reserve(key={tenant,subject,model m}, est={tokens_in, tokens_out})`；
2. QoS 返回 Allowed/Degrade（建议切更小模型）或 BudgetExceeded；
3. 调用完成：`settle(handle, actual={tokens_in/out})` → 记账 `Charge` → 写 `Ledger` 与指标；
4. 如失败或超时 → 冲销预留（不计费）。

**9.2 工具执行（Sandbox）**

1. 根据 Manifest 估算 `bytes_out`/`bytes_in` 与调用次数 → `reserve`；
2. 执行时实时节流（突发桶），超出 → `RateLimited`（重试或降级只读模式）；
3. 完成后按**实际** bytes/calls 结算。

**9.3 存储/日志留存**

1. 定时任务扫描符合 `RetentionRule` 的对象/证据/日志 → 归档或删除；
2. 写 Evidence 与指标；失败重试并报警。

**9.4 事务与重试（TX）**

1. `outbox` 投递前检查**速率桶**；
2. 失败进入重试 → QoS 记录 `retries` 单位；超过上限 → `BudgetExceeded` → 死信。

------

#### **10. 开放问题（Open Issues / TODO）**

- **信用期/后付费**：Debt/Credit 的完整生命周期（信用评分/冻结/解冻）；
- **多地域价目与汇率**：region/currency 的差分价表与按月汇率换算；
- **精细化成本归因**：跨模块流水的**统一账本**（把 LLM/工具/沙箱/存储/事务成本汇聚）；
- **自适应降级策略**：基于 SLO 偏差与成本阈值的**自动模型路由**与**工具选择**；
- **近实时异常检测**：基于滑窗与异常点检测（e.g. EWM/3-sigma）的成本激增报警。

------

> 本规约与全栈模块**同频共振**：**调用前许可 / 统一记账 / 等幂结算 / 留存归档 / 观测闭环**。若认可，此后将输出 **SB-14-TD（技术设计）**，给出 **Policy/Price/Reserve/Settle/Ledger/Retention** 的 Trait/DTO、突发桶与速率器算法、与各模块的调用时机与热更策略，并随后提供 **SB-14-RIS（最小可运行骨架）**。
