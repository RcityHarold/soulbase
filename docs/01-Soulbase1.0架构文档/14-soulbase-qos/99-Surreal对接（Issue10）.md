# Surreal 对接（Issue 10 对齐）

> 账页（Ledger）在 Surreal 的表结构/索引与一致性要求，统一去重与聚合口径。

## 表与索引（示例）

- `ledger{tenant, envelope_id, line_kind, amount, currency, usage, policy_hash, config_version, config_hash, created_at}`
- 索引：
  - `UNIQUE(tenant, envelope_id, line_kind)`（避免重复账务）；
  - `INDEX(tenant, period)`（分期聚合可选）；

## 一致性

- 去重主锚：`envelope_id`（见 Issue 02）；
- 聚合口径：同一 `envelope_id` 的重试/重放应聚合到同一账务视图；
- 快照：记录 `config_version/hash`（见 Issue 04）。

## 验收

- 唯一约束避免重复账务；
- 聚合/对账与回执一致；
- 指标可按 `config_version` 分维度聚合。
