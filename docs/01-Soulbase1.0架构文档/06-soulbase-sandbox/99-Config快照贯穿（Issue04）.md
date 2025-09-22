# Config 快照贯穿（Issue 04 对齐）

> 明确受控执行证据（Begin/End）的快照记录要求。

## 证据要求

- 在 `Sandbox.run(..)` 的 Begin/End Evidence 中记录 `config_version/hash`；
- 若执行期间发生配置热更，当前任务仍以开始时固化的快照为准；
- 观测标签（如有）使用白名单字段暴露 `config_version`，避免高基数。

## 验收

- 任一工具执行 Evidence 的 `config_version/hash` 与请求响应头一致；
- 热更期间，正在执行的任务不漂移；
- 负向用例：若缺失快照信息，返回 `SCHEMA.VALIDATION_FAILED` 或内部校验告警。
