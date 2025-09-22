# 幂等主锚（Issue 02 对齐）

> 本附录在 SB-19 中明确 `X-Env-Id` 注入与幂等重试的主锚关系。

## `X-Env-Id` 注入

- 出站请求默认通过拦截器将 `Envelope.envelope_id` 注入到请求头 `X-Env-Id`；
- 若上游无 Envelope，可按策略生成临时 `envelope_id`（仅用于链路关联，不参与账页/收据）；
- 与 `Trace/UA` 同级，建议在 `TraceUa` 或独立 `EnvId` 拦截器中实现。

## 幂等重试

- 仅对幂等方法（GET/HEAD/OPTIONS/部分 PUT/DELETE）或显式声明幂等的 POST 启用重试；
- 与 SB-10/15 的主锚一致：在端到端调用中以 `envelope_id` 作为幂等与去重的主锚。

## 验收

- 抓包可见 `X-Env-Id`；
- 端到端“重试 3 次仅 1 次生效”合同测试通过；
- 与 SB-10/15 的去重策略一致。
