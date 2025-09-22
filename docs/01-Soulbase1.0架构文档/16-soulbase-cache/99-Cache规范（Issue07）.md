# Cache 规范（Issue 07 对齐）

> 在不改变原文结构的前提下，补充键规范、SWR 只读策略、写后强失效与观测验收，统一与 Storage/Tx/Net 的接线方式。

## 键规范（Key Schema）

- 统一格式：`{tenant}[:{subject|roles_hash}]:{namespace}:{policy_hash}:{resource}:{key}`
- 说明：
  - `tenant` 必填；`subject|roles_hash` 任选其一，用于权限隔离；
  - `namespace` 用于业务域或表名/模型名；
  - `policy_hash`（可选）用于策略/过滤口径变化时批量失效；
  - `resource/key` 表达最终业务键；
- 要求：
  - 不得包含高基数字段原文（仅摘要）；
  - 同租户不同主体默认隔离；
  - 文档化 key 生成函数，便于统一 grep 与失效。

## TTL 与 SWR（只读路径）

- SWR 仅用于**只读**路径（如 GET/查询接口）；
- TTL 由数据新鲜度与成本共同决定；
- SWR 下游接口必须能接受陈旧值 + 后台刷新（不影响一致性操作）。

## 写后强失效（Write‑Invalidate）

- 触发来源：
  - Storage 变更事件（Insert/Update/Delete/Migration）；
  - Tx 提交回调（Outbox/Saga 成功提交）；
- 失效策略：
  - 以 `tenant/namespace/resource` 为粒度计算待失效的 key 前缀（或维护反向索引）；
  - 失败重试与去重（Idempotent）；
- 要保证“先写后删缓存”，避免短暂脏读；
- 对幂等写，多次失效调用应为无害操作。

## API 建议

- `get_or_load(k, loader, ttl, swr?) -> value + metrics`（单飞/抑制击穿）；
- `do_once(k, ttl, f) -> value`（幂等防抖）；
- `invalidate(prefix|keys[]) -> count`；
- `subscribe_invalidation(events)`（对接 Storage/Tx）。

## 观测与验收

- 指标：`cache_hit_ratio{tenant,ns}`、`stale_served_total`、`invalidation_total`、`age_ms`；
- 验收：
  - “先读后写”不出现脏读；
  - SWR 仅在只读路径；
  - 租户隔离到位；
  - 写后强失效在 Tx/Storage 事件发生时及时触发。
