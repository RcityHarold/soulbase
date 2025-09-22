# Auth×QoS（Issue 03 对齐）

> 对拦截器阶段职责进行明确：入口统一授权与配额判定，仅扣一次。

## 阶段约定

- `authz_quota` 阶段仅调用 `AuthFacade.authorize(..)`，依据其返回：
  - `Allow` → 继续；可携带 `reservation/degrade_plan/obligations`；
  - `RateLimited/BudgetExceeded` → 返回公共错误视图（SB‑02）。
- 后续阶段（`schema_guard` / `tools` / `net`）不得再次进行“次数扣减”；仅上报用量或做 `settle/metrics`。

## 观测与错误

- 观测记录：`auth_allow_total/auth_deny_total{code}` 与 `qos_reserve/settle` 指标；
- 出错统一走 SB‑02 公共视图；
- 双重扣额保护用例：若检测到重复扣减，应返回 `UNKNOWN.INTERNAL` 并报警。
