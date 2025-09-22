# A2A 对接（Issue 12 对齐）

> A2A 消息与 Outbox 的统一出/入站与可靠性约定，配合反重放与收据链。

## 出/入站

- 出站：`outbox.enqueue_in_tx(topic=peer/channel, payload)`，COMMIT 后 Dispatcher 发送；
- 入站：持久化接收事件，签名验证通过后生成回执 `Receipt` 并写账页；
- 去重主锚：`envelope_id`（见 Issue 02）。

## 失败/补偿

- 发送失败：`attempts/not_before` 重试；多次失败入 Dead‑Letter；
- 入站失败：签名或窗口校验失败 → `A2A.SIGNATURE_INVALID`/`A2A.REPLAY`；
- 回执对账：与 SB‑14 账页一致；

## 验收

- 出/入站可靠性验证；
- 反重放拒绝；
- 收据链回放一致；
- Dead‑Letter 可补偿。
