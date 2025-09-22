# 幂等主锚（Issue 02 对齐）

> 本附录明确 SB-15 在反重放、收据链上的主锚与窗口策略。

## 反重放与收据链

- 主判定键：`Envelope.envelope_id` 为反重放与收据链的主锚；
- 辅助窗口：`seq/nonce` 作为时间窗或乱序保护的辅助指标；
- 收据链：`Receipt` 应链至 `envelope_id`，并在双方账页与证据内可追溯。

## 与 Outbox 的一致性

- A2A 出/入站消息入 Outbox/Tx 时，以 `envelope_id` 为去重键保持与 SB-10 一致；
- 二级锚（`Idempotency-Key`、`(channel,seq)`）仅用于通道内的幂等提示，不跨域作为主键。

## 验收

- 重放请求被拒绝并记录为 `A2A.REPLAY`；
- 双方对同一 `envelope_id` 的收据链达成一致；
- 账页与证据中均以主锚对齐。
