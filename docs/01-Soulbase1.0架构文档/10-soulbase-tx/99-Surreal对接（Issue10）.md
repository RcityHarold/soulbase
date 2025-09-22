# Surreal 对接（Issue 10 对齐）

> 明确 Outbox/Idempotency 的表结构/索引/事务顺序，统一与 Surreal 的落地与验收口径。

## 表与索引（示例）

- `outbox{tenant, envelope_id, topic, payload, not_before, attempts, status, last_error, created_at}`
  - `UNIQUE(tenant, envelope_id)`；
  - `INDEX(tenant, status, not_before)` 便于轮询；
- `idempo{tenant, envelope_id, hash, status, result_digest, ttl, updated_at}`
  - `UNIQUE(tenant, envelope_id)`；

## 事务顺序

1. 业务写入（含幂等检查：`idempo.check_and_put(envelope_id, hash)`）；
2. 同一事务写入 Outbox 记录；
3. COMMIT 成功；
4. 触发缓存失效（Issue 07）；

## 失败与重放

- Outbox 失败重试按 `attempts/not_before` 控制；
- 幂等幂锚是 `envelope_id`（见 Issue 02）；
- 通过唯一约束与状态机避免重复投递；

## 验收

- 幂等写 + Outbox 提交整体可回放；
- 重试 3 次仅 1 次生效（去重命中）；
- 唯一约束生效；
- 与 SB‑09/14 的账页与去重一致。
