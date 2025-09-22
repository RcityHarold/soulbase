# SB-08 · sb-tools 开发总结
## 1. 当前完成情况
- Manifest 层：实现 `ToolManifest` SemVer 版本、Consent scope 提示、CompatMatrix、能力/Scope 对齐及 JSON-Schema 编译校验，确保声明默认拒绝、最小权限。
- Registry 层：在 `InMemoryRegistry` 中记录 policy_hash、配置指纹、LLM 可见性，新增 `update_policy` / `update_config_fingerprint` / `visible_only` 过滤，支撑热更新与多渠道准入。
- Preflight 编排：`PreflightService` 串联 Schema 校验、Idempotency-Key 强制、Consent/授权/配额判定、ConfigSnapshot 指纹透传，并生成 Planned Ops + ProfileHash + 预算快照。
- Invocation 编排：`InvokerImpl` 管理并发与幂等、复用 Planned Ops、聚合 Sandbox 预算/副作用、执行 obligations、生成 output/args 摘要；任何结果都产出 `ToolInvokeBegin/End` 事件与指标记录。
- 观测与事件：提供 `ToolEventSink`、`ToolMetrics` 默认实现，方便上层接入 sb-observe 或自定义监控；主流程示例在 `crates/sb-tools/tests/basic.rs` 验证“注册→预检→执行→幂等命中”。

## 2. 后续可选增强
1. 接入真实 Auth/QoS/Config Loader，实现生产级降级策略与预算回扣。
2. 扩展 `ToolMetrics` / `ToolEventSink` 与 sb-observe、EvidenceSink 的联动，覆盖更多标签与指标。
3. 丰富 obligations 策略（脱敏、水印、字段映射），并在 contract-testkit 中补充契约用例。

## 3. 参考文件
- 核心实现：`crates/sb-tools/src`
- 示例测试：`crates/sb-tools/tests/basic.rs`
