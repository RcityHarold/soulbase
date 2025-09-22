# Auth×QoS（Issue 03 对齐）

> 在不改变原文结构的前提下，明确与 Auth 的集成边界与职责切分。

## QosFacade（建议接口）

- `reserve(key, est) -> Reservation { handle, ttl_ms }`
- `consume(key, qty) -> Outcome { allowed|limited|exceeded, degrade_plan? }`
- `settle(handle, actual) -> ()`
- `refund(handle) -> ()`（失败回滚可选）

说明：`key = {tenant, subject?, resource, action}`；`est/actual` 支持 `tokens/calls/bytes/...`。

## 与 Auth 的内聚

- `AuthFacade.authorize(..)` 内部调用 `check_and_reserve/consume` 实现入口唯一扣减；
- 返回 `Allow/RateLimited/BudgetExceeded` 三态，携带 `degrade_plan/reservation`；
- 后续阶段仅做 `settle` 与统计，不得重复“次数扣减”。

## 验收

- 单请求仅一次扣减；
- 降级路径可观测；
- Ledger 与 `reserve/settle` 一致；
- 可通过 Contract-TestKit 的双重扣额保护用例校验。
