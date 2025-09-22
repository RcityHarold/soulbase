# Tools × Sandbox（Issue 05 对齐）

> 明确工具在预检与执行阶段对 Sandbox 基线的协同：声明能力、校验 Consent、透传并落证。

## 预检（Preflight）

- 工具在注册/调用前提供 Manifest：声明所需能力与资源上限（method/host 白名单、最大响应体积、超时、CPU/内存上限等）。
- Preflight：
  - 校验 Manifest 与租户/策略（Policy）一致；
  - 若涉及高风险能力（fs.write/process.spawn/net.http.post 等），必须检查并要求有效 `Consent`；
  - 失败时返回 `SANDBOX.PERMISSION_DENIED` 或 `SANDBOX.CAPABILITY_BLOCKED`（按策略/阈值）。

## 执行

- 调用时将 Manifest 与 `Consent` 透传给 Sandbox；
- 由 Sandbox 执行 SSRF/路径/资源上限等硬性校验；
- 仅在放行后执行实际逻辑；Evidence 记录能力/参数摘要/资源用量与 `config_version/hash`。

## 验收

- 未声明能力的调用 → 被拒；
- 缺失或过期 Consent 的高风险操作 → 被拒；
- 超过 Manifest 的资源上限 → 被终止并返回 `SANDBOX.CAPABILITY_BLOCKED`；
- 观测指标可见放行/拒绝与原因分布。
