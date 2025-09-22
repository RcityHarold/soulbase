# A2A 加密与签名（Issue 12 对齐）

> 明确 A2A 的 JWS/COSE、KeyStore/轮换/撤销、Canonical JSON、证据记录与指标口径。

## JWS/COSE 与 Canonical

- 默认 `JWS Detached + Ed25519`；可选 COSE_Sign1；
- 对消息体做 Canonical JSON（JCS‑like）；
- 证据记录：`kid/alg/payload_digest`（sha256）与 `envelope_id`；
- 验签要求：恒时比较；撤销列表校验；生效期检查。

## KeyStore/轮换/撤销

- KeyStore 提供 `get(kid)`、`rotate(policy)`、`revoke(kid)`；
- `kid` 规范：`ed25519:yyyy-mm:keyId`；
- JWKS/本地/远程 KMS 多实现；
- 指标：`crypto_key_rotate_total`、`crypto_key_revoke_total`。

## 错误映射（SB‑02）

- 验签失败/撤销/过期/算法不允许 → `A2A.SIGNATURE_INVALID`；
- 规范化失败/不支持的数值/非法输入 → `SCHEMA.VALIDATION_FAILED`；
- KMS/KeyStore 异常 → `PROVIDER.UNAVAILABLE`；
- 其它 → `UNKNOWN.INTERNAL`。

## 验收

- 跨域验签通过；
- 轮换/撤销生效；
- 证据与指标齐备；
- 负向用例：撤销后验签失败、过期失败、恒时比较与 zeroize 生效。
