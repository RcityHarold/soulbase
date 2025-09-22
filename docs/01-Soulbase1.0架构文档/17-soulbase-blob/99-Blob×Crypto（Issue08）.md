# Blob × Crypto（Issue 08 对齐）

> 在不改变原文结构的前提下，统一 ETag/Digest、PUT 幂等锚、可选 AEAD 与日志脱敏策略，并与 SB‑18 对齐。

## ETag 与 Digest 统一

- 规范：`ETag = sha256`（内容哈希，base16/带引号的 S3 兼容形式或标准化为 base64url 存于 `BlobMeta.digest`）。
- Evidence：记录 `Digest{algo="sha256", b64, size}`；A2A/回执仅透出 `BlobRef + Digest`，不透出原文。
- 校验：GET/HEAD 允许 `If-None-Match`；Presign GET 校验 ETag 条件请求。

## PUT 幂等锚

- 锚点：`(bucket, key, sha256)` 三元组；若同 key、同 sha256 重复 PUT，视为幂等；
- Multipart：合并后校验整体 sha256 与 ETag 一致；
- Presign PUT：要求声明 `content-type` 与 `content-length-range`，并携带内容 sha256（或在完成后回传校验）。

## 可选 AEAD（与 SB‑18 对齐）

- 算法：推荐 `XChaCha20-Poly1305`（或 `AES-256-GCM`）；
- 派生：`HKDF(ikm=tenant key, salt=envelope_id, info="blob:{bucket}/{key}")`；
- AAD：包含 `{tenant,bucket,key,content_type}` 等最小必要元信息；
- 记录：Evidence/Meta 仅记录 `alg` 与 `digest`/`len`，不记录明文/密钥；
- 解密：恒时比较（constant‑time）与 `zeroize` 释放敏感内存；
- 兼容：不使用 AEAD 时，保持明文存储但仍强制 `ETag=sha256` 与脱敏日志。

## 日志与审计（脱敏）

- 日志：仅写 `BlobRef{bucket,key,etag,size,content_type}` 与 `Digest`；不写明文/URL/授权头；
- Presign：记录 `method/expire_secs/refhash`，不记完整 URL；
- 观测：`blob_put_total{alg?,tenant}`、`blob_get_total{cache=hit|miss}`、`blob_multipart_total` 等。

## 验收

- ETag 统一为 sha256；
- 相同内容重复 PUT 不产生重复对象（幂等生效）；
- 启用 AEAD 时 seal/open 正确，日志/Evidence 无原文；
- Presign GET/PUT 校验项生效；
- 合同测试覆盖：幂等 PUT、ETag 条件 GET、AEAD seal/open、日志脱敏。
