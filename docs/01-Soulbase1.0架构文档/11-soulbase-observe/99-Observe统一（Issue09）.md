# Observe 统一（Issue 09 对齐）

> 在不改变原文结构的前提下，明确标签白名单、/metrics 暴露与覆盖率指标，统一观测口径并控制卡方风险。

## 标签白名单（建议）

- 错误相关：`code`、`kind`、`retryable`、`severity`
- 路由相关：`route_id`、`action`（read|write|invoke|admin|list|configure）
- 网络相关：`method`、`scheme`、`phase`（connect|ttfb|total）
- 缓存/幂等：`cache`（hit|miss|stale）、`idempotent`（true|false）
- 结果分箱：`status_class`（2xx|3xx|4xx|5xx）、`outcome`
- 可选维度（默认关闭，需评审开启以避免高基数）：`tenant`

说明与约束：
- 非白名单标签一律拒绝（编译期/单测或 CI 合同校验）。
- 严禁使用高基数或用户输入直出的标签（如 subject_id、path 原文、url 原文、hash 原文）。
- 建议以枚举/常量集中注册标签键，方便全局 grep 与审计。

## /metrics 暴露与基线指标

- Prometheus：暴露 `/metrics`；OpenTelemetry：提供 OTLP 导出开关。
- 直方图/计数器（示例）：
  - `req_latency_ms_bucket{route_id,action}`、`active_requests{route_id}`
  - `errors_total{code,kind,retryable,severity}`（来自 SB‑02 错误公共视图）
  - `net_latency_ms_bucket{phase,method}`、`net_requests_total{method,scheme,status_class}`
  - `cache_requests_total{cache}`、`cache_age_ms_bucket{route_id}`
  - `qos_reserve_total{outcome}`、`qos_settle_total{outcome}`

## 覆盖率与质量指标

- 错误码覆盖率：`error_code_coverage = known_coded_errors / all_errors ≥ 99.9%`；
  - 未映射/拼写错误视为 unknown，应在 CI 中拒绝；
- 标签合规率：`label_whitelist_violation_total == 0`；
- 采样与限流：为热点指标提供采样/聚合策略，避免卡方膨胀。

## 仪表板与告警（建议）

- 基础仪表板：请求延迟/吞吐、错误码分布、网络相位、缓存命中、QoS 决策、幂等去重命中；
- 告警：
  - error_code_coverage 低于阈值；
  - 5xx 比例异常；
  - 幂等重试放大（重试次数超阈）；
  - 缓存击穿/陈旧值异常；
  - QoS 限流/超额反常上升。

## 验收

- /metrics 可用；
- 仅出现白名单标签（或开启的可选维度）；
- 错误码覆盖率 ≥ 99.9%；
- 仪表板能展示基础健康与异常定位所需视图；
- 与 SB‑02/05/16/19 的字段命名对齐，无冲突。
