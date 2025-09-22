# sb-auth

统一认证 / 授权 / 配额 SPI：负责令牌映射、策略决策、同意校验、配额治理与决策缓存。

## 构建

- cargo check
- cargo test

## 示例

参考 crates/sb-auth/tests/basic.rs，演示 StaticTokenAuthenticator + StaticPolicyAuthorizer + MemoryQuotaStore 组合的授权流程。
