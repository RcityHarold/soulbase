# A2A 生产化（Issue 12 对齐）

> 在不改变原文结构的前提下，补充 JWS/COSE、Outbox 可靠投递与反重放窗口的统一约定，与 SB‑18/10/11/14 对齐。

## 签名与验签（默认 JWS Detached）

- 规范：默认 `JWS Detached + Ed25519`（可选 COSE_Sign1）；
- 格式：`kid/alg` 必填，`kid` 规范为 `ed25519:yyyy-mm:keyId`；
- Canonical：对消息体使用 Canonical JSON；
- 证据：记录 `kid/alg`、`payload_digest`（sha256）与 `envelope_id`；
- 安全：恒时比较 + zeroize；撤销与轮换通过 KeyStore（见 SB‑18）。

## Outbox 可靠投递（统一出/入站）

- 发送侧：A2A 消息入 SB‑10 Outbox（topic=peer/channel），成功 COMMIT 后由 Dispatcher 发送；
- 接收侧：持久化接收事件与回执（receipt），并以 `envelope_id` 去重；
- 幂等：以 `envelope_id` 为主锚，重复投递/重试不得重复生效；
- 死信：多次失败进入 Dead‑Letter 队列，后续人工或自动补偿。

## 反重放与收据链

- 反重放：`envelope_id` 为主锚，`seq/nonce + 窗口` 为辅助；
- 收据链：`Receipt{envelope_id,request_digest,result_digest,usage,code}` 由接收侧签名返回；
- 双边一致：双方账页/回执链在 `envelope_id` 维度达成一致；
- 窗口：默认 5 分钟可配置，过期拒绝并上报 `A2A.REPLAY`。

## 错误与公共视图（SB‑02）

- `A2A.SIGNATURE_INVALID`、`A2A.REPLAY`、`A2A.CONSENT_REQUIRED`、`A2A.LEDGER_MISMATCH`；
- 仅对外公共视图；审计信息写入观测。

## 验收

- 跨域验签 100% 通过；
- 重放被拒并记录 `A2A.REPLAY`；
- 双边账页/回执一致；
- Outbox 出/入站链路可回放；
- 失败进入 Dead‑Letter 并可人工/自动补偿。
