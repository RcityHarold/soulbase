# Auth×QoS（Issue 03 对齐）

> 在不改变原文结构的前提下，明确“授权判定与配额扣减一体化”的统一约定，避免重复扣额与职责重叠。

## 统一决策流

- 入口统一：`AuthFacade.authorize(subject, scope, attrs?, qos_estimate?) -> Decision`。
- 内聚逻辑：在 `authorize(..)` 内部调用 `QosFacade.check_and_reserve/consume`，对外仅返回单一决策：
  - `Allow { reservation?, obligations?, cache_ttl_ms? }`
  - `RateLimited { degrade_plan?, retry_after? }`
  - `BudgetExceeded { reason, alt_suggestions? }`
- 保证：每个请求最多发生**一次**配额扣减/预留；后续阶段（Tools/Net）不得再次“次数扣减”。

## 参数与证据

- `qos_estimate` 可选，用于预估 tokens/calls/bytes；若未知，可走最小保守策略。
- 证据：`Decision.evidence` 记录 `policy_version/hash`、估算与实际对账所需的关键字段。

## 契约与验收

- 契约：
  - 端到端每请求仅一次扣减；
  - 降级路径可观测（RateLimited→DegradePlan）。
- 验收：合同测试包含“双重扣额”保护用例；/metrics 可见 `auth_allow/deny` 与 `qos_reserve/settle` 的一致性。

---

> 对应模块：SB-14 提供 `QosFacade` 的 `reserve/settle` 语义；SB-05 在 `authz_quota` 阶段仅调用 `AuthFacade.authorize(..)` 并按返回值分支，后续不得重复扣额。
