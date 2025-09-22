# 幂等主锚（Issue 02 对齐）

> 在不改动原文结构的前提下，给出 SB-10 的幂等统一约定与实施要点。

## 统一约定

- 主锚：`Envelope.envelope_id` 为 Outbox/去重/补偿/回放 的唯一主锚。
- 次级：`Idempotency-Key`、`(channel, seq)` 仅作二级校验/提示；不作为跨模块去重主键。

## Outbox 与去重

- OutboxMessage 已含 `envelope_id` 字段；去重表/唯一索引请以 `envelope_id` 为第一主键维度（含 `tenant`）。
- 重放/重试的判断以 `envelope_id` 为主；可结合 `seq/nonce` 或摘要作为安全冗余。

## Idempotency Store 指南

- 接口建议：
  - `check_and_put(envelope_id, hash)` → 若不存在则插入并返回可执行；
  - `finish(envelope_id, result_digest)` → 终结状态并记录结果摘要；
  - 如需兼容 `Idempotency-Key`，应先映射/绑定到 `envelope_id` 再执行。

## 验收

- 同 `envelope_id` 的重复调用不产生重复 Outbox 投递与副作用；
- “重试 3 次仅 1 次生效”合同测试通过；
- 观测中可见 `dedup_hits` 与重试次数一致。
