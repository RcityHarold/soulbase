# 《SB-04 · sb-auth 开发总结》

## 1. 本轮完成情况
- 落地 sb-auth crate：实现认证/授权/配额/同意/缓存的 SPI 框架，并提供静态示例适配器（StaticTokenAuthenticator、StaticPolicyAuthorizer、MemoryQuotaStore、BasicConsentVerifier、MemoryDecisionCache）。
- AuthFacade 串联认证→属性补充→PDP 决策→同意校验→配额扣减→决策缓存，返回 Decision 并预留事件/指标挂钩。
- model.rs 定义资源 URN、动作、授权请求、决策/Evidence、决策键/配额键与属性合并；errors.rs 提供与 sb-errors 对齐的标准错误。
- events.rs / observe.rs 提供审计事件封装与指标标签；tests/basic.rs 覆盖授权成功（含缓存命中）与配额超限拒绝场景，cargo test 全量通过（含其他基础 crate）。

## 2. 后续落地事项（下一阶段）
1. 认证对接 Soul-Auth：实现 OIDC/JWT 验签、JWKS 轮换、API Key/mTLS/Service Token 适配器及撤销处理。
2. PDP 适配：补齐 OPA/Cedar/自研 PDP 客户端，输出策略版本哈希与规则证据，完善 deny-by-default 行为。
3. 配额扩展：提供分布式 QuotaStore（如 Redis），实现原子扣额、限流窗口，与 sb-qos 协调成本度量。
4. 同意校验强化：接入 Soul-Auth 下发的签名 Consent、scope 匹配及高风险二次确认策略。
5. 缓存撤销与事件：完善决策缓存失效通道、输出 AuthDecisionEvent/QuotaEvent，并与 sb-observe 集成指标。
6. 契约测试：在 sb-contract-testkit 中覆盖 PDP 输入/输出契约、错误码映射、缓存 TTL 与撤销、配额状态机等场景。

## 3. 参考文件
- 实现：crates/sb-auth/src
- 测试：crates/sb-auth/tests/basic.rs
- 事件：crates/sb-auth/src/events.rs
