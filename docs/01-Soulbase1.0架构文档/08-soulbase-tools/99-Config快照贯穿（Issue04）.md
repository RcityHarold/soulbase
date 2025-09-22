# Config 快照贯穿（Issue 04 对齐）

> 明确工具预检与调用结果的快照记录要求。

## 预检与调用

- Preflight 阶段确定并携带 `config_version/hash`；
- Tool 调用结果的 Evidence 应记录 `config_version/hash` 与 `tool/version`、`args_digest` 一致性；
- 与 SB-05/06 协同：由拦截器与 Sandbox 提供快照上下文，工具侧透传并落证。

## 验收

- 工具 Evidence 与响应头中的 `X-Config-*` 一致；
- 热更期间，预检后的调用不漂移；
- 合同测试：覆盖“Preflight 建立快照后执行阶段不受新配置影响”。
