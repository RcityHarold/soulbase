# Blob × Crypto（Issue 08 对齐）

> 明确与 SB‑17 的对接口径：默认摘要算法、AEAD 策略、HKDF 绑定上下文、日志脱敏与证据记录。

## 摘要与签名默认值

- Digest：默认 `sha256`，用于 Blob ETag 与 Evidence 摘要；
- JWS/COSE：建议默认 `Ed25519 + JWS Detached`；记录 `kid/alg` 于 Evidence；
- 对称加密：推荐 `XChaCha20-Poly1305`（或 `AES-256-GCM`）。

## AEAD 与 HKDF

- AEAD：`seal/open` 接口作为统一门面；
- HKDF 绑定：`ikm=tenant key`、`salt=envelope_id`、`info="blob:{bucket}/{key}"`；
- AAD：建议 `{tenant,bucket,key,content_type}`；
- 安全：恒时比较与 `zeroize` 必开；KeyStore/轮换/撤销记录 `kid` 与生效期。

## 日志与证据

- 日志：不写明文/密钥/签名原文；仅写 `kid/alg/digest/len`；
- Evidence：对 Blob 的 AEAD 操作记录 `alg`、`aad_digest`、`cipher_len` 与 `Digest(sha256)`；
- 指标：`crypto_aead_ms_bucket{alg,op}`、`crypto_digest_total{algo}`、`crypto_sign_ms_bucket{alg}`。

## 验收

- HKDF/AAD 绑定上下文与 SB‑17 一致；
- AEAD seal/open 与日志/证据对齐；
- 默认 `sha256` 摘要与 `Ed25519` 签名口径统一；
- 负向用例：解密失败恒时比较、zeroize 生效、撤销后的 KID 验证失败。
