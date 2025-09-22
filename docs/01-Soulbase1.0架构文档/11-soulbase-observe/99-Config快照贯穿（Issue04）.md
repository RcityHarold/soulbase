# Config 快照贯穿（Issue 04 对齐）

> 明确指标/日志/Trace 对配置快照的暴露与白名单原则。

## 观测标签

- 白名单标签新增：`config_version`（短字符串）；
- 不暴露 `config_hash` 全值（避免高基数与泄露），仅在 Evidence/账页/回执中存储；
- 错误码覆盖率、SLO 等指标按 `config_version` 可分维度聚合。

## 验收

- /metrics 可见 `config_version` 维度指标；
- 日志/Trace 不出现完整 hash 原文；
- 与 SB-03/05/06/14/15 的快照记录一致。
