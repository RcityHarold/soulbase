# 《SB-02 · sb-errors 开发总结》

## 1. 开发范围与完成度
- 已完成 sb-errors crate，提供统一错误域、稳定错误码注册表与跨协议映射能力。
- 定义 ErrorKind、RetryClass、Severity、ErrorObj、ErrorBuilder 等核心结构，支持公共/审计视图、观测标签与可选 wrap-* 封装。
- 工程已纳入工作区 Cargo.toml，并与 sb-types 形成完整依赖。

## 2. 关键实现亮点
- src/code.rs 集中注册码表，新增 Issue01 所列 Tx/A2A/Sandbox 码位（例如 TX.TIMEOUT、A2A.CONSENT_REQUIRED、SANDBOX.CAPABILITY_BLOCKED），并同步维护 HTTP/gRPC 映射及重试、严重度标签。
- src/model.rs 构建 ErrorObj/Builder，实现最小披露、公私视图拆分、因果链、Meta、回退提示等扩展字段。
- src/render.rs 与 src/labels.rs 输出公共视图和观测标签，确保 to_public 脱敏；src/mapping_http.rs、src/mapping_grpc.rs 提供协议映射；src/wrap.rs 在启用特性时统一 reqwest/sqlx 等外部错误分类。

## 3. 测试与验证
- tests/basic.rs 覆盖错误构造、公私视图、观测标签及新增码位的注册信息。
- cargo test 全量通过，包含 sb-types 与 sb-errors 的单元、集成及文档测试。

## 4. 未决事项与建议
- 后续可扩展 wrap-llm 等特性覆盖更多第三方错误，并在 sb-contract-testkit 中增加契约用例。
- 结合 98-工具（grep 未知错误码）添加 CI 门禁，发布时同步 Schema/码表变更记录。
- 与 sb-interceptors、sb-observe 对接时补充 HTTP/gRPC 响应示例及指标白名单校验。
