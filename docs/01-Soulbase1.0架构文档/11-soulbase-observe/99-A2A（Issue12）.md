# A2A 观测（Issue 12 对齐）

> 统一 A2A 的签名/反重放/收据链的指标与错误视图口径。

## 指标建议

- `a2a_sign_ms_bucket{alg}`、`a2a_verify_ms_bucket{alg}`；
- `a2a_replay_block_total{reason=window|duplicate}`；
- `a2a_outbox_total{status=enqueued|dispatched|failed}`；
- `a2a_receipt_total{status=ok|mismatch}`；
- 标签白名单：`alg`,`kid`（可选，默认关闭防高基数）、`code`、`outcome`。

## 错误公共视图

- `A2A.SIGNATURE_INVALID`、`A2A.REPLAY`、`A2A.CONSENT_REQUIRED`、`A2A.LEDGER_MISMATCH`；
- 仅公共视图对外；审计信息与证据入观测。

## 验收

- 指标可见签名/验签、出入站、反重放与收据链各环节；
- 错误映射统一；
- 标签合规且不过度高基数。
