# Surreal 对接（Issue 10 对齐）

> A2A 反重放/收据链与 Outbox 的落地结构与唯一性约束。

## 表与索引（示例）

- `a2a_replay{tenant, envelope_id, seq, nonce, ts}` + `UNIQUE(tenant, envelope_id, seq)`；
- `a2a_receipt{tenant, envelope_id, receipt_hash, policy_hash, config_version, config_hash, created_at}` + `UNIQUE(tenant, envelope_id, receipt_hash)`；

## 一致性

- 反重放：以 `envelope_id` 为主锚，`seq/nonce` 为窗口辅助（见 Issue 02）；
- Outbox：出/入站消息统一写入 SB‑10 Outbox；
- 快照：回执记录 `config_version/hash`（见 Issue 04）。

## 验收

- 重放被拒；
- 收据链可复核；
- 与 SB‑10/14 的 Outbox/账页一致。
