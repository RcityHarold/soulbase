# SB-10 ·  开发总结（阶段性记录）

> 2025-02-13 · 基于当前仓库状态（含配置驱动、指标钩子、QoS 预算守卫、HTTP/Kafka 传输骨架、后台调度器等能力）。

## 已完成能力概览

- **配置与运行参数**： 支持 outbox/saga/idempotency/dead-letter/worker 配置，默认值下已可直接运行；运行时可从  快照加载并热更新。
- **指标与预算钩子**： +  双抽象接入  和内存/Surreal store；成功/失败/死信均打点，同时支持限流、窗口计数、并发控制。
- **A2A 钩子占位**： 在 dead-letter / replay 路径中调用，为后续 ledger 签名、对账落地预留接口。
- **多传输通道**：提供内置 （默认启用）及可选 （feature ）；HTTP 传输含 loopback UT，结构可扩展 header、超时等参数。
- **后台调度器**： 周期调用 ； 根据配置清理老化 dead-letter，并支持组合式  启动/停机。
- **Surreal / 内存双实现**：Outbox/Saga/Idempotency/DeadLetter 均有内存实现及 Surreal 适配；新增  脚本与维护接口（含 ）。
- **测试覆盖**：、 已纳入配置、指标、传输与 worker 路径的单测/集成测试。

## 未完成 / 待外部信息支持

| 模块 | 缺口 | 依赖信息 |
| ---- | ---- | -------- |
| Kafka Transport | 仅完成基础发送；尚未串联消费端回执/幂等对账与 e2e 验证 | Kafka brokers、topic 约定、回执协议 |
| A2A Ledger | 目前  为 Noop；未落地签名、账本写入、对账流程 |  签名算法、账本字段、对账周期 |
| Saga / 租约守护 | Worker 仅包含 Outbox tick & 死信清理；Saga 超时、租约续期、死信策略待补 | Saga 心跳/超时策略、持久化字段、运行限制 |
| 契约 / 回放测试 | 尚无契约断言与 replay 脚本 | 契约/fixtures、预期输出 |

## 使用说明（当前阶段）

1. **加载配置与预算守卫**
   
2. **创建 Store**
   - 内存：
   - Surreal：，并执行 
3. **启动后台 Worker**
   
4. **HTTP Transport**
   
   请求默认发送 JSON body，topic 必须是  URL。

## 后续建议

1. 获取 Kafka 集群 / 回执接口规范，在现有骨架上补齐生产者确认、消费端回执和幂等自测。
2. 结合  定义签名/账本协议，实现  的实际写入与反重放校验。
3. 扩展 Saga worker：租约续期、超时补偿、阶段性死信策略全部纳入配置，确保与 runtime 集成。
4. 搭建契约测试（Contract Testkit）与 replay 脚本，保障幂等、预算、死信等路径的可重复验证。

> 若需继续推进以上缺口，请提供 Kafka/A2A/Saga/契约的外部接口或测试环境信息。
