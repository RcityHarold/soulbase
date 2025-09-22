# Config 快照贯穿（Issue 04 对齐）

> 在不改变原文结构的前提下，明确配置快照的固化、传播与验证，保证 Evidence/账页/回执 与 响应头一致携带 `config_version/hash`。

## 统一原则

- 入站固化：在请求进入拦截面后，由 Config Loader 解析并固化当前有效快照，得到 `config_version/hash`。
- 全链路贯穿：固化值应贯穿到 Evidence、Ledger（账页）、Receipt（回执）与出站响应头。
- 响应头规范：`X-Config-Version: <version>`，`X-Config-Checksum: <hash>`。

## 实施要点

- Loader/SnapshotSwitch：明确定义快照的选择规则与生效边界（灰度/租户/路由策略）。
- 与拦截器集成：SB-05 的 `response_stamp` 阶段统一写入 `X-Config-*` 头；
- 与执行层集成：SB-06/08 产生的 Evidence Begin/End 记录 `config_version/hash`；
- 与账页/回执集成：SB-14 账页行、SB-15 A2A 回执结构中包含 `config_version/hash` 字段。

## 验收

- 热更期间：同一请求的 Evidence/账页/回执/响应头四处 `config_version/hash` 保持一致；
- 旧请求不漂移：快照在处理开始后不受后续配置变更影响；
- 合同测试：覆盖“热更中请求不漂移”和“X-Config-* 存在且与 Evidence 一致”。
