# 05-soulbase-interceptors · 开发总结

## 1. 当前实现概述
- sb-interceptors crate 已实现完整的“请求阶段 + 响应阶段 + 异常应答”链路，统一调度认证、授权、幂等、Schema 校验、义务执行与响应戳记能力。
- 支持多协议接入：HTTP（Axum/Tower）、gRPC（Tonic JSON 包装）与 MQ 消息适配，并通过 ProtoRequest / ProtoResponse 抽象保持协议无关。
- 与基石模块联动：
  - sb-config：ContextInitStage 支持注入 ConfigSnapshotProvider，自动透传 X-Config-Version/Checksum。
  - sb-auth：Authn/Authz/Quota + Consent Base64(JSON) 解析完成一体化授权与预算扣减。
  - sb-errors：DefaultErrorResponder 将所有错误规范化为公共视图，自动写入 HTTP/gRPC/MQ 响应与观测标签。
  - sb-types：统一 Envelope/Trace/Tenant 语义。
- 提供 ResiliencePolicy（超时/重试/并发限制）与 IdempotencyLayer（幂等缓存 + 重放短路）等基础韧性设施，并在 InterceptorChain 级别启用。

## 2. 已完成功能
- ContextInitStage：生成或采纳 X-Request-Id、TraceContext、租户头，支持 ConfigSnapshotProvider，初始化 EnvelopeSeed 与配置戳记。
- RoutePolicyStage：解析静态 DSL（method/path/topic → Resource/Action/Attrs），支持声明请求与响应 Schema。
- SchemaGuardStage：接入 JSON Schema（feature schema-json），请求与响应均可校验，失败返回 SCHEMA.VALIDATION_FAILED。
- IdempotencyStage + 链路存储：处理 Idempotency-Key 去重、短路重放、缓存响应（带 TTL、尺寸限制，并输出 X-Idempotent-Replay 头）。
- ResiliencePolicy：在链路层实现可配置的超时（默认 10 秒）、RetryClass::Transient 重试与并发信号量限制。
- AuthnMapStage / AuthzQuotaStage：整合 Bearer 身份映射、租户一致性校验、Consent Base64(JSON) 解析、AuthFacade 授权与配额扣减。
- ObligationsStage：执行 mask / redact / watermark 义务，失败时返回策略拒绝码。
- ResponseStampStage：统一落 X-Request-Id / X-Trace-Id / X-Config-* / X-Obligations / X-Idempotent-Replay。
- DefaultErrorResponder：捕获请求阶段或链路异常，输出 soulbase-errors 公共视图、HTTP 状态，并把标签写入 cx.extensions 供观测面消费。
- 协议适配：HTTP（Axum/Tower）、gRPC（Tonic JSON 包装）、MQ 消息请求。
- 观测接口：observe::error_labels 生成标准化错误指标标签。
- 测试矩阵：cargo test -p sb-interceptors；cargo test -p sb-interceptors --features schema-json（覆盖鉴权通过、缺失凭证、幂等重放、Schema 校验失败等路径）。

## 3. 功能现状
模块已按 SB-05 功能规约全部落地：错误规范化、JSON Schema 校验、韧性治理、幂等缓存、Consent 融合、gRPC/MQ 适配、配置戳记与观测标签均已实现，可直接作为 Soul-Hub / SoulseedAGI 的统一拦截层复用。

## 4. 后续可选增强
1. Schema 缓存：当前验证时即时编译，可按需结合 sb-config 热更新缓存编译结果。
2. Protobuf Schema：扩展 schema-pb 后端，实现 Protobuf/gRPC 字段级校验。
3. Resilience 指标化：与即将落地的 sb-observe 对接，输出超时/重试/并发指标。
4. 策略热更新：通过 sb-config DSL 或远程策略中心，支持 RoutePolicySpec 灰度与热更新。

> 结论：soulbase-interceptors 模块已具备生产可用的请求治理、幂等、防护与响应规范化能力，可作为 Soul 平台各接口的统一拦截链直接启用。
